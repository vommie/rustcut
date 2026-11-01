//! The real helper on the real test media: fails, never skips, without them (like the other golden tests).

#![allow(clippy::unwrap_used, reason = "a test stops at the first thing that fails")]

use std::io::{BufReader, Write as _};
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};

use rustcut_thumbs_protocol::{Command, Job, Reply, Start, read_reply};

fn media(name: &str) -> String {
    let root = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap()).join("claude/pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    let path = root.join(name);
    assert!(path.is_file(), "missing test media {}", path.display());
    path.to_str().unwrap().to_owned()
}

/// Runs a helper over the jobs until it is idle; every reply with its picture.
fn run(path: String, jobs: Vec<Job>) -> Vec<(Reply, Vec<u8>)> {
    let mut child = Process::new(env!("CARGO_BIN_EXE_rustcut-thumbs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let start = Start {
        path,
        long_side: 320,
        quality: 50,
        workers: 2,
        nice: 0,
    };
    writeln!(stdin, "{}", serde_json::to_string(&start).unwrap()).unwrap();
    let mut replies = Vec::new();
    loop {
        let (reply, picture) = read_reply(&mut stdout).unwrap();
        match reply {
            Reply::Ready { .. } => {
                writeln!(stdin, "{}", serde_json::to_string(&Command::Add(jobs.clone())).unwrap()).unwrap();
            }
            Reply::Idle => break,
            Reply::Failed { .. } => {
                replies.push((reply, picture));
                break;
            }
            _ => replies.push((reply, picture)),
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success() || replies.iter().any(|(r, _)| matches!(r, Reply::Failed { .. })));
    replies
}

fn thumbs(replies: &[(Reply, Vec<u8>)]) -> usize {
    replies
        .iter()
        .filter(|(reply, _)| matches!(reply, Reply::Thumb { .. }))
        .count()
}

#[test]
fn a_keyframe_comes_once_and_the_rest_is_known() {
    let jobs = vec![Job::Key { ms: 1000 }, Job::Key { ms: 2000 }, Job::Key { ms: 3000 }];
    let replies = run(media("no_audio_h264.mp4"), jobs);
    assert_eq!(thumbs(&replies), 1, "{replies:?}");
    assert_eq!(replies.len(), 3);
    let known = replies
        .iter()
        .filter(|(reply, _)| matches!(reply, Reply::Known { time_ms: 0, .. }))
        .count();
    assert_eq!(known, 2);
}

#[test]
fn a_span_decodes_every_target_and_the_pictures_are_webp() {
    let targets: Vec<u64> = (0..16).map(|i| i * 500).collect();
    let replies = run(
        media("no_audio_h264.mp4"),
        vec![Job::Span {
            start_ms: 0,
            targets: targets.clone(),
        }],
    );
    assert_eq!(thumbs(&replies), targets.len());
    for (reply, picture) in &replies {
        let Reply::Thumb {
            target_ms,
            time_ms,
            exact,
            width,
            height,
        } = reply
        else {
            panic!("{reply:?}");
        };
        assert!(*exact);
        assert_eq!((*width, *height), (320, 180));
        assert!(time_ms.abs_diff(*target_ms) < 200, "{time_ms} for {target_ms}");
        assert_eq!(picture.get(..4), Some(&b"RIFF"[..]));
        assert_eq!(picture.get(8..12), Some(&b"WEBP"[..]));
    }
}

#[test]
fn exact_lands_on_the_time_in_every_codec() {
    for name in [
        "hevc_10bit.mkv",
        "av1_svt.mkv",
        "vp9_opus.webm",
        "mpeg4_mp3.avi",
        "prores_422_320x180.mov",
    ] {
        let replies = run(media(name), vec![Job::Exact { ms: 3000 }]);
        let [(Reply::Thumb { time_ms, .. }, _)] = replies.as_slice() else {
            panic!("{name}: {replies:?}");
        };
        assert!(time_ms.abs_diff(3000) < 100, "{name}: {time_ms}");
    }
}

#[test]
fn a_file_that_is_no_video_fails_before_any_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.mp4");
    std::fs::write(&path, b"this is no video").unwrap();
    let replies = run(path.to_str().unwrap().to_owned(), vec![]);
    assert!(matches!(replies.as_slice(), [(Reply::Failed { .. }, _)]), "{replies:?}");
}
