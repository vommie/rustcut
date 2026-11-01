//! "Open in mpv" (P12 5.1, user decision Q-P12d): the user's own mpv with their configuration, paused on the given
//! time. It is started on a thread of its own, leads its own process group and keeps running when RustCut quits.

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::time::Timestamp;

/// What mpv shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvStart {
    /// The program; `None` = `mpv` from `PATH`.
    pub program: Option<PathBuf>,
    pub file: PathBuf,
    pub start: Timestamp,
    /// The end of an A-B loop from `start`.
    pub loop_end: Option<Timestamp>,
}

/// mpv's seconds with millisecond precision.
fn seconds(t: Timestamp) -> String {
    format!("{:.3}", t.as_secs_f64())
}

impl MpvStart {
    /// mpv's arguments. `--no-resume-playback`: a position saved by `save-position-on-quit` would replace `--start`.
    /// `--hr-seek=yes`: the start is the exact frame, not the keyframe before it. `--` ends the options, so a file
    /// name that starts with a dash stays a file name.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "--pause".into(),
            "--no-resume-playback".into(),
            "--hr-seek=yes".into(),
            format!("--start={}", seconds(self.start)).into(),
        ];
        if let Some(end) = self.loop_end {
            args.push(format!("--ab-loop-a={}", seconds(self.start)).into());
            args.push(format!("--ab-loop-b={}", seconds(end)).into());
        }
        args.push("--".into());
        args.push(self.file.clone().into_os_string());
        args
    }

    fn program(&self) -> &Path {
        self.program.as_deref().unwrap_or_else(|| Path::new("mpv"))
    }

    /// Starts mpv on a thread of its own, which waits for it so no zombie stays behind (not the tokio runtime: its
    /// blocking threads would hold up RustCut's quit while mpv runs). `failed` runs on that thread when it cannot start.
    pub fn launch(self, failed: impl FnOnce(io::Error) + Send + 'static) {
        let spawned = std::thread::Builder::new().name("rustcut-mpv".into()).spawn(move || {
            let mut command = Command::new(self.program());
            command
                .args(self.args())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            tracing::info!(program = %self.program().display(), file = %self.file.display(), "[Player] Opening in mpv");
            match command.spawn() {
                Ok(mut child) => {
                    let status = child.wait();
                    tracing::debug!(?status, "[Player] mpv ended");
                }
                Err(e) => failed(e),
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[Player] No thread for mpv");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(loop_end: Option<Timestamp>) -> MpvStart {
        MpvStart {
            program: None,
            file: "/v/-odd name.mkv".into(),
            start: Timestamp::from_millis(83_456),
            loop_end,
        }
    }

    #[test]
    fn it_starts_paused_on_the_exact_time() {
        assert_eq!(
            start(None).args(),
            [
                "--pause",
                "--no-resume-playback",
                "--hr-seek=yes",
                "--start=83.456",
                "--",
                "/v/-odd name.mkv"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn a_section_becomes_an_ab_loop() {
        let args = start(Some(Timestamp::from_millis(90_000))).args();
        assert!(args.contains(&"--ab-loop-a=83.456".into()));
        assert!(args.contains(&"--ab-loop-b=90.000".into()));
    }

    #[test]
    fn the_program_is_mpv_unless_one_is_set() {
        assert_eq!(start(None).program(), Path::new("mpv"));
        let own = MpvStart {
            program: Some("/opt/mpv".into()),
            ..start(None)
        };
        assert_eq!(own.program(), Path::new("/opt/mpv"));
    }

    #[test]
    fn a_missing_program_reports_failure() {
        let (tx, rx) = std::sync::mpsc::channel();
        let missing = MpvStart {
            program: Some("/nonexistent/mpv".into()),
            ..start(None)
        };
        missing.launch(move |e| {
            let _ignored = tx.send(e.kind());
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            io::ErrorKind::NotFound
        );
    }
}
