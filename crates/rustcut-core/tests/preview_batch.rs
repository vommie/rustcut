//! The preview batch of the conflict dialog (M5.4) with stand-in programs: the existing target is read before the
//! similar files, never more than `DEFAULT_PARALLEL` programs run at once, and stopping the batch (the dialog closed) kills
//! the running ffprobe and ffmpeg at once, leaves nothing behind and does not spoil the cache for the next request.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustcut_core::media::{
    DEFAULT_PARALLEL, PreviewOptions, PreviewPrograms, PreviewRequest, PreviewService, PreviewSlot, THUMB_HEIGHT,
    THUMB_WIDTH,
};

/// The bytes of one picture.
const PICTURE: u32 = THUMB_WIDTH * THUMB_HEIGHT * 4;
const PROBE_JSON: &str = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":640,"height":360,"r_frame_rate":"25/1"}],"format":{"format_name":"mov","duration":"10.0"}}"#;

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A stand-in ffprobe that logs and answers after `sleep` seconds (`exec` keeps the pid, so a kill reaches the sleep).
fn ffprobe(dir: &Path, log: &Path, sleep: &str) -> PathBuf {
    let log = log.display();
    script(
        dir,
        "ffprobe",
        &format!(
            "for a; do f=$a; done\necho \"probe-start $$ $(basename \"$f\")\" >> '{log}'\n\
             if [ '{sleep}' = forever ]; then exec sleep 30; fi\nsleep {sleep}\n\
             echo \"probe-end $$ $(basename \"$f\")\" >> '{log}'\nprintf '%s' '{PROBE_JSON}'"
        ),
    )
}

/// A stand-in ffmpeg that logs, waits and writes one transparent picture.
fn ffmpeg(dir: &Path, log: &Path, sleep: &str) -> PathBuf {
    let log = log.display();
    script(
        dir,
        "ffmpeg",
        &format!(
            "for a; do case \"$a\" in *.mp4) f=$a;; esac; done\necho \"picture-start $$ $(basename \"$f\")\" >> '{log}'\n\
             if [ '{sleep}' = forever ]; then exec sleep 30; fi\nsleep {sleep}\n\
             echo \"picture-end $$ $(basename \"$f\")\" >> '{log}'\nhead -c {PICTURE} /dev/zero"
        ),
    )
}

fn media(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| {
            let path = dir.join(n);
            std::fs::write(&path, n.as_bytes()).unwrap();
            path
        })
        .collect()
}

fn pic(path: &Path) -> PreviewRequest {
    PreviewRequest::picture(path)
}

fn pics(paths: &[PathBuf]) -> Vec<PreviewRequest> {
    paths.iter().map(PreviewRequest::picture).collect()
}

fn lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn with(programs: PreviewPrograms) -> PreviewOptions {
    PreviewOptions {
        programs,
        ..PreviewOptions::default()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

/// The process is gone (or only waits to be reaped).
fn ended(pid: &str) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit_once(") ").is_some_and(|(_, rest)| rest.starts_with('Z')),
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_existing_target_comes_first_and_at_most_three_run_at_once() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let programs = PreviewPrograms {
        ffprobe: Some(ffprobe(dir.path(), &log, "0.05")),
        ffmpeg: Some(ffmpeg(dir.path(), &log, "0.3")),
    };
    let files = media(
        dir.path(),
        &["main.mp4", "s1.mp4", "s2.mp4", "s3.mp4", "s4.mp4", "s5.mp4"],
    );
    let service = PreviewService::new(with(programs));
    let (batch, mut events) = service.batch(rt.handle(), Some(pic(&files[0])), pics(&files[1..]));
    let mut got = Vec::new();
    rt.block_on(async {
        while let Some(event) = events.recv().await {
            assert!(event.result.unwrap().thumbnail.is_some());
            got.push(event.slot);
        }
    });
    // The task ends right after it dropped its sender.
    wait_for("the batch to end", || batch.is_finished());
    assert_eq!(got.len(), 6);
    assert_eq!(got[0], PreviewSlot::First, "the existing target's result comes first");
    let log = lines(&log);
    let main_end = log
        .iter()
        .position(|l| l.starts_with("picture-end") && l.ends_with("main.mp4"))
        .unwrap();
    let first_other = log.iter().position(|l| !l.ends_with("main.mp4")).unwrap();
    assert!(
        main_end < first_other,
        "nothing else starts before the main picture is done:\n{log:#?}"
    );
    let (mut running, mut most) = (0_usize, 0_usize);
    for line in &log {
        if line.starts_with("probe-start") {
            running += 1;
        } else if line.starts_with("picture-end") {
            running -= 1;
        }
        most = most.max(running);
    }
    assert_eq!(most, DEFAULT_PARALLEL, "{log:#?}");
}

#[test]
fn closing_the_dialog_kills_the_running_programs_and_keeps_the_cache_clean() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let tools = dir.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let files = media(dir.path(), &["main.mp4", "s1.mp4", "s2.mp4", "s3.mp4"]);
    // The picture of the main file hangs.
    let service = PreviewService::new(with(PreviewPrograms {
        ffprobe: Some(ffprobe(&tools, &log, "0")),
        ffmpeg: Some(ffmpeg(&tools, &log, "forever")),
    }));
    let (batch, events) = service.batch(rt.handle(), Some(pic(&files[0])), pics(&files[1..]));
    wait_for("the main picture to start", || {
        lines(&log).iter().any(|l| l.starts_with("picture-start"))
    });
    let pid = lines(&log)
        .iter()
        .find(|l| l.starts_with("picture-start"))
        .and_then(|l| l.split(' ').nth(1).map(str::to_owned))
        .unwrap();
    assert!(!ended(&pid));
    drop(batch);
    wait_for("ffmpeg to be killed", || ended(&pid));
    drop(events);
    assert!(
        !lines(&log).iter().any(|l| l.contains("s1.mp4")),
        "the similar files never started"
    );

    // Now the similar files hang in ffprobe (three at once); stopping kills all three.
    std::fs::remove_file(&log).unwrap();
    let service = PreviewService::new(with(PreviewPrograms {
        ffprobe: Some(ffprobe(&tools, &log, "forever")),
        ffmpeg: Some(ffmpeg(&tools, &log, "0")),
    }));
    let (batch, _events) = service.batch(rt.handle(), None, pics(&files[1..]));
    wait_for("three probes", || lines(&log).len() == DEFAULT_PARALLEL);
    let pids: Vec<String> = lines(&log)
        .iter()
        .filter_map(|l| l.split(' ').nth(1).map(str::to_owned))
        .collect();
    drop(batch);
    wait_for("every ffprobe to be killed", || pids.iter().all(|p| ended(p)));

    // The aborted reads left no placeholder behind: the next request reads the file (with working programs).
    service.set_options(with(PreviewPrograms {
        ffprobe: Some(ffprobe(&tools, &log, "0")),
        ffmpeg: Some(ffmpeg(&tools, &log, "0")),
    }));
    let preview = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(5), service.preview(&files[1])).await })
        .expect("not stuck behind the aborted read")
        .unwrap();
    assert!(preview.thumbnail.is_some());
}

#[test]
fn a_batch_ends_by_itself_when_nobody_listens() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let files = media(dir.path(), &["main.mp4", "s1.mp4"]);
    let service = PreviewService::new(with(PreviewPrograms {
        ffprobe: Some(ffprobe(dir.path(), &log, "0")),
        ffmpeg: Some(ffmpeg(dir.path(), &log, "0.2")),
    }));
    let (batch, events) = service.batch(rt.handle(), Some(pic(&files[0])), pics(&files[1..]));
    drop(events);
    wait_for("the batch to end", || batch.is_finished());
}

#[test]
fn a_stream_without_marked_keyframes_falls_back_to_an_exact_decode() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let files = media(dir.path(), &["clip.mp4"]);
    // This ffmpeg finds no keyframe: with -skip_frame it writes nothing.
    let picky = script(
        dir.path(),
        "ffmpeg",
        &format!(
            "for a; do case \"$a\" in -skip_frame) echo fast >> '{log}'; exit 0;; esac; done\n\
             echo exact >> '{log}'\nhead -c {PICTURE} /dev/zero",
            log = log.display()
        ),
    );
    let service = PreviewService::new(with(PreviewPrograms {
        ffprobe: Some(ffprobe(dir.path(), &dir.path().join("probe.log"), "0")),
        ffmpeg: Some(picky),
    }));
    let preview = rt.block_on(service.preview(&files[0])).unwrap();
    assert!(preview.thumbnail.is_some(), "{:?}", preview.problem);
    assert_eq!(lines(&log), ["fast", "exact"]);
}

#[test]
fn another_picture_place_is_read_again_and_fewer_turns_apply_to_new_reads() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let files = media(dir.path(), &["a.mp4", "b.mp4", "c.mp4", "d.mp4"]);
    let programs = PreviewPrograms {
        ffprobe: Some(ffprobe(dir.path(), &log, "0.1")),
        ffmpeg: Some(ffmpeg(dir.path(), &log, "0")),
    };
    let service = PreviewService::new(with(programs.clone()));
    let first = rt.block_on(service.preview(&files[0])).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &rt.block_on(service.preview(&files[0])).unwrap()
    ));
    service.set_options(PreviewOptions {
        programs,
        picture_at_percent: 50,
        parallel: 1,
        helper: None,
    });
    let moved = rt.block_on(service.preview(&files[0])).unwrap();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &moved),
        "another place is another picture"
    );
    std::fs::remove_file(&log).unwrap();
    let (_batch, mut events) = service.batch(rt.handle(), None, pics(&files[1..]));
    rt.block_on(async { while events.recv().await.is_some() {} });
    let (mut running, mut most) = (0_usize, 0_usize);
    for line in lines(&log) {
        if line.starts_with("probe-start") {
            running += 1;
        } else if line.starts_with("picture-end") {
            running -= 1;
        }
        most = most.max(running);
    }
    assert_eq!(most, 1, "one at a time");
}

#[test]
fn a_description_alone_runs_no_ffmpeg_and_a_cached_picture_serves_it() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let files = media(dir.path(), &["a.mp4", "b.mp4"]);
    let service = PreviewService::new(with(PreviewPrograms {
        ffprobe: Some(ffprobe(dir.path(), &log, "0")),
        ffmpeg: Some(ffmpeg(dir.path(), &log, "0")),
    }));
    let pictured = rt.block_on(service.preview(&files[0])).unwrap();
    let (_batch, mut events) = service.batch(
        rt.handle(),
        Some(PreviewRequest::describe(&files[0])),
        vec![PreviewRequest::describe(&files[1])],
    );
    let got = rt.block_on(async {
        let mut got = Vec::new();
        while let Some(event) = events.recv().await {
            got.push(event);
        }
        got
    });
    let first = got.iter().find(|e| e.slot == PreviewSlot::First).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(first.result.as_ref().unwrap(), &pictured),
        "the cached preview serves"
    );
    let other = got.iter().find(|e| e.slot == PreviewSlot::Rest(0)).unwrap();
    let other = other.result.as_ref().unwrap();
    assert!(other.info.is_some() && other.thumbnail.is_none());
    let pictures = lines(&log).iter().filter(|l| l.starts_with("picture-start")).count();
    assert_eq!(pictures, 1, "only the first preview ran ffmpeg");
}
