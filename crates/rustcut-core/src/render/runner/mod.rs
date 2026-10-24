//! One ffmpeg run as a supervised tokio child (ADR-3): its own process group, lowered CPU and I/O priority, progress from
//! `-progress pipe:1`, stderr into the capped [`JobLog`], pause and resume by SIGSTOP/SIGCONT on the group, cancel by
//! `q` -> SIGTERM -> SIGKILL, and a stall watchdog. Dropping the future kills ffmpeg (`kill_on_drop`).

mod log;
mod process;
mod supervise;

use std::ffi::OsString;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::watch;
use tokio::time::Instant;

pub(crate) use log::without_context;
pub use log::{HEAD_LINES, JobLog, MAX_LINE_CHARS, TAIL_LINES};
use process::ProcessGroup;
pub use process::RenderPriority;
pub use supervise::{Action, Control, GroupSignal, QUIT_GRACE, StopReason, Supervisor, TERMINATE_GRACE};

use super::{PlannedPass, ProgressParser, ProgressSample};
use crate::store::LogStream;

/// How long the remaining output may take to arrive after ffmpeg exited.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// How to run the passes.
#[derive(Debug, Clone, Copy)]
pub struct PassOptions<'a> {
    pub program: &'a Path,
    pub priority: RenderPriority,
    /// `None` = no stall watchdog.
    pub stall_timeout: Option<Duration>,
}

/// What the runner reports while a pass runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PassEvent {
    Progress(ProgressSample),
    /// The process group was stopped (`true`) or continued (`false`).
    Paused(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PassError {
    #[error("{program} was not found; install ffmpeg (sudo pacman -S ffmpeg) or set its path in the settings")]
    NotFound { program: String },
    #[error("{program} could not be started: {error}")]
    Spawn { program: String, error: String },
    #[error("canceled")]
    Canceled,
    #[error("stopped after {secs} s without progress")]
    Stalled { secs: u64 },
    #[error("ffmpeg failed ({status}): {message}")]
    Failed { status: String, message: String },
}

/// Runs one pass to its end. `control` carries the owner's wish (a dropped sender cancels); `events` gets progress and
/// pause changes. The command line, ffmpeg's stderr and the runner's own notes go to `log`.
pub async fn run_pass(
    pass: &PlannedPass,
    options: &PassOptions<'_>,
    control: &mut watch::Receiver<Control>,
    log: &mut JobLog,
    mut events: impl FnMut(PassEvent) + Send,
) -> Result<(), PassError> {
    let program = options.program.display().to_string();
    if *control.borrow_and_update() == Control::Cancel {
        return Err(PassError::Canceled);
    }
    log.note(&format!(
        "[Render] {}: {}",
        pass.kind.label(),
        command_line(options.program, &pass.args)
    ));
    let mut command = Command::new(options.program);
    command
        .args(&pass.args)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = crate::process::spawn(&mut command).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PassError::NotFound {
            program: program.clone(),
        },
        _ => PassError::Spawn {
            program: program.clone(),
            error: e.to_string(),
        },
    })?;
    let group = child.id().and_then(ProcessGroup::led_by);
    let mut lowered = false;
    if let Some(group) = group {
        lower(group, options.priority, log);
    }
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take().map(|p| BufReader::new(p).split(b'\n'));
    let mut stderr = child.stderr.take().map(|p| BufReader::new(p).split(b'\n'));
    let mut parser = ProgressParser::new();
    let mut supervisor = Supervisor::new(options.stall_timeout, Instant::now());
    let mut control_open = true;
    let first = *control.borrow_and_update();
    let actions = supervisor.control(first, Instant::now());
    apply(&actions, group, &mut stdin, log, &mut events).await;

    let status = loop {
        let deadline = supervisor.deadline();
        tokio::select! {
            status = child.wait() => break status,
            line = next_line(&mut stdout), if stdout.is_some() => match line {
                Some(line) => {
                    if let Some(sample) = parser.push_line(&line) {
                        supervisor.progress(&sample, Instant::now());
                        if !lowered && let Some(group) = group {
                            // Threads started before the first change kept the old priority: once more for them.
                            lowered = true;
                            lower(group, options.priority, log);
                        }
                        events(PassEvent::Progress(sample));
                    }
                }
                None => stdout = None,
            },
            line = next_line(&mut stderr), if stderr.is_some() => match line {
                Some(line) => log.push(LogStream::Ffmpeg, &line),
                None => stderr = None,
            },
            changed = control.changed(), if control_open => {
                let wanted = if changed.is_ok() {
                    *control.borrow_and_update()
                } else {
                    // The owner is gone: nobody would ever resume or collect this render.
                    control_open = false;
                    Control::Cancel
                };
                let actions = supervisor.control(wanted, Instant::now());
                apply(&actions, group, &mut stdin, log, &mut events).await;
            },
            () = sleep_until(deadline), if deadline.is_some() => {
                let actions = supervisor.tick(Instant::now());
                if supervisor.stop_reason() == Some(StopReason::Stalled) && actions.contains(&Action::Quit) {
                    log.note("[Render] No progress for the stall timeout; stopping ffmpeg");
                }
                apply(&actions, group, &mut stdin, log, &mut events).await;
            },
        }
    };
    drop(stdin);
    // Progress written just before the exit (often the final `progress=end`) is read here, after `wait` returned.
    for sample in drain(&mut stdout, &mut stderr, &mut parser, log).await {
        events(PassEvent::Progress(sample));
    }

    match supervisor.stop_reason() {
        Some(StopReason::Canceled) => {
            log.note("[Render] Canceled");
            Err(PassError::Canceled)
        }
        Some(StopReason::Stalled) => {
            let secs = options.stall_timeout.unwrap_or_default().as_secs();
            Err(PassError::Stalled { secs })
        }
        None => match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(PassError::Failed {
                status: describe(status),
                message: log.last_error().unwrap_or_else(|| "no message".to_owned()),
            }),
            Err(e) => Err(PassError::Failed {
                status: "no exit status".to_owned(),
                message: e.to_string(),
            }),
        },
    }
}

fn lower(group: ProcessGroup, priority: RenderPriority, log: &mut JobLog) {
    for problem in group.lower_priority(priority) {
        tracing::warn!("[Render] {problem}");
        log.note(&format!("[Render] {problem}"));
    }
}

async fn apply(
    actions: &[Action],
    group: Option<ProcessGroup>,
    stdin: &mut Option<ChildStdin>,
    log: &mut JobLog,
    events: &mut (impl FnMut(PassEvent) + Send),
) {
    for &action in actions {
        match action {
            Action::Quit => {
                let written = match stdin.as_mut() {
                    Some(pipe) => tokio::time::timeout(Duration::from_secs(1), pipe.write_all(b"q"))
                        .await
                        .map_err(|_elapsed| "timed out".to_owned())
                        .and_then(|r| r.map_err(|e| e.to_string())),
                    None => Err("stdin is closed".to_owned()),
                };
                if let Err(e) = written {
                    log.note(&format!("[Render] `q` not delivered ({e}); the next step follows"));
                }
            }
            Action::Signal(signal) => {
                let Some(group) = group else { continue };
                match group.signal(signal) {
                    Ok(()) => {
                        log.note(&format!("[Render] Sent {signal:?} to ffmpeg"));
                        match signal {
                            GroupSignal::Stop => events(PassEvent::Paused(true)),
                            GroupSignal::Continue => events(PassEvent::Paused(false)),
                            GroupSignal::Terminate | GroupSignal::Kill => {}
                        }
                    }
                    // ESRCH: it already ended; the exit status follows.
                    Err(e) => log.note(&format!("[Render] {signal:?} not delivered: {e}")),
                }
            }
        }
    }
}

type Lines<R> = tokio::io::Split<BufReader<R>>;

/// The next line of a pipe (lossy UTF-8, without `\r`); `None` at the end or on a read error.
async fn next_line<R: tokio::io::AsyncRead + Unpin>(pipe: &mut Option<Lines<R>>) -> Option<String> {
    let segment = pipe.as_mut()?.next_segment().await.ok().flatten()?;
    let text = String::from_utf8_lossy(&segment);
    Some(text.trim_end_matches('\r').to_owned())
}

/// Reads what is left in the pipes after the exit (bounded, a stray grandchild could keep them open) and returns the
/// progress samples found in it.
async fn drain<O, E>(
    stdout: &mut Option<Lines<O>>,
    stderr: &mut Option<Lines<E>>,
    parser: &mut ProgressParser,
    log: &mut JobLog,
) -> Vec<ProgressSample>
where
    O: tokio::io::AsyncRead + Unpin,
    E: tokio::io::AsyncRead + Unpin,
{
    let mut samples = Vec::new();
    let rest = async {
        while let Some(line) = next_line(stdout).await {
            samples.extend(parser.push_line(&line));
        }
        while let Some(line) = next_line(stderr).await {
            log.push(LogStream::Ffmpeg, &line);
        }
    };
    if tokio::time::timeout(DRAIN_TIMEOUT, rest).await.is_err() {
        tracing::debug!("[Render] ffmpeg's pipes stayed open after its exit");
    }
    samples
}

async fn sleep_until(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    }
}

fn describe(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        (None, None) => status.to_string(),
    }
}

/// The command as a shell line for the log (copy it into a terminal to reproduce the pass).
pub fn command_line(program: &Path, args: &[OsString]) -> String {
    let words: Vec<String> = std::iter::once(program.as_os_str())
        .chain(args.iter().map(OsString::as_os_str))
        .map(|w| w.to_string_lossy().into_owned())
        .collect();
    shlex::try_join(words.iter().map(String::as_str)).unwrap_or_else(|_nul| words.join(" "))
}

#[cfg(test)]
mod tests;
