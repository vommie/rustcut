//! ffmpeg runs that outlived the RustCut that started them. `kill_on_drop` and the cancel ladder only work while RustCut
//! runs: after a crash, a SIGKILL or the OOM killer, ffmpeg went on encoding into the hidden temp file (or stayed stopped
//! forever when its job was paused), and the restarted job wrote the same file a second time. At start the queue stops
//! the ffmpeg runs that write the work files of the jobs it recovers (and the probe render stops the ones of its folder),
//! before those files are deleted (`/proc` read with `procfs`).
//!
//! A process counts as such a run when it belongs to this user, leads its own process group (RustCut starts ffmpeg that
//! way), names one of the work files in its arguments, and its parent is not a running instance of this program (another
//! RustCut, or this one, still supervises it).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use procfs::process::{Process, all_processes};
use rustix::process::{Pid, Signal, getuid, kill_process_group};

/// How long a stopped run may take to disappear.
const GONE_WITHIN: Duration = Duration::from_secs(2);

/// Stops every orphaned ffmpeg run that writes one of `paths` (files or folders) and waits until they are gone.
/// Returns their process ids. Blocking: `/proc` is read synchronously (call it on a blocking thread).
pub fn stop_orphans(paths: &[PathBuf]) -> Vec<i32> {
    let needles: Vec<String> = paths
        .iter()
        .filter_map(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    if needles.is_empty() {
        return Vec::new();
    }
    let Ok(processes) = all_processes() else {
        tracing::warn!("[Render] /proc could not be read; orphaned ffmpeg runs are not looked for");
        return Vec::new();
    };
    let own = Process::myself().ok().and_then(|p| p.stat().ok()).map(|s| s.comm);
    let uid = getuid().as_raw();
    let mut stopped = Vec::new();
    for process in processes.flatten() {
        let (Ok(stat), Ok(owner), Ok(args)) = (process.stat(), process.uid(), process.cmdline()) else {
            continue;
        };
        if owner != uid || stat.pgrp != stat.pid || !names_one_of(&args, &needles) {
            continue;
        }
        let supervised = Process::new(stat.ppid)
            .and_then(|parent| parent.stat())
            .is_ok_and(|parent| own.as_deref() == Some(parent.comm.as_str()));
        if supervised {
            continue;
        }
        let Some(group) = Pid::from_raw(stat.pid) else { continue };
        // A stopped (paused) run first gets SIGCONT, so nothing of it stays stopped; SIGKILL ends it at once.
        let _ = kill_process_group(group, Signal::CONT);
        match kill_process_group(group, Signal::KILL) {
            Ok(()) => {
                tracing::warn!(pid = stat.pid, program = %stat.comm, "[Render] Stopped an ffmpeg run left by an earlier RustCut");
                stopped.push(stat.pid);
            }
            Err(e) => {
                tracing::warn!(pid = stat.pid, error = %e, "[Render] An orphaned ffmpeg run could not be stopped");
            }
        }
    }
    wait_until_gone(&stopped);
    stopped
}

/// Whether one of the arguments names one of the paths: inside a `file:` URL, a filter option or as a folder prefix, but
/// never as the start of a longer name (`job-7` is not `job-71`).
fn names_one_of(args: &[String], needles: &[String]) -> bool {
    args.iter().any(|arg| needles.iter().any(|needle| names(arg, needle)))
}

fn names(arg: &str, needle: &str) -> bool {
    arg.match_indices(needle).any(|(at, found)| {
        let next = arg.get(at + found.len()..).and_then(|rest| rest.chars().next());
        !next.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
    })
}

fn wait_until_gone(pids: &[i32]) {
    let start = Instant::now();
    while start.elapsed() < GONE_WITHIN {
        let alive = pids.iter().any(|&pid| {
            Process::new(pid)
                .and_then(|p| p.stat())
                .is_ok_and(|s| s.state != 'Z' && s.state != 'X')
        });
        if !alive {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    tracing::warn!(?pids, "[Render] Orphaned ffmpeg runs were still there after the kill");
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    fn alive(pid: i32) -> bool {
        Process::new(pid)
            .and_then(|p| p.stat())
            .is_ok_and(|s| s.state != 'Z' && s.state != 'X')
    }

    #[test]
    fn arguments_match_files_and_folders() {
        let needles = vec![
            "/out/.Clip - 01.rustcut-part.mkv".to_owned(),
            "/data/passlog/job-7".to_owned(),
        ];
        let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(names_one_of(
            &args(&["ffmpeg", "-f", "matroska", "file:/out/.Clip - 01.rustcut-part.mkv"]),
            &needles
        ));
        assert!(names_one_of(
            &args(&["ffmpeg", "-passlogfile", "/data/passlog/job-7/pass"]),
            &needles
        ));
        assert!(names_one_of(
            &args(&["ffmpeg", "-vf", "vidstabdetect=result=/data/passlog/job-7:shakiness=5"]),
            &needles
        ));
        assert!(!names_one_of(&args(&["ffmpeg", "file:/out/Clip - 01.mkv"]), &needles));
        assert!(!names_one_of(
            &args(&["ffmpeg", "-passlogfile", "/data/passlog/job-71/pass"]),
            &needles
        ));
        assert!(!names_one_of(
            &args(&["ffmpeg", "file:/out/.Clip - 01.rustcut-part.mkv2"]),
            &needles
        ));
    }

    #[test]
    fn an_orphaned_run_is_stopped_and_a_supervised_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".clip.rustcut-part.mkv");
        let marked = format!("sleep 30; : {}", marker.display());
        // An orphan: `setsid` makes the shell lead its own group, the outer shell exits at once and leaves it behind.
        let out = Command::new("sh")
            .args(["-c", &format!("setsid sh -c '{marked}' >/dev/null 2>&1 & echo $!")])
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        let orphan: i32 = String::from_utf8(out.stdout).unwrap().trim().parse().unwrap();
        // A run this program still supervises: its own child, also leading its own group.
        let mut child = std::os::unix::process::CommandExt::process_group(Command::new("sh").args(["-c", &marked]), 0)
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !(alive(orphan) && Process::new(orphan).unwrap().stat().unwrap().pgrp == orphan) {
            assert!(start.elapsed() < Duration::from_secs(5), "the orphan did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let stopped = stop_orphans(&[marker, PathBuf::from("/nonexistent/other")]);
        assert_eq!(stopped, vec![orphan]);
        assert!(!alive(orphan));
        assert!(child.try_wait().unwrap().is_none(), "a supervised run is left alone");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(stop_orphans(&[]).is_empty());
    }
}
