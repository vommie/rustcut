//! The lavfi text, the concat graph and the encoder arguments that the core builds, run through the real ffmpeg.
//! ffmpeg (with libsvtav1, libx264, libx265, libaom, libopus and vid.stab) is a required dependency (scripts/check-deps.sh);
//! these tests fail with the install command when it is missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use rustcut_core::edit::{
    ConcatInput, Crop, DeinterlaceRate, Deinterlacer, DeshakeParams, DeshakePass, FieldParity, Filter, FilterChain,
    LavfiTarget, Orientation, OrientationParams, ResizeAlgorithm, Size, concat_graph,
};
use rustcut_core::render::{
    EncodeContext, RateControl, RenderProfile, VideoEncoder, VideoPass, builtin_profiles, profile_args, video_args,
};
use rustcut_core::time::Timestamp;

fn run(program: &str, args: &[String]) -> String {
    let out = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{program} is required for this test (sudo pacman -S ffmpeg): {e}"));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{program} {}\nfailed:\n{stderr}", args.join(" "));
    assert!(
        !stderr.contains("Error parsing option"),
        "{program} {}\nrejected an option:\n{stderr}",
        args.join(" ")
    );
    String::from_utf8_lossy(&out.stdout).into_owned() + &stderr
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

/// A short yuv420p test clip: `size` (odd sizes allowed; `testsrc2` would round them to even), 10 fps, 1 s, with
/// `audio_tracks` sine tracks.
fn make_source(dir: &Path, size: Size, audio_tracks: usize) -> PathBuf {
    let path = dir.join("source.mkv");
    let mut args = s(&["-hide_banner", "-y", "-f", "lavfi", "-i"]);
    args.push(format!(
        "testsrc=s={}x{}:r=10:d=1,format=yuv420p",
        size.width, size.height
    ));
    for t in 0..audio_tracks {
        args.extend(s(&["-f", "lavfi", "-i"]));
        args.push(format!("sine=frequency={}:sample_rate=44100:duration=1", 440 + 110 * t));
    }
    for i in 0..=audio_tracks {
        args.extend(["-map".into(), format!("{i}")]);
    }
    args.extend(s(&["-c:v", "ffv1", "-c:a", "flac"]));
    args.push(path.to_string_lossy().into_owned());
    run("ffmpeg", &args);
    assert_eq!(probe_size(&path), size, "test source");
    path
}

fn probe_size(path: &Path) -> Size {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe is required (sudo pacman -S ffmpeg)");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.trim().split(',').map(|v| v.parse::<u32>().unwrap());
    Size::new(parts.next().unwrap(), parts.next().unwrap())
}

fn chain_with(filters: &[Filter]) -> FilterChain {
    let mut chain = FilterChain::default();
    for &f in filters {
        chain.set(f);
        chain.set_enabled(f.kind(), true);
    }
    chain
}

#[test]
fn every_filter_runs_and_the_output_size_is_predicted() {
    let dir = tempfile::tempdir().unwrap();
    let source_size = Size::new(321, 241);
    let source = make_source(dir.path(), source_size, 0);
    let chains = [
        chain_with(&[]),
        chain_with(&[Filter::Deinterlace {
            deinterlacer: Deinterlacer::Bwdif,
            rate: DeinterlaceRate::Field,
            field_order: FieldParity::TopFirst,
            only_interlaced: false,
        }]),
        chain_with(&[Filter::Deinterlace {
            deinterlacer: Deinterlacer::Yadif,
            rate: DeinterlaceRate::Frame,
            field_order: FieldParity::BottomFirst,
            only_interlaced: true,
        }]),
        chain_with(&[Filter::Deinterlace {
            deinterlacer: Deinterlacer::W3fdif,
            rate: DeinterlaceRate::Field,
            field_order: FieldParity::Auto,
            only_interlaced: true,
        }]),
        chain_with(&[Filter::Deinterlace {
            deinterlacer: Deinterlacer::Kerndeint,
            rate: DeinterlaceRate::Frame,
            field_order: FieldParity::TopFirst,
            only_interlaced: true,
        }]),
        chain_with(&[Filter::Crop(Crop::new(3, 10, 4, 7))]),
        chain_with(&[
            Filter::Orientation(Orientation::CLOCKWISE.into()),
            Filter::Crop(Crop::new(1, 2, 3, 4)),
        ]),
        chain_with(&[Filter::Resize {
            width: Some(200),
            height: None,
            algorithm: ResizeAlgorithm::Lanczos,
        }]),
        chain_with(&[Filter::Resize {
            width: None,
            height: Some(99),
            algorithm: ResizeAlgorithm::Spline,
        }]),
        chain_with(&[
            Filter::Crop(Crop::new(20, 0, 20, 0)),
            Filter::Orientation(Orientation::CLOCKWISE.into()),
            Filter::Resize {
                width: Some(150),
                height: None,
                algorithm: ResizeAlgorithm::Bilinear,
            },
        ]),
    ];
    // Every orientation (the odd source shows when width and height trade places).
    let orientations = (0..4).flat_map(|turns| {
        [false, true].map(|mirrored| {
            chain_with(&[Filter::Orientation(OrientationParams {
                turns,
                mirror_horizontal: mirrored,
                mirror_vertical: false,
            })])
        })
    });
    for (i, chain) in chains.into_iter().chain(orientations).enumerate() {
        let lavfi = chain
            .to_lavfi(LavfiTarget::Render {
                source: source_size,
                deshake: DeshakePass::None,
            })
            .unwrap();
        let out = dir.path().join(format!("out{i}.nut"));
        let mut args = s(&["-hide_banner", "-y", "-i"]);
        args.push(source.to_string_lossy().into_owned());
        if !lavfi.is_empty() {
            args.extend(["-vf".into(), lavfi.clone()]);
        }
        args.extend(s(&["-c:v", "rawvideo", "-f", "nut"]));
        args.push(out.to_string_lossy().into_owned());
        run("ffmpeg", &args);
        let expected = chain.output_size(source_size).unwrap().even_floor();
        assert_eq!(probe_size(&out), expected, "chain {i}: {lavfi}");
        let preview = chain.to_lavfi(LavfiTarget::Preview).unwrap();
        assert!(
            lavfi.starts_with(&preview),
            "the preview is the render chain without the even-size guard: {preview} / {lavfi}"
        );
    }
}

#[test]
fn deshake_two_passes_with_a_hostile_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = make_source(dir.path(), Size::new(320, 240), 0);
    let odd_dir = dir.path().join("a: b, [c]; 'd'");
    std::fs::create_dir(&odd_dir).unwrap();
    let trf = odd_dir.join("job-1.trf");
    // The limits and the adaptive zoom's speed are in vid.stab's ranges (maxangle in radians).
    let chain = chain_with(&[
        Filter::Crop(Crop::new(0, 0, 40, 0)),
        Filter::Deshake(DeshakeParams {
            optzoom: 2,
            zoom_speed: DeshakeParams::ZOOM_SPEED_MAX,
            max_shift: Some(DeshakeParams::MAX_SHIFT),
            max_angle: Some(DeshakeParams::MAX_ANGLE),
            ..DeshakeParams::default()
        }),
        Filter::Resize {
            width: Some(160),
            height: None,
            algorithm: ResizeAlgorithm::Bicubic,
        },
    ]);
    let size = Size::new(320, 240);
    let detect = chain
        .to_lavfi(LavfiTarget::Render {
            source: size,
            deshake: DeshakePass::Detect { result: &trf },
        })
        .unwrap();
    let mut args = s(&["-hide_banner", "-y", "-i"]);
    args.push(source.to_string_lossy().into_owned());
    args.extend(["-vf".into(), detect, "-f".into(), "null".into(), "-".into()]);
    run("ffmpeg", &args);
    assert!(
        trf.is_file(),
        "vidstabdetect wrote the transforms (needs vid.stab: sudo pacman -S vid.stab)"
    );

    let transform = chain
        .to_lavfi(LavfiTarget::Render {
            source: size,
            deshake: DeshakePass::Transform { input: &trf },
        })
        .unwrap();
    let out = dir.path().join("stable.nut");
    let mut args = s(&["-hide_banner", "-y", "-i"]);
    args.push(source.to_string_lossy().into_owned());
    args.extend([
        "-vf".into(),
        transform,
        "-c:v".into(),
        "rawvideo".into(),
        "-f".into(),
        "nut".into(),
    ]);
    args.push(out.to_string_lossy().into_owned());
    run("ffmpeg", &args);
    assert_eq!(probe_size(&out), Size::new(160, 100));
}

#[test]
fn concat_keeps_every_section_and_audio_track() {
    let dir = tempfile::tempdir().unwrap();
    let source = make_source(dir.path(), Size::new(160, 120), 2);
    let sections = [(0.0, 0.3), (0.5, 0.9)];
    let graph = concat_graph(
        ConcatInput {
            sections: sections.len(),
            audio_tracks: 2,
        },
        "hflip",
    )
    .unwrap();
    let mut args = s(&["-hide_banner", "-y"]);
    for (start, end) in sections {
        args.extend([
            "-ss".into(),
            format!("{start}"),
            "-to".into(),
            format!("{end}"),
            "-i".into(),
        ]);
        args.push(source.to_string_lossy().into_owned());
    }
    args.extend([
        "-filter_complex".into(),
        graph.graph.clone(),
        "-map".into(),
        format!("[{}]", graph.video),
    ]);
    for a in &graph.audio {
        args.extend(["-map".into(), format!("[{a}]")]);
    }
    let out = dir.path().join("cut.mkv");
    args.extend(s(&["-c:v", "ffv1", "-c:a", "flac"]));
    args.push(out.to_string_lossy().into_owned());
    run("ffmpeg", &args);
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type:format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(text.lines().filter(|l| l.starts_with("audio")).count(), 2, "{text}");
    let duration: f64 = text.lines().find_map(|l| l.parse().ok()).unwrap();
    assert!((duration - 0.7).abs() < 0.15, "duration {duration} for 0.3 s + 0.4 s");
}

fn encode(profile: &RenderProfile, source: &Path, out: &Path, pass: VideoPass<'_>) -> String {
    let ctx = EncodeContext {
        duration: Timestamp::from_secs(1),
        audio_streams: 1,
        pass,
    };
    let mut args = s(&["-hide_banner", "-y", "-i"]);
    args.push(source.to_string_lossy().into_owned());
    args.extend(s(&["-map", "0:v:0", "-map", "0:a:0", "-frames:v", "3"]));
    if matches!(pass, VideoPass::First { .. }) {
        args.extend(video_args(&profile.video, &ctx, &profile.audio).unwrap());
        args.extend(s(&["-an", "-f", "null", "-"]));
    } else {
        args.extend(profile_args(profile, &ctx).unwrap());
        args.push(out.to_string_lossy().into_owned());
    }
    run("ffmpeg", &args)
}

#[test]
fn every_built_in_profile_encodes() {
    let dir = tempfile::tempdir().unwrap();
    let source = make_source(dir.path(), Size::new(160, 120), 1);
    for profile in builtin_profiles() {
        let out = dir
            .path()
            .join(format!("{}.{}", profile.id, profile.container.extension()));
        encode(&profile, &source, &out, VideoPass::Single);
        assert_eq!(probe_size(&out), Size::new(160, 120), "{}", profile.id);
    }
}

#[test]
fn two_pass_encoders_accept_their_pass_flags() {
    let dir = tempfile::tempdir().unwrap();
    let source = make_source(dir.path(), Size::new(160, 120), 1);
    let log_dir = dir.path().join("logs: x");
    std::fs::create_dir(&log_dir).unwrap();
    let log = log_dir.join("passlog");
    for encoder in [VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::Aom] {
        let mut profile = builtin_profiles().remove(0);
        profile.video.encoder = encoder;
        profile.video.rate_control = RateControl::TwoPass { kbps: 300 };
        profile.video.preset = None;
        profile.video.tune = None;
        profile.video.film_grain = None;
        profile.video.codec_params.clear();
        profile.video.bit_depth = rustcut_core::render::BitDepth::Eight;
        let out = dir.path().join(format!("{encoder:?}.mkv"));
        encode(&profile, &source, &out, VideoPass::First { log: &log });
        encode(&profile, &source, &out, VideoPass::Second { log: &log });
        assert_eq!(probe_size(&out), Size::new(160, 120), "{encoder:?}");
    }
}
