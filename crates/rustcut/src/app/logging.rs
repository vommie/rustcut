//! Logging setup: `tracing` to stderr and to daily rotating files in the profile's state folder (`state/logs/`, the newest
//! `advanced.log_keep_days` files are kept). The level comes from `RUST_LOG` when set, otherwise from the setting
//! `advanced.log_level` once the settings are read (`info` until then).
//!
//! Logging starts before the settings are read, so the file layer buffers what is logged until [`LogControl::open_file`]
//! knows the folder and the retention; then the buffer goes first into the file. The file is written by
//! `tracing-appender`'s background thread, never by the thread that logs.
//!
//! Beside it, the shareable copy (P9.17, on by default, `advanced.shareable_log`): the same lines with file names,
//! folders, tags, user and host, MD5 sums and titles replaced (`app::redaction`), in `rustcut-shareable.DATE.log`.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

use super::log_book::LogBook;

/// Lines logged before the file is open are kept up to this size (startup logs a few kilobytes).
const EARLY_BUFFER_LIMIT: usize = 1 << 20;

/// File names: `rustcut.2026-10-23.log`, the shareable copy `rustcut-shareable.2026-10-23.log`.
const FILE_PREFIX: &str = "rustcut";
const SHAREABLE_PREFIX: &str = "rustcut-shareable";
const FILE_SUFFIX: &str = "log";

/// Changes the log filter after startup and owns the log file's writer thread (dropping it flushes the file).
pub struct LogControl {
    handle: Option<reload::Handle<EnvFilter, Registry>>,
    from_env: bool,
    file: FileSink,
    guard: Arc<Mutex<Option<WorkerGuard>>>,
    shareable: FileSink,
    shareable_guard: Arc<Mutex<Option<WorkerGuard>>>,
    book: LogBook,
}

/// Changes the log filter and the log files while RustCut runs.
#[derive(Clone)]
pub struct LevelSetter {
    handle: Option<reload::Handle<EnvFilter, Registry>>,
    from_env: bool,
    file: FileSink,
    guard: Arc<Mutex<Option<WorkerGuard>>>,
    shareable: FileSink,
    shareable_guard: Arc<Mutex<Option<WorkerGuard>>>,
}

impl LevelSetter {
    /// Writes the daily files to `dir` again, keeping the newest `keep_days` (older ones are deleted now), with the
    /// shareable copy or without it.
    pub fn reopen(&self, dir: &Path, keep_days: u16, shareable: bool) {
        open_file(&self.file, &self.guard, dir, keep_days, FILE_PREFIX);
        open_shareable(&self.shareable, &self.shareable_guard, dir, keep_days, shareable);
    }

    /// Uses `level` unless `RUST_LOG` was given (the environment wins).
    pub fn apply(&self, level: &str) {
        if self.from_env {
            return;
        }
        let Some(handle) = &self.handle else { return };
        match EnvFilter::try_new(level) {
            Ok(filter) => {
                if let Err(e) = handle.reload(filter) {
                    tracing::warn!(error = %e, "[App] The log level could not be changed");
                } else {
                    tracing::debug!(level, "[App] Log level from the settings");
                }
            }
            Err(e) => tracing::warn!(level, error = %e, "[App] The configured log level is not valid; keeping info"),
        }
    }
}

/// Installs the global subscriber. A second call (tests) leaves the first one in place.
pub fn init() -> LogControl {
    let from_env = std::env::var_os("RUST_LOG").is_some();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (layer, handle) = reload::Layer::new(filter);
    let file = FileSink::default();
    let shareable = FileSink::redacting();
    let book = LogBook::default();
    let installed = tracing_subscriber::registry()
        .with(layer)
        .with(book.layer())
        .with(fmt::layer().with_target(false))
        .with(
            fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(file.clone()),
        )
        .with(
            fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(shareable.clone()),
        )
        .try_init()
        .is_ok();
    LogControl {
        handle: installed.then_some(handle),
        from_env,
        file,
        guard: Arc::new(Mutex::new(None)),
        shareable,
        shareable_guard: Arc::new(Mutex::new(None)),
        book,
    }
}

impl LogControl {
    /// Uses the configured level unless `RUST_LOG` was given (the environment wins).
    pub fn apply_setting(&self, level: &str) {
        self.level_setter().apply(level);
    }

    /// This run's log in memory (the Logs panel's App and RustView logs).
    pub fn book(&self) -> LogBook {
        self.book.clone()
    }

    /// Changes the level and the log files later (the Settings window).
    pub fn level_setter(&self) -> LevelSetter {
        LevelSetter {
            handle: self.handle.clone(),
            from_env: self.from_env,
            file: self.file.clone(),
            guard: Arc::clone(&self.guard),
            shareable: self.shareable.clone(),
            shareable_guard: Arc::clone(&self.shareable_guard),
        }
    }

    /// Starts writing the log to daily files in `dir`, keeping the newest `keep_days` (at least one), and the shareable
    /// copy beside it when `shareable`.
    pub fn open_file(&self, dir: &Path, keep_days: u16, shareable: bool) {
        open_file(&self.file, &self.guard, dir, keep_days, FILE_PREFIX);
        open_shareable(&self.shareable, &self.shareable_guard, dir, keep_days, shareable);
    }
}

/// The shareable copy: written like the log (`on`), or not at all.
fn open_shareable(file: &FileSink, guard: &Mutex<Option<WorkerGuard>>, dir: &Path, keep_days: u16, on: bool) {
    if on {
        open_file(file, guard, dir, keep_days, SHAREABLE_PREFIX);
    } else {
        file.close();
        guard.lock().take();
    }
}

/// Starts writing the log to daily files in `dir` (again) named `prefix.DATE.log`, keeping the newest `keep_days`.
fn open_file(file: &FileSink, guard: &Mutex<Option<WorkerGuard>>, dir: &Path, keep_days: u16, prefix: &str) {
    {
        if let Err(e) = std::fs::create_dir_all(dir) {
            file.close();
            tracing::warn!(dir = %dir.display(), error = %e, "[App] No log folder; logging to stderr only");
            return;
        }
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(prefix)
            .filename_suffix(FILE_SUFFIX)
            .max_log_files(usize::from(keep_days.max(1)))
            .build(dir);
        match appender {
            Ok(appender) => {
                let (writer, new_guard) = tracing_appender::non_blocking(appender);
                file.open(writer);
                *guard.lock() = Some(new_guard);
                tracing::info!(dir = %dir.display(), keep_days, prefix, "[App] Logging to files");
            }
            Err(e) => {
                file.close();
                tracing::warn!(dir = %dir.display(), error = %e, "[App] No log file; logging to stderr only");
            }
        }
    }
}

/// Where a file layer writes: a buffer until the file is open, then the non-blocking file writer. A redacting sink
/// writes every line as the shareable copy shows it.
#[derive(Clone, Default)]
struct FileSink {
    state: Arc<Mutex<FileState>>,
    redacted: bool,
}

enum FileState {
    Buffering { lines: Vec<u8>, dropped: usize },
    Open(NonBlocking),
    Off,
}

impl Default for FileState {
    fn default() -> Self {
        Self::Buffering {
            lines: Vec::new(),
            dropped: 0,
        }
    }
}

impl FileSink {
    fn redacting() -> Self {
        Self {
            redacted: true,
            ..Self::default()
        }
    }

    /// `bytes` as this sink writes them.
    fn shown(&self, bytes: &[u8]) -> Vec<u8> {
        if self.redacted {
            super::redaction::redact(&String::from_utf8_lossy(bytes)).into_bytes()
        } else {
            bytes.to_vec()
        }
    }

    fn open(&self, mut writer: NonBlocking) {
        let mut state = self.state.lock();
        if let FileState::Buffering { lines, dropped } = &*state {
            // The non-blocking writer only queues; a failure means its thread is gone and nothing could be logged anyway.
            writer.write_all(&self.shown(lines)).ok();
            if *dropped > 0 {
                writeln!(
                    writer,
                    "[App] {dropped} early log bytes did not fit into the startup buffer"
                )
                .ok();
            }
        }
        *state = FileState::Open(writer);
    }

    fn close(&self) {
        *self.state.lock() = FileState::Off;
    }
}

impl std::io::Write for FileSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let redacted = self.redacted;
        match &mut *self.state.lock() {
            FileState::Buffering { lines, dropped } => {
                if lines.len() + buf.len() <= EARLY_BUFFER_LIMIT {
                    lines.extend_from_slice(buf);
                } else {
                    *dropped += buf.len();
                }
                Ok(buf.len())
            }
            FileState::Open(writer) if redacted => {
                // The shareable line (one event per write); the caller counts the original bytes.
                writer.write_all(super::redaction::redact(&String::from_utf8_lossy(buf)).as_bytes())?;
                Ok(buf.len())
            }
            FileState::Open(writer) => writer.write(buf),
            FileState::Off => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut *self.state.lock() {
            FileState::Open(writer) => writer.flush(),
            FileState::Buffering { .. } | FileState::Off => Ok(()),
        }
    }
}

impl<'a> MakeWriter<'a> for FileSink {
    type Writer = FileSink;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_lines_reach_the_file_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FileSink::default();
        sink.write_all(b"early line\n").unwrap();
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .build(dir.path())
            .unwrap();
        let (writer, guard) = tracing_appender::non_blocking(appender);
        sink.open(writer);
        sink.write_all(b"late line\n").unwrap();
        drop(guard);
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("rustcut.") && files[0].extension().is_some_and(|e| e == "log"),
            "{name}"
        );
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "early line\nlate line\n");
    }

    #[test]
    fn the_startup_buffer_is_bounded() {
        let mut sink = FileSink::default();
        let chunk = vec![b'x'; EARLY_BUFFER_LIMIT / 2 + 1];
        sink.write_all(&chunk).unwrap();
        sink.write_all(&chunk).unwrap();
        let FileState::Buffering { lines, dropped } = &*sink.state.lock() else {
            panic!("still buffering")
        };
        assert_eq!((lines.len(), *dropped), (chunk.len(), chunk.len()));
    }

    #[test]
    fn the_shareable_copy_hides_paths_from_the_first_line_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FileSink::redacting();
        sink.write_all(b"INFO [Open] Opened path=/home/someone/Videos/holiday.mp4\n")
            .unwrap();
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(SHAREABLE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .build(dir.path())
            .unwrap();
        let (writer, guard) = tracing_appender::non_blocking(appender);
        sink.open(writer);
        sink.write_all(b"INFO [Render] Done /home/someone/Videos/holiday.mp4\n")
            .unwrap();
        drop(guard);
        let file = std::fs::read_dir(dir.path()).unwrap().next().unwrap().unwrap().path();
        assert!(
            file.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("rustcut-shareable.")
        );
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("holiday") && !text.contains("/home/someone"), "{text}");
        let (first, second) = text.split_once('\n').unwrap();
        let tail = |l: &str| {
            l.rsplit_once(' ')
                .map(|(_, t)| t.trim_start_matches("path=").to_owned())
                .unwrap()
        };
        assert_eq!(
            tail(first),
            tail(second.trim_end()),
            "the same file, the same placeholder"
        );
    }

    #[test]
    fn a_closed_sink_swallows_lines() {
        let mut sink = FileSink::default();
        sink.close();
        assert_eq!(sink.write(b"gone\n").unwrap(), 5);
    }
}
