//! The real ffmpeg's detection filters: auto-crop on a generated letterboxed clip (also through a rotation before the
//! crop, with black scenes between, with dark gray bars, on chosen sides and inside sections), and the interlace check on PyCutRv's interlaced and progressive test media
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

use rustcut_core::edit::{Crop, Filter, FilterChain, FilterKind, LavfiTarget, Orientation, Size};
use rustcut_core::media::{
    CROP_SAMPLES, CropSearch, CropSides, DetectOptions, Interlacing, crop_sample_times, detect_crop, detect_interlace,
};
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
    boxed(dir, "letterboxed.mp4", "pad=w=640:h=480:x=0:y=61:color=black")
}

/// The 640x356 test picture of [`letterboxed`] padded by `pad` (the middle two seconds black).
fn boxed(dir: &Path, name: &str, pad: &str) -> PathBuf {
    let path = dir.join(name);
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "testsrc=size=640x356:rate=25:duration=6,{pad},drawbox=enable='between(t,2,4)':color=black:t=fill"
        ))
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
    let times = crop_sample_times(Some(Timestamp::from_secs(6)), CROP_SAMPLES, &[]);
    let search = CropSearch::default();
    let found = rt
        .block_on(detect_crop(
            &clip,
            Size::new(640, 480),
            &times,
            "",
            &search,
            &DetectOptions::default(),
        ))
        .unwrap();
    // 61 rounds inward to 62; 480 - 62 - 63 = 355 is odd, so one more row goes at the bottom.
    assert_eq!(found.crop, Crop::new(62, 0, 64, 0));
    assert_eq!(found.samples_taken, 3);
    assert_eq!(found.samples_used, 2, "the black middle sample says nothing");

    // With a rotation before the crop, the borders are found in the turned frame the crop gets.
    let mut chain = FilterChain::default();
    chain.set(Filter::Orientation(Orientation::CLOCKWISE.into()));
    chain.set_enabled(FilterKind::Orientation, true);
    while chain.move_up(FilterKind::Orientation) {}
    let prefix = chain.before(FilterKind::Crop).to_lavfi(LavfiTarget::Preview).unwrap();
    assert_eq!(prefix, "transpose=dir=clock");
    let turned = rt
        .block_on(detect_crop(
            &clip,
            Size::new(480, 640),
            &times,
            &prefix,
            &search,
            &DetectOptions::default(),
        ))
        .unwrap();
    // Clockwise: the bottom bar (63) is now on the left, the top bar (61) on the right.
    assert_eq!(turned.crop, Crop::new(0, 62, 0, 64));
}

#[test]
fn auto_crop_options_dark_gray_bars_sides_and_sections() {
    let dir = tempfile::tempdir().unwrap();
    // A plain 600x356 picture (a high limit also eats into dark picture parts, see `CropSearch::limit`) in dark gray
    // (#303030) bars: 20 px left and right, 62 px above and below.
    let clip = boxed(
        dir.path(),
        "gray.mp4",
        "crop=w=600:x=20,drawbox=color=0x8090a0:t=fill,pad=w=640:h=480:x=20:y=62:color=0x303030",
    );
    let rt = runtime();
    let detect = |search: CropSearch, times: &[Timestamp]| {
        rt.block_on(detect_crop(
            &clip,
            Size::new(640, 480),
            times,
            "",
            &search,
            &DetectOptions::default(),
        ))
        .unwrap()
        .crop
    };
    let whole = crop_sample_times(Some(Timestamp::from_secs(6)), CROP_SAMPLES, &[]);
    // The default limit takes the gray for picture; a higher one counts it as border.
    assert_eq!(detect(CropSearch::default(), &whole), Crop::default());
    let gray = CropSearch {
        limit: 64,
        ..CropSearch::default()
    };
    assert_eq!(detect(gray, &whole), Crop::new(62, 20, 62, 20));
    let top_bottom = CropSearch {
        sides: CropSides::TopBottom,
        ..gray
    };
    assert_eq!(detect(top_bottom, &whole), Crop::new(62, 0, 62, 0));
    // Only the black middle seconds as the section: nothing is found, nothing is cut.
    let black = crop_sample_times(
        None,
        2,
        &[(Timestamp::from_millis(2_200), Timestamp::from_millis(3_800))],
    );
    assert_eq!(detect(gray, &black), Crop::default());
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
