//! The real ffprobe and ffmpeg behind the previews (M5.4): a picture and a description of PyCutRv's test media
//! (`~/claude/pycutrv-testenv/golden/source/`, or `RUSTCUT_GOLDEN_MEDIA`), a portrait picture through the source's
//! rotation, a broken file, the cache by path + size + time, and requests that run on the runtime's threads. ffmpeg and
//! the test media are required; the tests fail with what is missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustcut_core::media::{PreviewOptions, PreviewPrograms, PreviewService, THUMB_HEIGHT, THUMB_WIDTH, Thumbnail};

fn golden_dir() -> PathBuf {
    let dir = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || {
            let home = std::env::var_os("HOME").expect("HOME is set");
            Path::new(&home).join("claude/pycutrv-testenv/golden/source")
        },
        PathBuf::from,
    );
    assert!(
        dir.is_dir(),
        "the test media are missing: {} (set RUSTCUT_GOLDEN_MEDIA to the folder with PyCutRv's golden sources)",
        dir.display()
    );
    dir
}

fn with(programs: PreviewPrograms) -> PreviewOptions {
    PreviewOptions {
        programs,
        ..PreviewOptions::default()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

/// The alpha value at `(x, y)`.
fn alpha(t: &Thumbnail, x: u32, y: u32) -> u8 {
    let i = usize::try_from((y * t.width + x) * 4 + 3).unwrap();
    t.rgba[i]
}

#[test]
fn a_landscape_video_fills_the_picture_and_is_described() {
    let rt = runtime();
    let service = PreviewService::new(PreviewOptions::default());
    let path = golden_dir().join("hd_h264_1080p.mp4");
    let preview = rt.block_on(service.preview(&path)).unwrap();
    assert_eq!(preview.problem, None);
    let info = preview.info.as_ref().unwrap();
    assert_eq!(info.primary_video().unwrap().1.oriented_size(), Some((1920, 1080)));
    assert_eq!(preview.bytes, std::fs::metadata(&path).unwrap().len());
    let t = preview.thumbnail.as_ref().unwrap();
    assert_eq!((t.width, t.height), (THUMB_WIDTH, THUMB_HEIGHT));
    assert_eq!(t.rgba.len(), usize::try_from(THUMB_WIDTH * THUMB_HEIGHT * 4).unwrap());
    for (x, y) in [
        (0, 0),
        (THUMB_WIDTH - 1, THUMB_HEIGHT - 1),
        (THUMB_WIDTH / 2, THUMB_HEIGHT / 2),
    ] {
        assert_eq!(alpha(t, x, y), 255, "16:9 fills the picture at {x},{y}");
    }
}

#[test]
fn a_rotated_portrait_video_stands_in_the_middle() {
    let rt = runtime();
    let service = PreviewService::new(PreviewOptions::default());
    let preview = rt
        .block_on(service.preview(&golden_dir().join("rotated_90_metadata.mp4")))
        .unwrap();
    let t = preview.thumbnail.as_ref().expect("a picture");
    // Shown 360x640: 101x180 in the middle, transparent at the sides.
    assert_eq!(alpha(t, 2, THUMB_HEIGHT / 2), 0, "transparent left");
    assert_eq!(alpha(t, THUMB_WIDTH - 3, THUMB_HEIGHT / 2), 0, "transparent right");
    assert_eq!(
        alpha(t, THUMB_WIDTH / 2, 2),
        255,
        "the picture is upright: it reaches the top"
    );
    assert_eq!(alpha(t, THUMB_WIDTH / 2, THUMB_HEIGHT - 3), 255, "and the bottom");
}

#[test]
fn a_broken_file_has_a_problem_and_no_picture() {
    let rt = runtime();
    let service = PreviewService::new(PreviewOptions::default());
    let dir = golden_dir().join("broken");
    for name in ["not_a_video.mp4", "empty.mp4"] {
        let preview = rt.block_on(service.preview(&dir.join(name))).unwrap();
        assert!(preview.thumbnail.is_none(), "{name}");
        assert!(preview.problem.is_some(), "{name}");
    }
    let gone = rt.block_on(service.preview(&dir.join("no such file.mp4")));
    assert!(gone.is_err());
    let folder = rt.block_on(service.preview(&dir));
    assert!(folder.is_err(), "a folder is no file");
}

#[test]
fn previews_are_cached_until_the_file_changes() {
    let rt = runtime();
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("clip.mp4");
    std::fs::copy(golden_dir().join("shaky_h264_640x360.mp4"), &path).unwrap();
    let service = PreviewService::new(PreviewOptions::default());
    // Several requests at once from the runtime's threads share one read.
    let requests: Vec<_> = (0..4)
        .map(|_| {
            let (service, path) = (service.clone(), path.clone());
            rt.spawn(async move { service.preview(&path).await.unwrap() })
        })
        .collect();
    let first: Vec<_> = requests.into_iter().map(|r| rt.block_on(r).unwrap()).collect();
    assert!(
        first.iter().all(|p| Arc::ptr_eq(p, &first[0])),
        "one read for four requests"
    );
    let again = rt.block_on(service.preview(&path)).unwrap();
    assert!(Arc::ptr_eq(&again, &first[0]), "from the cache");
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let changed = rt.block_on(service.preview(&path)).unwrap();
    assert!(!Arc::ptr_eq(&changed, &first[0]), "a changed file is read again");
    assert!(changed.thumbnail.is_some());
}

#[test]
fn a_missing_ffmpeg_is_a_problem_not_an_error() {
    let rt = runtime();
    let service = PreviewService::new(with(PreviewPrograms {
        ffmpeg: Some(PathBuf::from("/nonexistent/ffmpeg")),
        ffprobe: None,
    }));
    let preview = rt
        .block_on(service.preview(&golden_dir().join("basic_h264_720p.mp4")))
        .unwrap();
    assert!(preview.info.is_some(), "ffprobe described it");
    assert!(preview.thumbnail.is_none());
    assert!(
        preview.problem.as_deref().unwrap().contains("not found"),
        "{:?}",
        preview.problem
    );
}

#[test]
fn every_video_format_rustcut_lists_is_read_by_the_installed_ffmpeg() {
    let out = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-demuxers"])
        .output()
        .expect("ffmpeg must be installed (sudo pacman -S ffmpeg)");
    let text = String::from_utf8_lossy(&out.stdout);
    // Lines like " D  matroska,webm    Matroska / WebM": the names are the second column, comma-separated.
    let demuxers: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .flat_map(|names| names.split(','))
        .collect();
    for (ext, demuxer) in rustcut_core::media::VIDEO_FORMATS {
        assert!(demuxers.contains(demuxer), ".{ext}: ffmpeg has no demuxer {demuxer}");
    }
}

/// The keyframe alone makes the picture of every test video at once: the fallbacks (an exact decode, the start) are for
/// odd files, not the rule. Before the timestamps passed through, 10 of these 18 files gave no picture this way.
#[test]
fn the_fast_picture_works_for_every_test_video() {
    use rustcut_core::media::{picture_time, thumbnail_args};
    let rt = runtime();
    let service = PreviewService::new(PreviewOptions::default());
    let dir = golden_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && rustcut_core::media::is_video_file(p))
        .collect();
    files.sort();
    assert!(files.len() >= 15, "{} test videos", files.len());
    for file in files {
        let preview = rt.block_on(service.preview(&file)).unwrap();
        let info = preview.info.as_ref().unwrap();
        let (stream, _) = info.primary_video().unwrap();
        let at = picture_time(info.duration(), 10);
        let out = std::process::Command::new("ffmpeg")
            .args(thumbnail_args(&file, stream.index, at, true))
            .output()
            .unwrap();
        assert_eq!(
            out.stdout.len(),
            usize::try_from(THUMB_WIDTH * THUMB_HEIGHT * 4).unwrap(),
            "{}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
