//! The hash actor (ADR-4, ADR-6): one task on the runtime owns the queue; each file is read on a blocking thread
//! (`spawn_blocking`), never on the UI thread. A sidecar `.<name>.md5` answers without reading the video; otherwise the
//! MD5 is computed in 1 MiB chunks with a cancel check between chunks and progress at most every [`PROGRESS_INTERVAL`].
//! One file is hashed at a time (two parallel reads of big files only slow each other down); requests for the file being
//! hashed or already waiting share its result. Computed hashes are remembered for the session by size and modification
//! time, so opening a file again costs nothing; with `SetWriteSidecar(true)` a computed hash also goes into the sidecar
//! next to the video, the place other programs share (user decision 2026-10-24). Sidecars follow the rules shared with
//! RustKnown and RustTransferFrom (crate `md5-sidecar`, 2026-11-04): only a sidecar that is not older than its video
//! answers, a written one holds the bare hash, and one with the right hash is never rewritten.
//! A read tells the kernel it is sequential (`posix_fadvise`: larger read-ahead) and runs at the lowest best-effort drive
//! priority, so the player reading the same video comes first.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use md5::{Digest as _, Md5};
use tokio::task::JoinHandle;

use md5_sidecar::{Lookup, Md5Hex, Stamp, Written};

use super::{HashCommand, HashError, HashEvent, sidecar_path};
use crate::actor::{EventBus, Handle, Mailbox, Reply, mailbox};

pub type HashHandle = Handle<HashCommand>;

/// Progress events while hashing come at most this often (the UI coalesces them again at `rate::PROGRESS`).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);
/// Read size: big enough for full disk speed, small enough that a cancel takes effect at once.
const CHUNK: usize = 1024 * 1024;
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
    let file = std::fs::File::open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    let (len, modified) = (meta.len(), meta.modified().ok());
    if let Some(md5) = read_sidecar(path, modified) {
        return Ok(Hashed {
            md5,
            from_sidecar: true,
        });
    }
    // Taken before the bytes are read: a video that changes while it is hashed gets no sidecar.
    let stamp = Stamp::from_metadata(&meta).ok();
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
        match store_sidecar(path, &md5, stamp) {
            Ok(Written::Unchanged) => {
                tracing::debug!(path = %path.display(), "[Hash] The sidecar already holds this hash; kept");
            }
            Ok(_) => tracing::info!(path = %path.display(), "[Hash] Sidecar written"),
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

/// The sidecar's hash when it is valid and not older than the video (md5-sidecar rules 3 and 4); `None` otherwise
/// (logged). `modified` is the video's modification time.
fn read_sidecar(video: &Path, modified: Option<SystemTime>) -> Option<String> {
    let found = match modified {
        Some(modified) => md5_sidecar::lookup_at(video, modified),
        None => md5_sidecar::lookup(video),
    };
    let sidecar = sidecar_path(video).unwrap_or_default();
    match found {
        Ok(Lookup::Fresh(hash)) => Some(hash.to_string()),
        Ok(Lookup::Missing) => None,
        Ok(Lookup::Stale(_)) => {
            tracing::info!(sidecar = %sidecar.display(), "[Hash] Sidecar older than the video; computing instead");
            None
        }
        Ok(Lookup::Invalid(why)) => {
            tracing::warn!(sidecar = %sidecar.display(), reason = %why, "[Hash] Sidecar holds no MD5; computing instead");
            None
        }
        Err(e) => {
            tracing::warn!(sidecar = %sidecar.display(), error = %e, "[Hash] Sidecar not readable; computing instead");
            None
        }
    }
}

/// Writes `video`'s sidecar (md5-sidecar rule 5): the bare 32 hex digits, atomically. A sidecar that already holds this
/// hash and is not older than the video stays; a missing, invalid or older one is written; one with another hash is
/// replaced (the new hash comes from the bytes). `stamp` was taken before the bytes were read, so a video that changed
/// meanwhile gets none. Blocking.
fn store_sidecar(video: &Path, md5: &str, stamp: Option<Stamp>) -> Result<Written, String> {
    let hash: Md5Hex = md5.parse().map_err(|e: md5_sidecar::Invalid| e.to_string())?;
    let stamp = stamp.ok_or("the video's modification time is unknown")?;
    md5_sidecar::write(video, hash, stamp).map_err(|e| e.to_string())
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
