//! Logging setup: `tracing` to stderr and to daily rotating files in the profile's state folder (`state/logs/`, the newest
//! `advanced.log_keep_days` files are kept). The level comes from `RUST_LOG` when set, otherwise from the setting
//! `advanced.log_level` once the settings are read (`info` until then).
//!
//! Logging starts before the settings are read, so the file layer buffers what is logged until [`LogControl::open_file`]
//! knows the folder and the retention; then the buffer goes first into the file. The file is written by
//! `tracing-appender`'s background thread, never by the thread that logs.

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

/// Lines logged before the file is open are kept up to this size (startup logs a few kilobytes).
const EARLY_BUFFER_LIMIT: usize = 1 << 20;

/// File names: `rustcut.2026-10-23.log`.
const FILE_PREFIX: &str = "rustcut";
const FILE_SUFFIX: &str = "log";

/// Changes the log filter after startup and owns the log file's writer thread (dropping it flushes the file).
pub struct LogControl {
    handle: Option<reload::Handle<EnvFilter, Registry>>,
    from_env: bool,
    file: FileSink,
    guard: Mutex<Option<WorkerGuard>>,
}

/// Changes the log filter (a clone of the reload handle).
#[derive(Clone)]
pub struct LevelSetter {
    handle: Option<reload::Handle<EnvFilter, Registry>>,
    from_env: bool,
}

impl LevelSetter {
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
    let installed = tracing_subscriber::registry()
        .with(layer)
        .with(fmt::layer().with_target(false))
        .with(
            fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(file.clone()),
        )
        .try_init()
        .is_ok();
    LogControl {
        handle: installed.then_some(handle),
        from_env,
        file,
        guard: Mutex::new(None),
    }
}

impl LogControl {
    /// Uses the configured level unless `RUST_LOG` was given (the environment wins).
    pub fn apply_setting(&self, level: &str) {
        self.level_setter().apply(level);
    }

    /// Changes the level later (the Settings window), with the same rule.
    pub fn level_setter(&self) -> LevelSetter {
        LevelSetter {
            handle: self.handle.clone(),
            from_env: self.from_env,
        }
    }

    /// Starts writing the log to daily files in `dir`, keeping the newest `keep_days` (at least one).
    pub fn open_file(&self, dir: &Path, keep_days: u16) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.file.close();
            tracing::warn!(dir = %dir.display(), error = %e, "[App] No log folder; logging to stderr only");
            return;
        }
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .max_log_files(usize::from(keep_days.max(1)))
            .build(dir);
        match appender {
            Ok(appender) => {
                let (writer, guard) = tracing_appender::non_blocking(appender);
                self.file.open(writer);
                *self.guard.lock() = Some(guard);
                tracing::info!(dir = %dir.display(), keep_days, "[App] Logging to files");
            }
            Err(e) => {
                self.file.close();
                tracing::warn!(dir = %dir.display(), error = %e, "[App] No log file; logging to stderr only");
            }
        }
    }
}

/// Where the file layer writes: a buffer until the file is open, then the non-blocking file writer.
#[derive(Clone, Default)]
struct FileSink(Arc<Mutex<FileState>>);

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
    fn open(&self, mut writer: NonBlocking) {
        let mut state = self.0.lock();
        if let FileState::Buffering { lines, dropped } = &*state {
            // The non-blocking writer only queues; a failure means its thread is gone and nothing could be logged anyway.
            writer.write_all(lines).ok();
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
        *self.0.lock() = FileState::Off;
    }
}

impl std::io::Write for FileSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut *self.0.lock() {
            FileState::Buffering { lines, dropped } => {
                if lines.len() + buf.len() <= EARLY_BUFFER_LIMIT {
                    lines.extend_from_slice(buf);
                } else {
                    *dropped += buf.len();
                }
                Ok(buf.len())
            }
            FileState::Open(writer) => writer.write(buf),
            FileState::Off => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut *self.0.lock() {
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
        let FileState::Buffering { lines, dropped } = &*sink.0.lock() else {
            panic!("still buffering")
        };
        assert_eq!((lines.len(), *dropped), (chunk.len(), chunk.len()));
    }

    #[test]
    fn a_closed_sink_swallows_lines() {
        let mut sink = FileSink::default();
        sink.close();
        assert_eq!(sink.write(b"gone\n").unwrap(), 5);
    }
}
