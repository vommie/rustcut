//! RustCut's thumbnail helper (P12 block 6, Q-P12e): one process per video, so a decoder that crashes on a broken file
//! cannot take the editor along, a new FFmpeg cannot keep RustCut from starting, its memory goes back when it ends, and
//! its priority applies to all of it. The protocol is `rustcut_thumbs_protocol`; the end of stdin ends the helper.

mod picture;
mod queue;
mod source;

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;
use rustcut_thumbs_protocol::{Command, Job, Reply, Start, write_reply};

use queue::{Next, Queue};
use source::{Outcome, Source};

/// The replies of all workers go through here, one frame at a time.
type Output = Arc<Mutex<io::Stdout>>;

fn send(out: &Output, reply: &Reply, picture: &[u8]) {
    let mut stdout = out.lock();
    if write_reply(&mut *stdout, reply, picture).is_err() {
        // RustCut is gone, and so is everyone who wanted the rest.
        #[expect(
            clippy::exit,
            reason = "a worker thread ends the whole helper; nothing here needs cleaning up"
        )]
        std::process::exit(0);
    }
}

fn say(out: &Output, outcome: Outcome) {
    match outcome {
        Outcome::Thumb { reply, webp } => send(out, &reply, &webp),
        Outcome::Plain(reply) => send(out, &reply, &[]),
    }
}

/// Niceness and drive priority for this thread and all it starts. Thumbnails go ahead of renders, which use the idle
/// class, and behind the player. A failure only costs speed elsewhere.
fn lower_priority(nice: i32) {
    let own = rustix::process::getpriority_process(None).unwrap_or(0);
    let _ignored = rustix::process::setpriority_process(None, nice.max(own));
    let drive = ioprio::Priority::new(ioprio::Class::BestEffort(ioprio::BePriorityLevel::lowest()));
    let _ignored = ioprio::set_priority(ioprio::Target::Process(ioprio::Pid::this()), drive);
}

fn work(queue: &Queue, source: &mut Source, out: &Output) {
    while let Next::Job(job) = queue.next() {
        match job {
            Job::Key { ms } => say(out, source.keyframe(ms)),
            Job::Exact { ms } => say(out, source.exact(ms)),
            Job::Span { start_ms, targets } => {
                let left = source.span(start_ms, &targets, &mut |outcome| say(out, outcome), &|| {
                    queue.front_waiting()
                });
                if let Some(&next) = left.first() {
                    queue.resume(Job::Span {
                        start_ms: next,
                        targets: left,
                    });
                }
            }
        }
        if queue.finished() {
            send(out, &Reply::Idle, &[]);
        }
    }
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let first = lines.next().ok_or("no start line")?.map_err(|e| e.to_string())?;
    let start: Start = serde_json::from_str(&first).map_err(|e| e.to_string())?;
    lower_priority(start.nice);
    ffmpeg_next::init().map_err(|e| e.to_string())?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Quiet);

    let path = PathBuf::from(&start.path);
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let out: Output = Arc::new(Mutex::new(io::stdout()));
    let queue = Arc::new(Queue::default());
    let first_source =
        Source::open(&path, start.long_side, start.quality, Arc::clone(&seen)).map_err(|e| e.to_string())?;
    send(
        &out,
        &Reply::Ready {
            duration_ms: first_source.duration_ms(),
        },
        &[],
    );
    let mut workers = Vec::new();
    let mut spare = Some(first_source);
    for _ in 0..start.workers.max(1) {
        let (queue, out, seen, path) = (Arc::clone(&queue), Arc::clone(&out), Arc::clone(&seen), path.clone());
        let opened = spare.take();
        let (long_side, quality) = (start.long_side, start.quality);
        workers.push(thread::spawn(move || {
            let source = opened.map_or_else(|| Source::open(&path, long_side, quality, seen), Ok);
            if let Ok(mut source) = source {
                work(&queue, &mut source, &out);
            }
        }));
    }

    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        match serde_json::from_str::<Command>(&line).map_err(|e| e.to_string())? {
            Command::Add(jobs) => queue.add(jobs),
            Command::Front(jobs) => queue.front(jobs),
            Command::Clear => queue.clear(),
            Command::Quit => break,
        }
    }
    // Stdin ended or RustCut said so: a decoder in the middle of a frame has nothing left to give.
    queue.close();
    let _ignored = io::stdout().flush();
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let out: Output = Arc::new(Mutex::new(io::stdout()));
            send(&out, &Reply::Failed { message }, &[]);
            ExitCode::FAILURE
        }
    }
}
