//! The real ffprobe over PyCutRv's test media (the folder `RUSTCUT_GOLDEN_MEDIA` names, by default `pycutrv-testenv/golden/source/` next to the repository):
//! every file's Media Info text is a snapshot, and the special cases (rotation, odd size, two audio tracks, no audio,
//! interlaced, broken files, a variable frame rate) are asserted directly. ffprobe and the test media are required; the
//! tests fail with what is missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use rustcut_core::media::{
    FrameRateMode, MediaInfo, ProbeError, ProbeOptions, StreamKind, probe, probe_with_timing, summarize, to_text,
};
use rustcut_core::time::Timestamp;

fn golden_dir() -> PathBuf {
    let dir = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    assert!(
        dir.is_dir(),
        "the test media are missing: {} (set RUSTCUT_GOLDEN_MEDIA or place the PyCutRv test media next to the repository)",
        dir.display()
    );
    dir
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn probe_file(path: &Path) -> Result<MediaInfo, ProbeError> {
    runtime().block_on(probe_with_timing(path, &ProbeOptions::default()))
}

fn probe_golden(name: &str) -> MediaInfo {
    let path = golden_dir().join(name);
    probe_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Snapshot names for the test media (ASCII, one per file).
const FILES: &[(&str, &str)] = &[
    ("umlauts", "Ärger über Größe [test] (1).mp4"),
    ("audio_ends_early", "audio_ends_early_3s.mp4"),
    ("av1_svt", "av1_svt.mkv"),
    ("basic_h264_720p", "basic_h264_720p.mp4"),
    ("hd_h264_1080p", "hd_h264_1080p.mp4"),
    ("hevc_10bit", "hevc_10bit.mkv"),
    ("interlaced_pal_576i", "interlaced_pal_576i.mpg"),
    ("long_h264_320x180", "long_h264_320x180.mp4"),
    ("mpeg4_mp3", "mpeg4_mp3.avi"),
    ("multi_audio_2tracks", "multi_audio_2tracks.mkv"),
    ("no_audio_h264", "no_audio_h264.mp4"),
    ("odd_size_yuv444", "odd_size_853x479_yuv444.mp4"),
    ("portrait_h264_720x1280", "portrait_h264_720x1280.mp4"),
    ("prores_422", "prores_422_320x180.mov"),
    ("rotated_90_metadata", "rotated_90_metadata.mp4"),
    ("shaky_h264_640x360", "shaky_h264_640x360.mp4"),
    ("vp9_opus", "vp9_opus.webm"),
    ("clip_with_sidecar", "with_md5/clip_with_sidecar.mp4"),
];

#[test]
fn every_test_file_has_its_media_info_snapshot() {
    for (snapshot, file) in FILES {
        let info = probe_golden(file);
        insta::assert_snapshot!(*snapshot, to_text(&summarize(&info)), file);
    }
}

#[test]
fn rotation_odd_size_and_portrait() {
    let rotated = probe_golden("rotated_90_metadata.mp4");
    let (_, video) = rotated.primary_video().unwrap();
    assert_eq!(
        video.rotation, 270,
        "ffprobe's 90 degrees counter-clockwise = mpv's 270 clockwise"
    );
    assert_eq!(video.oriented_size(), Some((360, 640)));

    let odd = probe_golden("odd_size_853x479_yuv444.mp4");
    let (_, video) = odd.primary_video().unwrap();
    // The file name promises 853x479, but the encoder stored an even size.
    assert_eq!((video.width, video.height), (Some(852), Some(478)));
    assert_eq!(video.pixel_format.as_deref(), Some("yuv444p"));

    let portrait = probe_golden("portrait_h264_720x1280.mp4");
    assert_eq!(portrait.primary_video().unwrap().1.oriented_size(), Some((720, 1280)));
}

#[test]
fn audio_tracks() {
    let two = probe_golden("multi_audio_2tracks.mkv");
    let languages: Vec<Option<&str>> = two.audio_streams().map(|(s, _)| s.language.as_deref()).collect();
    assert_eq!(languages, [Some("eng"), Some("deu")]);

    let none = probe_golden("no_audio_h264.mp4");
    assert!(!none.has_audio());
    assert!(none.primary_video().is_some());

    let early = probe_golden("audio_ends_early_3s.mp4");
    let (audio, _) = early.audio_streams().next().unwrap();
    assert_eq!(audio.duration.map(Timestamp::as_micros), Some(3_000_000));
    assert_eq!(early.duration().map(Timestamp::as_micros), Some(20_000_000));
}

#[test]
fn interlaced_ten_bit_and_constant_rates() {
    let interlaced = probe_golden("interlaced_pal_576i.mpg");
    assert!(interlaced.primary_video().unwrap().1.is_interlaced());
    assert!(interlaced.container.start_time.is_some_and(|s| s.as_micros() > 500_000));

    let hevc = probe_golden("hevc_10bit.mkv");
    assert_eq!(hevc.primary_video().unwrap().1.bit_depth, Some(10));

    for (_, file) in FILES {
        let info = probe_golden(file);
        assert_eq!(info.frame_rate_mode(), FrameRateMode::Constant, "{file}");
        assert!(info.frame_timing.is_some(), "{file}: packet timestamps were sampled");
    }
}

#[test]
fn broken_files_are_reported_not_crashed_on() {
    for file in ["broken/empty.mp4", "broken/not_a_video.mp4", "broken/truncated.mp4"] {
        let path = golden_dir().join(file);
        match probe_file(&path) {
            Err(ProbeError::Unreadable(reason)) => {
                assert_eq!(reason, "Invalid data found when processing input", "{file}");
            }
            other => panic!("{file}: {other:?}"),
        }
    }
    match probe_file(&golden_dir().join("does-not-exist.mp4")) {
        Err(ProbeError::Unreadable(reason)) => assert_eq!(reason, "No such file or directory"),
        other => panic!("{other:?}"),
    }
}

/// A clip that changes from 30 to 10 fps after two seconds, like a screen recorder writes it, in MP4 (the average rate
/// shows it) and Matroska (only the timestamps show it).
#[test]
fn a_variable_frame_rate_is_detected_in_mp4_and_matroska() {
    let dir = tempfile::tempdir().unwrap();
    for (name, codec) in [("vfr.mp4", "libx264"), ("vfr.mkv", "ffv1")] {
        let path = dir.path().join(name);
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=160x120:r=30:d=4"])
            .args([
                "-vf",
                "select='lt(n\\,60)+not(mod(n\\,3))'",
                "-fps_mode",
                "vfr",
                "-c:v",
                codec,
            ])
            .args(["-pix_fmt", "yuv420p"])
            .arg(&path)
            .status()
            .unwrap_or_else(|e| panic!("ffmpeg is required for this test (sudo pacman -S ffmpeg): {e}"));
        assert!(status.success(), "ffmpeg could not write {name}");
        let info = probe_file(&path).unwrap();
        match info.frame_rate_mode() {
            FrameRateMode::Variable {
                min_fps: Some(min),
                max_fps: Some(max),
            } => {
                assert!(
                    (min - 10.0).abs() < 0.5 && (max - 30.0).abs() < 1.0,
                    "{name}: {min} to {max}"
                );
            }
            other => panic!("{name}: {other:?}"),
        }
        let text = to_text(&summarize(&info));
        assert!(text.contains("Frame rate: Variable, 10 to "), "{name}:\n{text}");
        assert!(matches!(info.streams[0].kind, StreamKind::Video(_)));
    }
}

/// A preallocated, partly downloaded file: the header is fine, gigabytes of zeros follow. The media description comes at
/// once; only the frame-timing sample runs into its own time limit and is left out.
#[test]
fn a_preallocated_download_is_described_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial download.mkv");
    std::fs::copy(golden_dir().join("av1_svt.mkv"), &path).unwrap();
    // Sparse: no disk space is used.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(2 * 1024 * 1024 * 1024)
        .unwrap();
    let options = ProbeOptions {
        timing_timeout: std::time::Duration::from_secs(1),
        ..ProbeOptions::default()
    };
    let rt = runtime();
    let started = std::time::Instant::now();
    let info = rt.block_on(probe(&path, &options)).unwrap();
    assert!(
        started.elapsed().as_secs_f64() < 3.0,
        "the header probe took {:?}",
        started.elapsed()
    );
    assert_eq!(info.streams.len(), 2);
    let info = rt.block_on(probe_with_timing(&path, &options)).unwrap();
    assert_eq!(info.frame_timing, None, "the sample gave up");
    assert_eq!(info.frame_rate_mode(), FrameRateMode::Constant, "the rates still tell");
}
