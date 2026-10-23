//! The runner against small shell scripts that behave like ffmpeg (progress on stdout, messages on stderr, `q` on stdin).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::*;
use crate::render::PassKind;
use crate::time::Timestamp;

/// A progress block as ffmpeg writes it.
const BLOCK: &str =
    r#"printf 'frame=%s\nout_time_us=%s\ntotal_size=%s\nspeed=1.5x\nprogress=%s\n' "$1" "$2" "$3" "$4""#;

fn script(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, format!("#!/bin/sh\nblock() {{ {BLOCK}; }}\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

fn pass() -> PlannedPass {
    PlannedPass {
        kind: PassKind::Encode,
        args: vec!["-i".into(), "file:/videos/a b.mp4".into()],
        duration: Timestamp::from_secs(3),
    }
}

fn options(program: &Path) -> PassOptions<'_> {
    PassOptions {
        program,
        priority: RenderPriority { nice: 5, idle_io: true },
        stall_timeout: None,
    }
}

type Seen = Arc<Mutex<Vec<PassEvent>>>;

fn recorder() -> (Seen, impl FnMut(PassEvent) + Send) {
    let seen: Seen = Arc::default();
    let sink = Arc::clone(&seen);
    (seen, move |e| sink.lock().unwrap().push(e))
}

#[tokio::test]
async fn progress_and_messages_of_a_good_run() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(
        &dir,
        "ffmpeg",
        "echo '[info] Stream mapping:' >&2\nblock 10 400000 1000 continue\nblock 75 3000000 9000 end\necho '[info] done' >&2",
    );
    let (_tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let (seen, sink) = recorder();
    run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, sink)
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    let samples: Vec<&ProgressSample> = seen
        .iter()
        .filter_map(|e| match e {
            PassEvent::Progress(s) => Some(s),
            PassEvent::Paused(_) => None,
        })
        .collect();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[1].out_time, Some(Timestamp::from_secs(3)));
    assert!(samples[1].end);
    let text = log.to_text();
    assert!(text.contains("encode: "), "{text}");
    assert!(text.contains("-i 'file:/videos/a b.mp4'"), "{text}");
    assert!(
        text.contains("[info] Stream mapping:\n") && text.contains("[info] done\n"),
        "{text}"
    );
}

#[tokio::test]
async fn a_failure_reports_ffmpegs_error() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(
        &dir,
        "ffmpeg",
        "echo '[libsvtav1 @ 0x1] [error] Svt[error]: bad' >&2\necho '[info] Conversion failed!' >&2\nexit 187",
    );
    let (_tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let error = run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, |_| {})
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PassError::Failed {
            status: "exit code 187".into(),
            message: "libsvtav1: Svt[error]: bad".into()
        }
    );
}

#[tokio::test]
async fn cancel_sends_q_and_ffmpeg_ends_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    // Like ffmpeg: progress, then wait for a key on stdin; `q` ends it with exit code 0.
    let ffmpeg = script(
        &dir,
        "ffmpeg",
        "block 1 40000 10 continue\nkey=$(head -c 1)\n[ \"$key\" = q ] && echo '[info] Exiting normally, received signal 2.' >&2 && exit 0\nexit 9",
    );
    let (tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let started = std::time::Instant::now();
    let sink = move |e| {
        if matches!(e, PassEvent::Progress(_)) {
            tx.send_replace(Control::Cancel);
        }
    };
    let error = run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, sink)
        .await
        .unwrap_err();
    assert_eq!(error, PassError::Canceled);
    assert!(started.elapsed() < QUIT_GRACE, "q was enough");
    assert!(log.to_text().contains("Exiting normally"));
}

#[tokio::test(start_paused = true)]
async fn a_process_that_ignores_q_and_sigterm_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(&dir, "ffmpeg", "trap '' TERM\nwhile :; do sleep 0.05; done");
    let (_tx, mut rx) = watch::channel(Control::Cancel);
    let mut log = JobLog::new();
    // Canceled before the start: nothing runs.
    assert_eq!(
        run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, |_| {}).await,
        Err(PassError::Canceled)
    );
    assert!(log.is_empty());

    let (tx, mut rx) = watch::channel(Control::Run);
    let canceler = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send_replace(Control::Cancel);
        tx
    });
    let error = run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, |_| {})
        .await
        .unwrap_err();
    assert_eq!(error, PassError::Canceled);
    let text = log.to_text();
    assert!(text.contains("Sent Terminate") && text.contains("Sent Kill"), "{text}");
    drop(canceler.await);
}

#[tokio::test]
async fn pause_stops_the_group_until_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ticks");
    let ffmpeg = script(
        &dir,
        "ffmpeg",
        &format!(
            "i=0\nwhile [ $i -lt 30 ]; do i=$((i+1)); echo $i >> '{}'; block $i $((i*40000)) $i continue; sleep 0.03; done\nblock 31 1240000 31 end",
            marker.display()
        ),
    );
    let (tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let (seen, sink) = recorder();
    let controller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        tx.send_replace(Control::Pause);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let lines = |p: &Path| std::fs::read_to_string(p).unwrap_or_default().lines().count();
        let before = lines(&marker);
        tokio::time::sleep(Duration::from_millis(400)).await;
        let after = lines(&marker);
        tx.send_replace(Control::Run);
        (before, after, tx)
    });
    run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, sink)
        .await
        .unwrap();
    let (before, after, _tx) = controller.await.unwrap();
    assert_eq!(before, after, "nothing ran while paused");
    let seen = seen.lock().unwrap();
    let pauses: Vec<bool> = seen
        .iter()
        .filter_map(|e| match e {
            PassEvent::Paused(p) => Some(*p),
            PassEvent::Progress(_) => None,
        })
        .collect();
    assert_eq!(pauses, [true, false]);
}

#[tokio::test(start_paused = true)]
async fn the_watchdog_stops_a_stalled_render() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(&dir, "ffmpeg", "block 1 40000 10 continue\nexec sleep 30");
    let (_tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let mut opts = options(&ffmpeg);
    opts.stall_timeout = Some(Duration::from_secs(60));
    let error = run_pass(&pass(), &opts, &mut rx, &mut log, |_| {}).await.unwrap_err();
    assert_eq!(error, PassError::Stalled { secs: 60 });
    assert!(log.to_text().contains("No progress for the stall timeout"));
}

#[tokio::test]
async fn a_dropped_owner_cancels_and_a_missing_program_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(&dir, "ffmpeg", "key=$(head -c 1)\nexit 0");
    let (tx, mut rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let dropper = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(tx);
    });
    assert_eq!(
        run_pass(&pass(), &options(&ffmpeg), &mut rx, &mut log, |_| {}).await,
        Err(PassError::Canceled)
    );
    dropper.await.unwrap();

    let (_tx, mut rx) = watch::channel(Control::Run);
    let missing = dir.path().join("no-ffmpeg");
    let error = run_pass(&pass(), &options(&missing), &mut rx, &mut log, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(error, PassError::NotFound { .. }), "{error:?}");
    assert!(error.to_string().contains("sudo pacman -S ffmpeg"));
}

#[test]
fn command_lines_are_shell_quoted() {
    let args: Vec<OsString> = vec!["-i".into(), "file:/a/it's here.mkv".into(), "[0:v:0]null[vout]".into()];
    assert_eq!(
        command_line(Path::new("/usr/bin/ffmpeg"), &args),
        r#"/usr/bin/ffmpeg -i "file:/a/it's here.mkv" '[0:v:0]null[vout]'"#
    );
}
