//! The hash actor (ADR-4, ADR-6): one task on the runtime owns the queue; each file is read on a blocking thread
//! (`spawn_blocking`), never on the UI thread. A sidecar `.<name>.md5` answers without reading the video; otherwise the
//! MD5 is computed in 1 MiB chunks with a cancel check between chunks and progress at most every [`PROGRESS_INTERVAL`].
//! One file is hashed at a time (two parallel reads of big files only slow each other down); requests for the file being
//! hashed or already waiting share its result. Computed hashes are remembered for the session by size and modification
//! time, so opening a file again costs nothing; with `SetWriteSidecar(true)` a computed hash also goes into a new sidecar
//! next to the video, the place other programs share (user decision 2026-10-24; an existing sidecar is never replaced).
//! A read tells the kernel it is sequential (`posix_fadvise`: larger read-ahead) and runs at the lowest best-effort drive
//! priority, so the player reading the same video comes first.

use std::collections::{HashMap, VecDeque};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use md5::{Digest as _, Md5};
use tokio::task::JoinHandle;

use super::{HashCommand, HashError, HashEvent, parse_sidecar, sidecar_path};
use crate::actor::{EventBus, Handle, Mailbox, Reply, mailbox};

pub type HashHandle = Handle<HashCommand>;

/// Progress events while hashing come at most this often (the UI coalesces them again at `rate::PROGRESS`).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);
/// Read size: big enough for full disk speed, small enough that a cancel takes effect at once.
const CHUNK: usize = 1024 * 1024;
/// A sidecar holds 32 hex digits and maybe a file name; anything longer is not one.
const MAX_SIDECAR: u64 = 4096;
/// How long a shutdown waits for a running read to notice the cancel.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// The running hash service.
#[derive(Debug, Clone)]
pub struct HashService {
    pub handle: HashHandle,
    pub events: EventBus<HashEvent>,
}

/// Starts the hash actor on `runtime`.
pub fn spawn_hash_service(runtime: &tokio::runtime::Handle) -> (HashService, JoinHandle<()>) {
    let (handle, rx) = mailbox();
    let events = EventBus::new();
    let actor = Actor {
        events: events.clone(),
        waiting: VecDeque::new(),
        active: None,
        computed: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        write_sidecar: false,
    };
    let task = runtime.spawn(actor.run(rx));
    (HashService { handle, events }, task)
}

type Answer = Reply<Result<String, HashError>>;

struct Waiting {
    path: PathBuf,
    replies: Vec<Answer>,
}

struct Active {
    path: PathBuf,
    replies: Vec<Answer>,
    cancel: Arc<AtomicBool>,
    task: JoinHandle<Result<Hashed, HashError>>,
}

/// A computed hash and the file state it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Remembered {
    len: u64,
    modified: Option<SystemTime>,
    md5: String,
}

type Memory = Arc<parking_lot::Mutex<HashMap<PathBuf, Remembered>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hashed {
    md5: String,
    from_sidecar: bool,
}

struct Actor {
    events: EventBus<HashEvent>,
    waiting: VecDeque<Waiting>,
    active: Option<Active>,
    computed: Memory,
    /// A computed hash goes into a new sidecar.
    write_sidecar: bool,
}

enum Step {
    Command(Option<HashCommand>),
    Done(Result<Result<Hashed, HashError>, tokio::task::JoinError>),
}

impl Actor {
    async fn run(mut self, mut rx: Mailbox<HashCommand>) {
        tracing::debug!("[Hash] Service started");
        loop {
            let step = {
                let active = self.active.as_mut().map(|a| &mut a.task);
                tokio::select! {
                    command = rx.recv() => Step::Command(command),
                    done = async {
                        match active {
                            Some(task) => task.await,
                            None => std::future::pending().await,
                        }
                    } => Step::Done(done),
                }
            };
            match step {
                Step::Command(Some(HashCommand::Hash { path, reply })) => self.enqueue(path, reply),
                Step::Command(Some(HashCommand::Cancel { path })) => self.cancel(&path),
                Step::Command(Some(HashCommand::SetWriteSidecar(on))) => self.write_sidecar = on,
                Step::Command(Some(HashCommand::Shutdown { reply })) => {
                    self.shut_down().await;
                    let _ = reply.send(());
                    break;
                }
                Step::Command(None) => {
                    self.shut_down().await;
                    break;
                }
                Step::Done(result) => {
                    if let Some(active) = self.active.take() {
                        let result = result.unwrap_or_else(|e| {
                            Err(HashError::Io {
                                path: active.path.clone(),
                                error: format!("the hashing task failed: {e}"),
                            })
                        });
                        self.finish(active.path, active.replies, result);
                    }
                    self.start_next();
                }
            }
        }
        tracing::debug!("[Hash] Service stopped");
    }

    fn enqueue(&mut self, path: PathBuf, reply: Option<Answer>) {
        if let Some(active) = self.active.as_mut().filter(|a| a.path == path) {
            active.replies.extend(reply);
            return;
        }
        if let Some(waiting) = self.waiting.iter_mut().find(|w| w.path == path) {
            waiting.replies.extend(reply);
            return;
        }
        self.waiting.push_back(Waiting {
            path,
            replies: reply.into_iter().collect(),
        });
        self.start_next();
    }

    fn start_next(&mut self) {
        if self.active.is_some() {
            return;
        }
        let Some(next) = self.waiting.pop_front() else { return };
        let cancel = Arc::new(AtomicBool::new(false));
        let events = self.events.clone();
        let memory = Arc::clone(&self.computed);
        let path = next.path.clone();
        let flag = Arc::clone(&cancel);
        let write_sidecar = self.write_sidecar;
        tracing::debug!(path = %path.display(), "[Hash] Started");
        let task = tokio::task::spawn_blocking(move || {
            let mut last = None::<Instant>;
            hash_file(&path, &flag, &memory, write_sidecar, |done, total| {
                let due = last.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) || done == total;
                if due {
                    last = Some(Instant::now());
                    events.publish(HashEvent::Progress {
                        path: path.clone(),
                        done,
                        total,
                    });
                }
            })
        });
        self.active = Some(Active {
            path: next.path,
            replies: next.replies,
            cancel,
            task,
        });
    }

    fn cancel(&mut self, path: &Path) {
        if let Some(active) = self.active.as_ref().filter(|a| a.path == path) {
            active.cancel.store(true, Ordering::Relaxed);
            return;
        }
        if let Some(position) = self.waiting.iter().position(|w| w.path == path)
            && let Some(waiting) = self.waiting.remove(position)
        {
            self.finish(waiting.path, waiting.replies, Err(HashError::Canceled));
        }
    }

    /// Cancels everything; waits a moment for a running read to stop.
    async fn shut_down(&mut self) {
        for waiting in std::mem::take(&mut self.waiting) {
            self.finish(waiting.path, waiting.replies, Err(HashError::Canceled));
        }
        if let Some(mut active) = self.active.take() {
            active.cancel.store(true, Ordering::Relaxed);
            if tokio::time::timeout(SHUTDOWN_WAIT, &mut active.task).await.is_err() {
                tracing::warn!(path = %active.path.display(), "[Hash] A read did not stop in time");
            }
            self.finish(active.path, active.replies, Err(HashError::Canceled));
        }
    }

    fn finish(&self, path: PathBuf, replies: Vec<Answer>, result: Result<Hashed, HashError>) {
        let event = match &result {
            Ok(hashed) => {
                tracing::info!(path = %path.display(), md5 = %hashed.md5, sidecar = hashed.from_sidecar, "[Hash] Done");
                HashEvent::Finished {
                    path,
                    md5: hashed.md5.clone(),
                    from_sidecar: hashed.from_sidecar,
                }
            }
            Err(HashError::Canceled) => {
                tracing::info!(path = %path.display(), "[Hash] Canceled");
                HashEvent::Canceled { path }
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Hash] Failed");
                HashEvent::Failed {
                    path,
                    error: e.to_string(),
                }
            }
        };
        for reply in replies {
            let _ = reply.send(result.clone().map(|h| h.md5));
        }
        self.events.publish(event);
    }
}

/// Sidecar, remembered hash, or MD5 of the whole file (blocking; runs on a blocking thread). `progress(done, total)` is
/// called with 0 before the first read and after every chunk.
fn hash_file(
    path: &Path,
    cancel: &AtomicBool,
    memory: &Memory,
    write_sidecar: bool,
    mut progress: impl FnMut(u64, u64),
) -> Result<Hashed, HashError> {
    let io = |error: std::io::Error| HashError::Io {
        path: path.to_path_buf(),
        error: error.to_string(),
    };
    if let Some(md5) = read_sidecar(path) {
        return Ok(Hashed {
            md5,
            from_sidecar: true,
        });
    }
    let file = std::fs::File::open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    let (len, modified) = (meta.len(), meta.modified().ok());
    if let Some(known) = memory
        .lock()
        .get(path)
        .filter(|r| r.len == len && r.modified == modified)
    {
        return Ok(Hashed {
            md5: known.md5.clone(),
            from_sidecar: false,
        });
    }
    // Sequential: the kernel reads further ahead (fewer, larger requests); the drive serves the player first.
    if let Err(e) = rustix::fs::fadvise(&file, 0, None, rustix::fs::Advice::Sequential) {
        tracing::debug!(error = %e, "[Hash] No read-ahead advice");
    }
    let md5 = {
        let _low = LowDrivePriority::here();
        md5_of(file, len, cancel, &mut progress).map_err(|e| match e {
            ReadError::Canceled => HashError::Canceled,
            ReadError::Io(e) => io(e),
        })?
    };
    if write_sidecar {
        match write_new_sidecar(path, &md5) {
            Ok(Some(sidecar)) => tracing::info!(sidecar = %sidecar.display(), "[Hash] Sidecar written"),
            Ok(None) => tracing::debug!(path = %path.display(), "[Hash] A sidecar appeared meanwhile; kept"),
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "[Hash] Sidecar not written"),
        }
    }
    memory.lock().insert(
        path.to_path_buf(),
        Remembered {
            len,
            modified,
            md5: md5.clone(),
        },
    );
    Ok(Hashed {
        md5,
        from_sidecar: false,
    })
}

/// The sidecar's hash; `None` when there is none or it does not hold one (logged).
fn read_sidecar(video: &Path) -> Option<String> {
    let sidecar = sidecar_path(video)?;
    let file = std::fs::File::open(&sidecar).ok()?;
    let mut text = String::new();
    if let Err(e) = file.take(MAX_SIDECAR).read_to_string(&mut text) {
        tracing::warn!(sidecar = %sidecar.display(), error = %e, "[Hash] Sidecar not readable; computing instead");
        return None;
    }
    let hash = parse_sidecar(&text);
    if hash.is_none() {
        tracing::warn!(sidecar = %sidecar.display(), "[Hash] Sidecar holds no MD5; computing instead");
    }
    hash
}

/// Writes `.<name>.md5` next to `video` in md5sum's format (`<md5>  <name>`, so `md5sum -c` checks it), through a hidden
/// temp file and a rename that never replaces: an existing sidecar stays (`Ok(None)`). Blocking.
fn write_new_sidecar(video: &Path, md5: &str) -> Result<Option<PathBuf>, String> {
    let sidecar = sidecar_path(video).ok_or("the file name is not UTF-8")?;
    if sidecar.symlink_metadata().is_ok() {
        return Ok(None);
    }
    let name = video
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the file name is not UTF-8")?;
    let temp = sidecar.with_file_name(format!(".{name}.md5.rustcut-part"));
    let written = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        std::io::Write::write_all(&mut file, format!("{md5}  {name}\n").as_bytes())?;
        file.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e.to_string());
    }
    let renamed = crate::render::rename_no_replace(&temp, &sidecar);
    let _ = std::fs::remove_file(&temp);
    match renamed {
        Ok(()) => Ok(Some(sidecar)),
        Err(crate::render::FinalizeError::TargetExists(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// The calling thread at the lowest best-effort drive priority until dropped: a blocking-pool thread gets its old
/// priority back (the niceness is left alone, since raising it again would need privileges).
struct LowDrivePriority(Option<(ioprio::Target, ioprio::Priority)>);

impl LowDrivePriority {
    fn here() -> Self {
        let tid = rustix::thread::gettid();
        let target = ioprio::Target::Process(ioprio::Pid::from_raw(tid.as_raw_nonzero().get()));
        let Ok(old) = ioprio::get_priority(target) else {
            return Self(None);
        };
        let low = ioprio::Priority::new(ioprio::Class::BestEffort(ioprio::BePriorityLevel::lowest()));
        match ioprio::set_priority(target, low) {
            Ok(()) => Self(Some((target, old))),
            Err(e) => {
                tracing::debug!(error = %e, "[Hash] Drive priority not lowered");
                Self(None)
            }
        }
    }
}

impl Drop for LowDrivePriority {
    fn drop(&mut self) {
        if let Some((target, old)) = self.0
            && let Err(e) = ioprio::set_priority(target, old)
        {
            tracing::debug!(error = %e, "[Hash] Drive priority not restored");
        }
    }
}

enum ReadError {
    Canceled,
    Io(std::io::Error),
}

/// MD5 of everything `reader` yields, in lowercase hex.
fn md5_of(
    mut reader: impl std::io::Read,
    total: u64,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
) -> Result<String, ReadError> {
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut done = 0u64;
    progress(0, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ReadError::Canceled);
        }
        let n = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ReadError::Io(e)),
        };
        hasher.update(buffer.get(..n).unwrap_or_default());
        done = done.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        // A file that grows while it is read reports more than its size; the total follows.
        progress(done, total.max(done));
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
