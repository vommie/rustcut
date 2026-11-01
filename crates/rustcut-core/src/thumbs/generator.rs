//! Who makes the pictures: the helper program, or ffmpeg one picture at a time when it cannot start (user decision Q-P12e).
//! Both answer with the helper's replies, tagged with the run that asked, so a late answer of a stopped run is dropped.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rustcut_thumbs_protocol::{Command, Job, Reply, Start, fit, read_reply};
use tokio::sync::{Notify, mpsc};

use crate::media::probe::run_as;

/// Pictures at the same time in the fallback.
const FALLBACK_PARALLEL: usize = 2;
const FALLBACK_TIMEOUT: Duration = Duration::from_secs(15);

/// A message of a run.
#[derive(Debug)]
pub(super) enum Arrived {
    Reply(Reply, Vec<u8>),
    /// The helper's stdout ended.
    Closed,
}

pub(super) type Arrivals = mpsc::UnboundedSender<(u64, Arrived)>;

/// The helper's program: `RUSTCUT_THUMBS_HELPER`, next to RustCut (a build), or where the package puts it.
pub fn helper_program() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUSTCUT_THUMBS_HELPER") {
        return Some(PathBuf::from(path));
    }
    let beside = std::env::current_exe().ok()?.with_file_name("rustcut-thumbs");
    let installed = Path::new("/usr/lib/rustcut/rustcut-thumbs");
    [beside.as_path(), installed]
        .iter()
        .find(|path| path.is_file())
        .map(|path| path.to_path_buf())
}

pub(super) struct Helper {
    child: Child,
    stdin: ChildStdin,
}

impl Helper {
    /// Starts the program and tells it the video. Replies arrive on a thread of their own.
    pub(super) fn start(program: &Path, start: &Start, run: u64, arrivals: Arrivals) -> std::io::Result<Self> {
        let mut child = Process::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(std::io::Error::other("the helper's pipes are missing"));
        };
        let line = serde_json::to_string(start).map_err(std::io::Error::other)?;
        writeln!(stdin, "{line}")?;
        std::thread::Builder::new()
            .name("rustcut-thumbs-read".into())
            .spawn(move || {
                let mut input = BufReader::new(stdout);
                loop {
                    match read_reply(&mut input) {
                        Ok((reply, picture)) => {
                            if arrivals.send((run, Arrived::Reply(reply, picture))).is_err() {
                                return;
                            }
                        }
                        Err(_closed_or_broken) => {
                            let _ = arrivals.send((run, Arrived::Closed));
                            return;
                        }
                    }
                }
            })?;
        Ok(Self { child, stdin })
    }

    fn send(&mut self, command: &Command) {
        // The pipe's end only goes away with the helper, and the reader reports that.
        if let Ok(line) = serde_json::to_string(command) {
            let _ = writeln!(self.stdin, "{line}");
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The work of the fallback: what the pointer wants first.
#[derive(Default)]
struct Line {
    front: VecDeque<(u64, bool)>,
    back: VecDeque<(u64, bool)>,
    running: usize,
}

/// One ffmpeg per picture, on the runtime. The size is known from the probe.
pub(super) struct Fallback {
    line: Arc<Mutex<Line>>,
    wake: Arc<Notify>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

/// The fallback's fixed facts.
pub(super) struct FallbackVideo {
    pub ffmpeg: PathBuf,
    pub path: PathBuf,
    pub size: (u32, u32),
    pub aspect: (i32, i32),
    pub long_side: u32,
    pub quality: u8,
}

fn targets(job: Job) -> Vec<(u64, bool)> {
    match job {
        Job::Key { ms } => vec![(ms, true)],
        Job::Exact { ms } => vec![(ms, false)],
        Job::Span { targets, .. } => targets.into_iter().map(|ms| (ms, false)).collect(),
    }
}

impl Fallback {
    pub(super) fn start(video: FallbackVideo, run: u64, arrivals: &Arrivals) -> Self {
        let line = Arc::new(Mutex::new(Line::default()));
        let wake = Arc::new(Notify::new());
        let video = Arc::new(video);
        let tasks = (0..FALLBACK_PARALLEL)
            .map(|_| {
                let (line, wake, video, arrivals) = (
                    Arc::clone(&line),
                    Arc::clone(&wake),
                    Arc::clone(&video),
                    arrivals.clone(),
                );
                tokio::spawn(async move {
                    loop {
                        let next = {
                            let mut line = line.lock();
                            let next = line.front.pop_front().or_else(|| line.back.pop_front());
                            if next.is_some() {
                                line.running += 1;
                            }
                            next
                        };
                        let Some((ms, fast)) = next else {
                            wake.notified().await;
                            continue;
                        };
                        let answer = one_picture(&video, ms, fast).await;
                        if arrivals.send((run, answer)).is_err() {
                            return;
                        }
                        let idle = {
                            let mut line = line.lock();
                            line.running = line.running.saturating_sub(1);
                            line.running == 0 && line.front.is_empty() && line.back.is_empty()
                        };
                        if idle {
                            let _ = arrivals.send((run, Arrived::Reply(Reply::Idle, Vec::new())));
                        }
                    }
                })
            })
            .collect();
        let _ = arrivals.send((run, Arrived::Reply(Reply::Ready { duration_ms: 0 }, Vec::new())));
        Self { line, wake, tasks }
    }
}

impl Drop for Fallback {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn one_picture(video: &FallbackVideo, ms: u64, fast: bool) -> Arrived {
    let missing = Arrived::Reply(Reply::Missing { target_ms: ms }, Vec::new());
    let (width, height) = fit(video.size.0, video.size.1, video.aspect, video.long_side);
    let seconds = format!("{}.{:03}", ms / 1000, ms % 1000);
    let mut args: Vec<OsString> = ["-v", "error", "-hide_banner", "-nostats", "-nostdin"]
        .map(Into::into)
        .into();
    if fast {
        args.extend(["-noaccurate_seek", "-skip_frame", "nokey"].map(Into::into));
    }
    args.extend(["-threads", "1", "-ss", &seconds, "-i"].map(Into::into));
    args.push(video.path.clone().into_os_string());
    args.extend(["-map", "0:v:0", "-frames:v", "1", "-vf"].map(Into::into));
    args.push(format!("scale={width}:{height}:flags=bicubic").into());
    args.extend(["-an", "-sn", "-dn", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"].map(Into::into));
    let size = u64::from(width) * u64::from(height) * 3;
    let Ok(output) = run_as(&video.ffmpeg, args, FALLBACK_TIMEOUT, size, true).await else {
        return missing;
    };
    if u64::try_from(output.stdout.len()).ok() != Some(size) {
        return missing;
    }
    let quality = f32::from(video.quality.min(100));
    let rgb = output.stdout;
    let encoded =
        tokio::task::spawn_blocking(move || webp::Encoder::from_rgb(&rgb, width, height).encode(quality).to_vec());
    let Ok(webp) = encoded.await else { return missing };
    Arrived::Reply(
        Reply::Thumb {
            target_ms: ms,
            time_ms: ms,
            exact: !fast,
            width,
            height,
        },
        webp,
    )
}

/// The running generator.
pub(super) enum Generator {
    Helper(Helper),
    Fallback(Fallback),
}

impl Generator {
    pub(super) fn add(&mut self, jobs: Vec<Job>) {
        match self {
            Self::Helper(helper) => helper.send(&Command::Add(jobs)),
            Self::Fallback(fallback) => {
                fallback.line.lock().back.extend(jobs.into_iter().flat_map(targets));
                fallback.wake.notify_waiters();
            }
        }
    }

    /// Goes first and replaces the last one that has not started.
    pub(super) fn front(&mut self, jobs: Vec<Job>) {
        match self {
            Self::Helper(helper) => helper.send(&Command::Front(jobs)),
            Self::Fallback(fallback) => {
                fallback.line.lock().front = jobs.into_iter().flat_map(targets).collect();
                fallback.wake.notify_waiters();
            }
        }
    }

    pub(super) fn clear(&mut self) {
        match self {
            Self::Helper(helper) => helper.send(&Command::Clear),
            Self::Fallback(fallback) => {
                let mut line = fallback.line.lock();
                line.front.clear();
                line.back.clear();
            }
        }
    }
}
