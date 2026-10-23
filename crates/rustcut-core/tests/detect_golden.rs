//! The real ffmpeg's detection filters: auto-crop on a generated letterboxed clip (also through a rotation before the
//! crop, and with black scenes between), and the interlace check on PyCutRv's interlaced and progressive test media
//! (`~/claude/pycutrv-testenv/golden/source/`, or `RUSTCUT_GOLDEN_MEDIA`). ffmpeg and the test media are required; the
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

use rustcut_core::edit::{Crop, Filter, FilterChain, FilterKind, LavfiTarget, Rotation, Size};
use rustcut_core::media::{DetectOptions, Interlacing, detect_crop, detect_interlace};
use rustcut_core::time::Timestamp;

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

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

/// A 6 s 640x480 clip: a 640x356 picture with 62 px black bars above and below (odd bars: 61 above, 63 below), where
/// the middle two seconds are completely black (a dark scene must not shrink the result).
fn letterboxed(dir: &Path) -> PathBuf {
    let path = dir.join("letterboxed.mp4");
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(
            "testsrc=size=640x356:rate=25:duration=6,pad=w=640:h=480:x=0:y=61:color=black,\
             drawbox=enable='between(t,2,4)':color=black:t=fill",
        )
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .expect("ffmpeg must be installed (sudo pacman -S ffmpeg)");
    assert!(status.success());
    path
}

#[test]
fn auto_crop_finds_the_bars_over_several_samples() {
    let dir = tempfile::tempdir().unwrap();
    let clip = letterboxed(dir.path());
    let rt = runtime();
    let found = rt
        .block_on(detect_crop(
            &clip,
            Size::new(640, 480),
            Some(Timestamp::from_secs(6)),
            "",
            &DetectOptions::default(),
        ))
        .unwrap();
    // 61 rounds inward to 62; 480 - 62 - 63 = 355 is odd, so one more row goes at the bottom.
    assert_eq!(found.crop, Crop::new(62, 0, 64, 0));
    assert_eq!(found.samples_taken, 3);
    assert_eq!(found.samples_used, 2, "the black middle sample says nothing");

    // With a rotation before the crop, the borders are found in the turned frame the crop gets.
    let mut chain = FilterChain::default();
    chain.set(Filter::Rotate {
        rotation: Rotation::Clockwise,
    });
    chain.set_enabled(FilterKind::Rotate, true);
    while chain.move_up(FilterKind::Rotate) {}
    let prefix = chain.before(FilterKind::Crop).to_lavfi(LavfiTarget::Preview).unwrap();
    assert_eq!(prefix, "transpose=dir=clock");
    let turned = rt
        .block_on(detect_crop(
            &clip,
            Size::new(480, 640),
            Some(Timestamp::from_secs(6)),
            &prefix,
            &DetectOptions::default(),
        ))
        .unwrap();
    // Clockwise: the bottom bar (63) is now on the left, the top bar (61) on the right.
    assert_eq!(turned.crop, Crop::new(0, 62, 0, 64));
}

#[test]
fn the_interlace_check_tells_interlaced_from_progressive() {
    let rt = runtime();
    let options = DetectOptions::default();
    let interlaced = rt
        .block_on(detect_interlace(
            &golden_dir().join("interlaced_pal_576i.mpg"),
            None,
            &options,
        ))
        .unwrap();
    assert_eq!(interlaced.verdict(), Interlacing::Interlaced { top_field_first: true });
    let progressive = rt
        .block_on(detect_interlace(
            &golden_dir().join("hd_h264_1080p.mp4"),
            Some(Timestamp::from_secs(10)),
            &options,
        ))
        .unwrap();
    assert_eq!(progressive.verdict(), Interlacing::Progressive, "{progressive:?}");
    // Synthetic test patterns fool idet into both field orders: no verdict rather than a wrong one.
    let synthetic = rt
        .block_on(detect_interlace(
            &golden_dir().join("basic_h264_720p.mp4"),
            None,
            &options,
        ))
        .unwrap();
    assert_eq!(synthetic.verdict(), Interlacing::Unknown, "{synthetic:?}");
}

#[test]
fn a_broken_file_is_an_error_not_a_verdict() {
    let rt = runtime();
    let broken = golden_dir().join("broken");
    let file = std::fs::read_dir(&broken)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_file())
        .expect("a broken test file");
    assert!(
        rt.block_on(detect_interlace(&file, None, &DetectOptions::default()))
            .is_err()
    );
}
