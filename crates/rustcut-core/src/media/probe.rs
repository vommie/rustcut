//! Running ffprobe (ADR-3): a supervised child process on the tokio runtime with a time limit and a cap on its output.
//! The child is killed when the probe is dropped (a newer file was opened, the app quits) or takes too long, so a hanging
//! network share or a hostile file never leaves a process behind or blocks anything.
//!
//! Two calls: [`probe`] reads the container and the streams (a few milliseconds, only the header is parsed), then
//! [`probe_frame_timing`] samples the primary video's packet timestamps for the variable-frame-rate check. The sample is
//! separate because demuxing can be slow: in a preallocated, partly downloaded file ffprobe scans gigabytes of zeros for
//! the next packet. Then only the frame-rate check waits or gives up, never the media description.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::Command;

use super::ffprobe::{DecodeError, decode, decode_timing};
use super::model::{FrameTiming, MediaInfo};

/// How long the probe may take before ffprobe is killed (PyCutRv allowed 8 s; files on slow shares need more).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the frame-timing sample may take; after that the frame rate is shown without the check.
pub const TIMING_TIMEOUT: Duration = Duration::from_secs(10);
/// Seconds of packet timestamps read for the frame-spacing check (demuxing only, a few milliseconds normally).
pub const TIMING_SAMPLE_SECS: u32 = 10;
/// More output than this is not a media description (ffprobe writes a few kilobytes per file).
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
/// stderr kept (ffmpeg's detection filters write a line per frame there; ffprobe only its errors).
const MAX_STDERR: u64 = 1024 * 1024;

/// What ffprobe to run and how long to wait for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    /// `None` = `ffprobe` from `PATH` (settings: `advanced.ffprobe`).
    pub program: Option<PathBuf>,
    /// Time limit of [`probe`].
    pub timeout: Duration,
    /// Time limit of [`probe_frame_timing`].
    pub timing_timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            program: None,
            timeout: PROBE_TIMEOUT,
            timing_timeout: TIMING_TIMEOUT,
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
    #[error("{program} did not finish within {secs} s")]
    TimedOut { program: String, secs: u64 },
    /// ffprobe could not read the file (broken, not a media file, no access); its own words.
    #[error("{0}")]
    Unreadable(String),
    #[error("{program} failed ({status}): {message}")]
    Failed {
        program: String,
        status: String,
        message: String,
    },
    #[error("the tool's output is not readable: {0}")]
    Output(String),
}

const COMMON_ARGS: [&str; 5] = ["-v", "error", "-hide_banner", "-print_format", "json"];

/// The arguments of [`probe`]: format, streams and errors as JSON. `-i` keeps a path that starts with `-` a path.
pub fn probe_args(file: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = COMMON_ARGS.iter().map(OsString::from).collect();
    args.extend(["-show_error", "-show_format", "-show_streams", "-i"].map(OsString::from));
    args.push(file.as_os_str().to_owned());
    args
}

/// The arguments of [`probe_frame_timing`]: the timestamps of stream `stream_index` in the first
/// [`TIMING_SAMPLE_SECS`] seconds.
pub fn timing_args(file: &Path, stream_index: u32) -> Vec<OsString> {
    let mut args: Vec<OsString> = COMMON_ARGS.iter().map(OsString::from).collect();
    args.push("-select_streams".into());
    args.push(stream_index.to_string().into());
    args.extend(["-show_entries", "packet=pts_time", "-read_intervals"].map(OsString::from));
    args.push(format!("%+{TIMING_SAMPLE_SECS}").into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args
}

/// Probes `file`'s container and streams (without [`MediaInfo::frame_timing`]). Must run on the tokio runtime;
/// dropping the future kills ffprobe.
pub async fn probe(file: &Path, options: &ProbeOptions) -> Result<MediaInfo, ProbeError> {
    let output = run(options.program(), probe_args(file), options.timeout, MAX_OUTPUT).await?;
    match (decode(&output.stdout), output.status.success()) {
        (Ok(info), true) => Ok(info),
        (Err(DecodeError::Reported(reason)), _) => Err(ProbeError::Unreadable(reason)),
        (Err(DecodeError::Malformed(message)), true) => Err(ProbeError::Output(message)),
        (_, false) => Err(output.failure()),
    }
}

/// Samples the packet timestamps of stream `stream_index` (the primary video) for [`MediaInfo::frame_timing`]. `None`
/// when there are too few frames to tell.
pub async fn probe_frame_timing(
    file: &Path,
    stream_index: u32,
    options: &ProbeOptions,
) -> Result<Option<FrameTiming>, ProbeError> {
    let output = run(
        options.program(),
        timing_args(file, stream_index),
        options.timing_timeout,
        MAX_OUTPUT,
    )
    .await?;
    if !output.status.success() {
        return Err(output.failure());
    }
    Ok(decode_timing(&output.stdout))
}

/// [`probe`], then [`probe_frame_timing`] of the primary video; a failed sample only leaves the frame timing empty.
pub async fn probe_with_timing(file: &Path, options: &ProbeOptions) -> Result<MediaInfo, ProbeError> {
    let mut info = probe(file, options).await?;
    if let Some(index) = info.primary_video().map(|(stream, _)| stream.index) {
        match probe_frame_timing(file, index, options).await {
            Ok(timing) => info.frame_timing = timing,
            Err(e) => tracing::warn!(path = %file.display(), error = %e, "[Media] Frame timing not sampled"),
        }
    }
    Ok(info)
}

/// What a finished tool wrote and how it ended.
pub(crate) struct Output {
    pub(crate) program: String,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: String,
    pub(crate) status: ExitStatus,
}

impl Output {
    pub(crate) fn failure(&self) -> ProbeError {
        ProbeError::Failed {
            program: self.program.clone(),
            status: self.status.to_string(),
            message: last_line(&self.stderr).unwrap_or_else(|| "no message".into()),
        }
    }
}

/// Runs `program` (ffprobe or ffmpeg) with `args` and collects its output within `timeout`; more than `max_stdout` bytes
/// on stdout is an error, stderr is cut at [`MAX_STDERR`]. Dropping the future kills the program.
pub(crate) async fn run(
    program: &Path,
    args: Vec<OsString>,
    timeout: Duration,
    max_stdout: u64,
) -> Result<Output, ProbeError> {
    let program_name = || program.display().to_string();
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = crate::process::spawn(&mut command).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ProbeError::NotFound {
            program: program_name(),
        },
        _ => ProbeError::Spawn {
            program: program_name(),
            error: e.to_string(),
        },
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let collect = async {
        tokio::join!(
            read_capped(stdout, max_stdout),
            read_capped(stderr, MAX_STDERR),
            child.wait()
        )
    };
    let (out, err, status) = tokio::time::timeout(timeout, collect)
        .await
        .map_err(|_elapsed| ProbeError::TimedOut {
            program: program_name(),
            secs: timeout.as_secs(),
        })?;
    let status = status.map_err(|e| ProbeError::Failed {
        program: program_name(),
        status: "no exit status".into(),
        message: e.to_string(),
    })?;
    Ok(Output {
        program: program_name(),
        stdout: out.map_err(ProbeError::Output)?,
        stderr: String::from_utf8_lossy(&err.unwrap_or_default()).into_owned(),
        status,
    })
}

/// Reads a pipe to its end, failing when it carries more than `limit` bytes.
async fn read_capped(pipe: Option<impl AsyncRead + Unpin>, limit: u64) -> Result<Vec<u8>, String> {
    let Some(pipe) = pipe else { return Ok(Vec::new()) };
    let mut buffer = Vec::new();
    let mut limited = pipe.take(limit.saturating_add(1));
    limited
        .read_to_end(&mut buffer)
        .await
        .map_err(|e| format!("reading the tool's output failed: {e}"))?;
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
        assert!(args.iter().any(|a| a == "-show_streams"));
        let timing = timing_args(Path::new("/v/a.mkv"), 2);
        let text: Vec<String> = timing.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            text.join(" "),
            "-v error -hide_banner -print_format json -select_streams 2 -show_entries packet=pts_time \
             -read_intervals %+10 -i /v/a.mkv"
        );
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
            ..ProbeOptions::default()
        };
        let started = std::time::Instant::now();
        let error = probe(Path::new("/tmp/x.mp4"), &options).await.unwrap_err();
        assert!(matches!(error, ProbeError::TimedOut { secs: 0, .. }), "{error:?}");
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
