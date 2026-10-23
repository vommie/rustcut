//! Running ffprobe (ADR-3): a supervised child process on the tokio runtime with a time limit and a cap on its output.
//! The child is killed when the probe is dropped (a newer file was opened, the app quits) or takes too long, so a hanging
//! network share or a hostile file never leaves a process behind or blocks anything.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::Command;

use super::ffprobe::{DecodeError, decode};
use super::model::MediaInfo;

/// How long ffprobe may take before it is killed (PyCutRv allowed 8 s; files on slow shares need more).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Seconds of packet timestamps read for the frame-spacing check (demuxing only, a few milliseconds).
pub const TIMING_SAMPLE_SECS: u32 = 10;
/// More output than this is not a media description (ffprobe writes a few kilobytes per file).
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
/// stderr kept for the error message.
const MAX_STDERR: u64 = 64 * 1024;

/// What ffprobe to run and how long to wait for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    /// `None` = `ffprobe` from `PATH` (settings: `advanced.ffprobe`).
    pub program: Option<PathBuf>,
    pub timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            program: None,
            timeout: PROBE_TIMEOUT,
        }
    }
}

impl ProbeOptions {
    fn program(&self) -> &Path {
        self.program.as_deref().unwrap_or_else(|| Path::new("ffprobe"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("{program} was not found; install ffmpeg (sudo pacman -S ffmpeg) or set its path in the settings")]
    NotFound { program: String },
    #[error("{program} could not be started: {error}")]
    Spawn { program: String, error: String },
    #[error("ffprobe did not finish within {secs} s")]
    TimedOut { secs: u64 },
    /// ffprobe could not read the file (broken, not a media file, no access); its own words.
    #[error("{0}")]
    Unreadable(String),
    #[error("ffprobe failed ({status}): {message}")]
    Failed { status: String, message: String },
    #[error("ffprobe's output is not readable: {0}")]
    Output(String),
}

/// The arguments for `file`: format, streams and errors as JSON, plus the packet timestamps of the first
/// [`TIMING_SAMPLE_SECS`] seconds (stream index and time only). `-i` keeps a path that starts with `-` a path.
pub fn probe_args(file: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-v",
        "error",
        "-hide_banner",
        "-print_format",
        "json",
        "-show_error",
        "-show_format",
        "-show_streams",
        "-show_entries",
        "packet=stream_index,pts_time",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push("-read_intervals".into());
    args.push(format!("%+{TIMING_SAMPLE_SECS}").into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args
}

/// Probes `file` with ffprobe. Must run on the tokio runtime; dropping the future kills ffprobe.
pub async fn probe(file: &Path, options: &ProbeOptions) -> Result<MediaInfo, ProbeError> {
    let program = options.program();
    let program_name = program.display().to_string();
    let mut child = Command::new(program)
        .args(probe_args(file))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ProbeError::NotFound {
                program: program_name.clone(),
            },
            _ => ProbeError::Spawn {
                program: program_name.clone(),
                error: e.to_string(),
            },
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let run = async {
        tokio::join!(
            read_capped(stdout, MAX_OUTPUT),
            read_capped(stderr, MAX_STDERR),
            child.wait()
        )
    };
    let (out, err, status) =
        tokio::time::timeout(options.timeout, run)
            .await
            .map_err(|_elapsed| ProbeError::TimedOut {
                secs: options.timeout.as_secs(),
            })?;
    let status = status.map_err(|e| ProbeError::Failed {
        status: "no exit status".into(),
        message: e.to_string(),
    })?;
    let out = out.map_err(ProbeError::Output)?;
    let stderr_text = String::from_utf8_lossy(&err.unwrap_or_default()).into_owned();
    match (decode(&out), status.success()) {
        (Ok(info), true) => Ok(info),
        (Err(DecodeError::Reported(reason)), _) => Err(ProbeError::Unreadable(reason)),
        (Err(DecodeError::Malformed(message)), true) => Err(ProbeError::Output(message)),
        (_, false) => Err(ProbeError::Failed {
            status: status.to_string(),
            message: last_line(&stderr_text).unwrap_or_else(|| "no message".into()),
        }),
    }
}

/// Reads a pipe to its end, failing when it carries more than `limit` bytes.
async fn read_capped(pipe: Option<impl AsyncRead + Unpin>, limit: u64) -> Result<Vec<u8>, String> {
    let Some(pipe) = pipe else { return Ok(Vec::new()) };
    let mut buffer = Vec::new();
    let mut limited = pipe.take(limit.saturating_add(1));
    limited
        .read_to_end(&mut buffer)
        .await
        .map_err(|e| format!("reading ffprobe's output failed: {e}"))?;
    if u64::try_from(buffer.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("more than {limit} bytes"));
    }
    Ok(buffer)
}

/// The last non-empty line of ffprobe's stderr without the `<path>: ` prefix ffmpeg puts in front of file errors.
fn last_line(stderr: &str) -> Option<String> {
    let line = stderr.lines().rev().map(str::trim).find(|l| !l.is_empty())?;
    let message = line.rsplit_once(": ").map_or(line, |(_, message)| message);
    Some(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_keep_the_path_a_path() {
        let args = probe_args(Path::new("-odd name.mp4"));
        assert_eq!(args.last().map(OsString::as_os_str), Some("-odd name.mp4".as_ref()));
        assert_eq!(args.get(args.len() - 2).map(OsString::as_os_str), Some("-i".as_ref()));
        assert!(args.iter().any(|a| a == "%+10"));
    }

    #[test]
    fn the_last_stderr_line_without_the_path() {
        let stderr = "[mov,mp4 @ 0x5] moov atom not found\n/v/a: b.mp4: Invalid data found when processing input\n\n";
        assert_eq!(
            last_line(stderr).as_deref(),
            Some("Invalid data found when processing input")
        );
        assert_eq!(last_line("plain message").as_deref(), Some("plain message"));
        assert_eq!(last_line("\n \n"), None);
    }

    #[tokio::test]
    async fn a_missing_program_says_how_to_install_it() {
        let options = ProbeOptions {
            program: Some(PathBuf::from("/nonexistent/ffprobe")),
            ..ProbeOptions::default()
        };
        let error = probe(Path::new("/tmp/x.mp4"), &options).await.unwrap_err();
        assert!(matches!(error, ProbeError::NotFound { .. }), "{error:?}");
        assert!(error.to_string().contains("sudo pacman -S ffmpeg"));
    }

    #[tokio::test]
    async fn a_hanging_program_is_killed_at_the_timeout() {
        // A stand-in that never answers, like ffprobe on a stalled network share.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("slow-ffprobe");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        let options = ProbeOptions {
            program: Some(script),
            timeout: Duration::from_millis(200),
        };
        let started = std::time::Instant::now();
        let error = probe(Path::new("/tmp/x.mp4"), &options).await.unwrap_err();
        assert_eq!(error, ProbeError::TimedOut { secs: 0 });
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_failing_program_reports_its_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("failing-ffprobe");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'first' >&2\necho \"$0: something broke\" >&2\nexit 3\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        let options = ProbeOptions {
            program: Some(script),
            ..ProbeOptions::default()
        };
        match probe(Path::new("/tmp/x.mp4"), &options).await {
            Err(ProbeError::Failed { message, .. }) => assert_eq!(message, "something broke"),
            other => panic!("{other:?}"),
        }
    }
}
