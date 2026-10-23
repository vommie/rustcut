use std::ffi::OsString;
use std::fmt::Write as _;

use super::*;
use crate::edit::{Crop, DeshakeParams, Filter, FilterChain, Mirror, Rotation, SectionList};
use crate::job::sample_spec;
use crate::media::{AudioStream, Container as MediaContainer, Stream, VideoStream};
use crate::render::{RateControl, VideoEncoder};

fn stream(index: u32, kind: StreamKind) -> Stream {
    Stream {
        index,
        kind,
        codec: None,
        codec_long: None,
        profile: None,
        bit_rate: None,
        duration: None,
        start_time: None,
        language: None,
        title: None,
        default: false,
    }
}

/// A 60 s source of `width`x`height` turned by `rotation`, with `audio` tracks (the second one named and in German).
fn source(width: u32, height: u32, rotation: u16, audio: usize) -> MediaInfo {
    let mut streams = vec![stream(
        0,
        StreamKind::Video(VideoStream {
            width: Some(width),
            height: Some(height),
            rotation,
            ..VideoStream::default()
        }),
    )];
    for t in 0..audio {
        let mut s = stream(u32::try_from(t + 1).unwrap(), StreamKind::Audio(AudioStream::default()));
        if t == 1 {
            s.language = Some("deu".into());
            s.title = Some("Kommentar: Regie".into());
        }
        streams.push(s);
    }
    // A subtitle track: never mapped (ADR-8).
    streams.push(stream(9, StreamKind::Subtitle));
    MediaInfo {
        container: MediaContainer {
            duration: Some(Timestamp::from_secs(60)),
            ..MediaContainer::default()
        },
        streams,
        frame_timing: None,
    }
}

fn dirs() -> WorkDirs {
    WorkDirs {
        deshake: PathBuf::from("/data/rustcut/deshake"),
        passlog: PathBuf::from("/data/rustcut/passlog"),
    }
}

fn sections(ranges: &[(i64, i64)]) -> SectionList {
    ranges
        .iter()
        .map(|&(a, b)| Section::new(Timestamp::from_millis(a), Timestamp::from_millis(b)).unwrap())
        .collect::<Vec<_>>()
        .into()
}

fn build(spec: &JobSpec, source: &MediaInfo, hardware: &HardwareContext) -> Result<RenderPlan, PlanError> {
    plan(PlanInput {
        job: JobId(42),
        spec,
        source,
        dirs: &dirs(),
        hardware,
    })
}

fn text(args: &[OsString]) -> String {
    args.iter()
        .map(|a| {
            let a = a.to_string_lossy();
            if a.contains(' ') {
                format!("'{a}'")
            } else {
                a.into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe(plan: &RenderPlan) -> String {
    let mut out = format!(
        "target: {}\ntemp: {}\nexpected: {:?}\ntransforms: {:?}\npasslog: {:?}\n",
        plan.target.display(),
        plan.temp.display(),
        plan.expected,
        plan.transforms,
        plan.passlog
    );
    for pass in &plan.passes {
        let _ = writeln!(
            out,
            "{} ({}):\n  ffmpeg {}",
            pass.kind.label(),
            pass.duration.as_micros(),
            text(&pass.args)
        );
    }
    out
}

fn chain(filters: &[Filter]) -> FilterChain {
    let mut chain = FilterChain::default();
    for &f in filters {
        chain.set(f);
        chain.set_enabled(f.kind(), true);
    }
    chain
}

#[test]
fn one_section_one_audio_track() {
    let spec = sample_spec();
    let plan = build(&spec, &source(1920, 1080, 0, 1), &HardwareContext::default()).unwrap();
    assert_eq!(plan.pass_count(), 1);
    assert_eq!(plan.temp, PathBuf::from("/videos/out/.Clip - 01.rustcut-part.mkv"));
    insta::assert_snapshot!(describe(&plan));
}

#[test]
fn three_sections_two_tracks_with_filters() {
    let mut spec = sample_spec();
    spec.sections = sections(&[(1_000, 4_000), (30_500, 31_250), (10_000, 12_000)]);
    spec.filters = chain(&[
        Filter::Crop(Crop::new(10, 20, 30, 40)),
        Filter::Rotate {
            rotation: Rotation::Clockwise,
        },
        Filter::Mirror(Mirror {
            horizontal: true,
            vertical: false,
        }),
    ]);
    spec.target.stem = "Ärger: [cut], 'x'".into();
    let plan = build(&spec, &source(1920, 1080, 0, 2), &HardwareContext::default()).unwrap();
    assert_eq!(plan.expected.audio_streams, 2);
    assert_eq!(plan.expected.duration, Timestamp::from_millis(5_750));
    assert_eq!(plan.expected.size, Size::new(1040, 1860));
    insta::assert_snapshot!(describe(&plan));
}

#[test]
fn odd_rotated_source_is_evened_without_extra_rotation() {
    let mut spec = sample_spec();
    // Stored 479x853 turned by 90 degrees: the filters see 853x479, the guard crops it to 852x478.
    spec.filters = FilterChain::default();
    let plan = build(&spec, &source(479, 853, 90, 0), &HardwareContext::default()).unwrap();
    let args = text(&plan.passes[0].args);
    assert!(
        args.contains("[0:v:0]concat=n=1:v=1:a=0[vcat];[vcat]crop=w=852:h=478[vout]"),
        "{args}"
    );
    assert!(!args.contains("transpose"), "ffmpeg rotates the source itself: {args}");
    assert!(!args.contains("aout"));
    assert_eq!(plan.expected.size, Size::new(852, 478));
}

#[test]
fn deshake_and_two_pass_add_passes() {
    let mut spec = sample_spec();
    spec.filters = chain(&[
        Filter::Deshake(DeshakeParams::default()),
        Filter::Rotate {
            rotation: Rotation::UpsideDown,
        },
    ]);
    spec.profile.video.encoder = VideoEncoder::X264;
    spec.profile.video.preset = None;
    spec.profile.video.tune = None;
    spec.profile.video.codec_params.clear();
    spec.profile.video.film_grain = None;
    spec.profile.video.rate_control = RateControl::TwoPass { kbps: 3000 };
    let plan = build(&spec, &source(640, 360, 0, 1), &HardwareContext::default()).unwrap();
    let kinds: Vec<PassKind> = plan.passes.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        [PassKind::DeshakeAnalysis, PassKind::FirstVideoPass, PassKind::Encode]
    );
    assert_eq!(plan.transforms, Some(PathBuf::from("/data/rustcut/deshake/job-42.trf")));
    assert_eq!(plan.passlog, Some(PathBuf::from("/data/rustcut/passlog/job-42")));
    insta::assert_snapshot!(describe(&plan));
}

#[test]
fn vaapi_uploads_after_the_chain() {
    let mut spec = sample_spec();
    spec.profile.video.encoder = VideoEncoder::HevcVaapi;
    spec.profile.video.preset = None;
    spec.profile.video.tune = None;
    spec.profile.video.codec_params.clear();
    spec.profile.video.film_grain = None;
    spec.profile.video.rate_control = RateControl::Quality { value: 25 };
    spec.filters = chain(&[Filter::Mirror(Mirror {
        horizontal: false,
        vertical: true,
    })]);
    let src = source(1280, 720, 0, 1);
    assert_eq!(
        build(&spec, &src, &HardwareContext::default()),
        Err(PlanError::NoVaapiDevice("H.265 / HEVC (VAAPI)"))
    );
    let hardware = HardwareContext {
        vaapi_device: Some(PathBuf::from("/dev/dri/renderD128")),
    };
    let plan = build(&spec, &src, &hardware).unwrap();
    insta::assert_snapshot!(describe(&plan));
}

#[test]
fn sections_are_cut_at_the_end_of_the_source() {
    let mut spec = sample_spec();
    spec.sections = sections(&[(58_000, 70_000), (61_000, 65_000)]);
    let plan = build(&spec, &source(1920, 1080, 0, 1), &HardwareContext::default()).unwrap();
    assert_eq!(plan.expected.duration, Timestamp::from_secs(2));
    let args = text(&plan.passes[0].args);
    assert!(args.contains("-ss 58.000000 -to 60.000000 -i"), "{args}");
    assert_eq!(args.matches(" -i ").count(), 1, "the section after the end is dropped");

    spec.sections = sections(&[(61_000, 65_000)]);
    assert_eq!(
        build(&spec, &source(1920, 1080, 0, 1), &HardwareContext::default()),
        Err(PlanError::SectionsPastEnd("0:01:00.000".into()))
    );
}

#[test]
fn refusals() {
    let hw = HardwareContext::default();
    let src = source(1920, 1080, 0, 1);
    let mut spec = sample_spec();
    spec.sections = SectionList::new();
    assert_eq!(build(&spec, &src, &hw), Err(PlanError::NoSections));

    let mut spec = sample_spec();
    spec.source = spec.target_path();
    assert_eq!(build(&spec, &src, &hw), Err(PlanError::SameFile));

    let mut spec = sample_spec();
    spec.profile.audio.codec = AudioCodec::Copy;
    assert_eq!(build(&spec, &src, &hw), Err(PlanError::AudioCopy));
    assert!(
        build(&spec, &source(1920, 1080, 0, 0), &hw).is_ok(),
        "nothing to copy without audio"
    );

    let spec = sample_spec();
    let mut no_video = src.clone();
    no_video.streams.retain(|s| !matches!(s.kind, StreamKind::Video(_)));
    assert_eq!(build(&spec, &no_video, &hw), Err(PlanError::NoVideo));

    let mut spec = sample_spec();
    spec.filters = chain(&[Filter::Crop(Crop::new(600, 0, 600, 0))]);
    assert!(matches!(build(&spec, &src, &hw), Err(PlanError::Chain(_))));
}

#[test]
fn temp_names() {
    assert_eq!(
        temp_path(Path::new("/o"), "a.b", "mp4"),
        PathBuf::from("/o/.a.b.rustcut-part.mp4")
    );
    assert_eq!(target_file_name(&sample_spec()), "Clip - 01.mkv");
}
