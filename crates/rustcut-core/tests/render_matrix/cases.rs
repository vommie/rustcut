//! The cases of the render matrix (`tests/render_matrix.rs`): which source, sections, profile and filters each render
//! gets, and what it must give.

use rustcut_core::edit::{
    Crop, DeinterlaceRate, Deinterlacer, DeshakeParams, Filter, FilterChain, FilterKind, Mirror, ResizeAlgorithm,
    Rotation,
};
use rustcut_core::render::{
    AudioCodec, AudioSettings, BitDepth, Container, RateControl, RenderProfile, Severity, VideoEncoder, VideoSettings,
    builtin_profiles,
};

/// What a case must end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// A verified file whose streams, size and codecs match the job.
    Renders,
    /// An error before or while rendering (unreadable sources).
    Fails,
    /// Either; the report says which (a truncated file whose first seconds may still be readable).
    Either,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub group: &'static str,
    pub name: String,
    pub source: &'static str,
    /// Sections in milliseconds.
    pub sections: Vec<(i64, i64)>,
    pub profile: RenderProfile,
    pub filters: FilterChain,
    pub expect: Expect,
}

/// Every readable test video with a two-second section inside it (milliseconds).
pub const SOURCES: &[(&str, (i64, i64))] = &[
    ("basic_h264_720p.mp4", (1_000, 3_000)),
    ("hd_h264_1080p.mp4", (1_000, 3_000)),
    ("long_h264_320x180.mp4", (100_000, 102_000)),
    ("portrait_h264_720x1280.mp4", (1_000, 3_000)),
    ("odd_size_853x479_yuv444.mp4", (0, 2_000)),
    ("interlaced_pal_576i.mpg", (1_000, 3_000)),
    ("rotated_90_metadata.mp4", (1_000, 3_000)),
    ("shaky_h264_640x360.mp4", (2_000, 4_000)),
    ("no_audio_h264.mp4", (1_000, 3_000)),
    ("multi_audio_2tracks.mkv", (500, 2_500)),
    ("audio_ends_early_3s.mp4", (8_000, 10_000)),
    ("hevc_10bit.mkv", (1_000, 3_000)),
    ("vp9_opus.webm", (1_000, 3_000)),
    ("av1_svt.mkv", (1_000, 3_000)),
    ("prores_422_320x180.mov", (1_000, 3_000)),
    ("mpeg4_mp3.avi", (1_000, 3_000)),
    ("Ärger über Größe [test] (1).mp4", (1_000, 3_000)),
    ("dups/basic_h264_720p_copy.mp4", (1_000, 3_000)),
    ("with_md5/clip_with_sidecar.mp4", (1_000, 3_000)),
    ("nested/deep/in_a_subfolder.mp4", (1_000, 3_000)),
];

/// The sources every built-in profile renders in the standard matrix: landscape with AAC, portrait, two audio tracks,
/// a stored display rotation.
const PROFILE_SOURCES: &[&str] = &[
    "basic_h264_720p.mp4",
    "portrait_h264_720x1280.mp4",
    "multi_audio_2tracks.mkv",
    "rotated_90_metadata.mp4",
];

fn section_of(source: &str) -> (i64, i64) {
    SOURCES
        .iter()
        .find(|(name, _)| *name == source)
        .map_or((0, 2_000), |(_, section)| *section)
}

fn audio(codec: AudioCodec) -> AudioSettings {
    AudioSettings {
        codec,
        bitrate_kbps: None,
        channels: None,
        sample_rate: None,
    }
}

/// The fastest settings of `encoder`: its fastest speed preset, its default quality, 8 bit.
pub fn fastest(encoder: VideoEncoder, container: Container, audio_codec: AudioCodec) -> RenderProfile {
    let spec = encoder.spec();
    RenderProfile {
        id: format!("matrix-{}", spec.ffmpeg_name),
        name: format!("{} fastest", spec.display_name),
        container,
        video: VideoSettings {
            preset: spec.speeds.map(|s| s[0].to_owned()),
            bit_depth: BitDepth::Eight,
            ..VideoSettings::new(encoder)
        },
        audio: audio(audio_codec),
    }
}

/// The quick profile most cases use: x264 at its fastest with AAC in Matroska.
pub fn quick() -> RenderProfile {
    fastest(VideoEncoder::X264, Container::Mkv, AudioCodec::Aac)
}

fn chain(filters: &[Filter]) -> FilterChain {
    let mut chain = FilterChain::default();
    for filter in filters {
        chain.set(*filter);
        chain.set_enabled(filter.kind(), true);
    }
    chain
}

fn case(group: &'static str, name: impl Into<String>, source: &'static str, profile: RenderProfile) -> Case {
    Case {
        group,
        name: name.into(),
        source,
        sections: vec![section_of(source)],
        profile,
        filters: FilterChain::default(),
        expect: Expect::Renders,
    }
}

/// Every built-in profile on the profile sources (`full`: on every source).
fn profiles(full: bool) -> Vec<Case> {
    let sources: Vec<&'static str> = if full {
        SOURCES.iter().map(|(name, _)| *name).collect()
    } else {
        PROFILE_SOURCES.to_vec()
    };
    let mut cases = Vec::new();
    for profile in builtin_profiles() {
        for source in &sources {
            cases.push(case("profile", profile.name.clone(), source, profile.clone()));
        }
    }
    cases
}

/// Every software encoder at its fastest preset (the hardware ones are listed by the report when they work here).
fn encoders() -> Vec<Case> {
    [
        VideoEncoder::SvtAv1,
        VideoEncoder::Aom,
        VideoEncoder::Rav1e,
        VideoEncoder::X265,
        VideoEncoder::X264,
    ]
    .into_iter()
    .map(|encoder| {
        let profile = fastest(encoder, Container::Mkv, AudioCodec::Opus);
        case("encoder", profile.name.clone(), "basic_h264_720p.mp4", profile)
    })
    .collect()
}

/// Rate controls besides constant quality: two passes to a bit rate, constant QP, 10 bit.
fn rate_controls() -> Vec<Case> {
    let mut cases = Vec::new();
    for encoder in [VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::SvtAv1] {
        let spec = encoder.spec();
        if spec.two_pass {
            let mut profile = fastest(encoder, Container::Mkv, AudioCodec::Opus);
            profile.video.rate_control = RateControl::TwoPass { kbps: 900 };
            cases.push(case(
                "rate",
                format!("{} two-pass 900 kbit/s", spec.display_name),
                "shaky_h264_640x360.mp4",
                profile,
            ));
        }
        if let Some(qp) = spec.constant_qp {
            let mut profile = fastest(encoder, Container::Mkv, AudioCodec::Opus);
            profile.video.rate_control = RateControl::ConstantQp { qp: qp.default };
            cases.push(case(
                "rate",
                format!("{} constant QP {}", spec.display_name, qp.default),
                "basic_h264_720p.mp4",
                profile,
            ));
        }
        if spec.pix_fmt_10.is_some() {
            let mut profile = fastest(encoder, Container::Mkv, AudioCodec::Opus);
            profile.video.bit_depth = BitDepth::Ten;
            if encoder == VideoEncoder::X265 {
                profile.video.profile = Some("main10".to_owned());
            }
            cases.push(case(
                "rate",
                format!("{} 10-bit", spec.display_name),
                "hevc_10bit.mkv",
                profile,
            ));
        }
    }
    cases
}

/// Every container with every encoded audio codec the profile check lets through (an AV1 video in WebM, else x264;
/// copied audio has its own group). The combinations the check refuses are returned as refusals for the report.
fn containers() -> (Vec<Case>, Vec<String>) {
    let mut cases = Vec::new();
    let mut refused = Vec::new();
    for container in [Container::Mkv, Container::Mp4, Container::Webm, Container::Mov] {
        let encoder = if container == Container::Webm {
            VideoEncoder::SvtAv1
        } else {
            VideoEncoder::X264
        };
        for codec in [
            AudioCodec::Opus,
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Vorbis,
            AudioCodec::Flac,
        ] {
            let profile = fastest(encoder, container, codec);
            let name = format!("{container:?} + {} + {codec:?}", encoder.spec().display_name);
            match errors_of(&profile) {
                None => cases.push(case("container", name, "basic_h264_720p.mp4", profile)),
                Some(errors) => refused.push(format!("{name}: {errors}")),
            }
        }
    }
    (cases, refused)
}

/// Every video encoder family in every container (with the container's usual audio); the combinations the profile
/// check refuses are returned as refusals for the report.
fn video_in_containers() -> (Vec<Case>, Vec<String>) {
    let mut cases = Vec::new();
    let mut refused = Vec::new();
    for container in [Container::Mkv, Container::Mp4, Container::Mov, Container::Webm] {
        let audio = if container == Container::Webm {
            AudioCodec::Opus
        } else {
            AudioCodec::Aac
        };
        for encoder in [VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::SvtAv1] {
            let profile = fastest(encoder, container, audio);
            let name = format!("{container:?} + {}", encoder.spec().display_name);
            match errors_of(&profile) {
                None => cases.push(case("video in container", name, "basic_h264_720p.mp4", profile)),
                Some(errors) => refused.push(format!("{name}: {errors}")),
            }
        }
    }
    (cases, refused)
}

/// The profile check's errors in one line, or `None`.
fn errors_of(profile: &RenderProfile) -> Option<String> {
    let errors: Vec<String> = profile
        .validate()
        .into_iter()
        .filter(|issue| issue.severity == Severity::Error)
        .map(|issue| issue.message)
        .collect();
    (!errors.is_empty()).then(|| errors.join("; "))
}

/// Audio copied unchanged (Q-N6): every source audio codec into every container. A container that takes the codec
/// (`Container::takes_copied_audio`) must render it with the codec unchanged; the others must be refused before
/// ffmpeg runs. Copying needs one section: two sections must be refused.
fn audio_copies() -> Vec<Case> {
    let sources = [
        ("basic_h264_720p.mp4", "aac"),
        ("vp9_opus.webm", "opus"),
        ("mpeg4_mp3.avi", "mp3"),
        ("interlaced_pal_576i.mpg", "mp2"),
        ("prores_422_320x180.mov", "pcm_s16le"),
    ];
    let mut cases = Vec::new();
    for container in [Container::Mkv, Container::Mp4, Container::Webm, Container::Mov] {
        let encoder = if container == Container::Webm {
            VideoEncoder::SvtAv1
        } else {
            VideoEncoder::X264
        };
        for (source, codec) in sources {
            let profile = fastest(encoder, container, AudioCodec::Copy);
            let expect = if container.takes_copied_audio(codec) {
                Expect::Renders
            } else {
                Expect::Fails
            };
            cases.push(Case {
                expect,
                ..case("audio copy", format!("{codec} into {container:?}"), source, profile)
            });
        }
    }
    let profile = fastest(VideoEncoder::X264, Container::Mkv, AudioCodec::Copy);
    cases.push(Case {
        expect: Expect::Fails,
        sections: vec![(1_000, 2_000), (4_000, 5_000)],
        ..case("audio copy", "two sections", "basic_h264_720p.mp4", profile)
    });
    cases
}

/// Every filter (and every variant of it) on a source that shows what it does.
fn filters() -> Vec<Case> {
    let mut list: Vec<(String, &'static str, Vec<Filter>)> = Vec::new();
    for deinterlacer in Deinterlacer::ALL {
        for rate in [DeinterlaceRate::Frame, DeinterlaceRate::Field] {
            if rate == DeinterlaceRate::Field && !deinterlacer.supports_field_rate() {
                continue;
            }
            list.push((
                format!("deinterlace {deinterlacer:?} {rate:?}"),
                "interlaced_pal_576i.mpg",
                vec![Filter::Deinterlace { deinterlacer, rate }],
            ));
        }
    }
    list.push((
        "crop".into(),
        "hd_h264_1080p.mp4",
        vec![Filter::Crop(Crop::new(100, 40, 60, 200))],
    ));
    list.push((
        "crop of a rotated source".into(),
        "rotated_90_metadata.mp4",
        vec![Filter::Crop(Crop::new(100, 20, 41, 0))],
    ));
    list.push((
        "deshake".into(),
        "shaky_h264_640x360.mp4",
        vec![Filter::Deshake(DeshakeParams::default())],
    ));
    for rotation in [Rotation::Clockwise, Rotation::CounterClockwise, Rotation::UpsideDown] {
        list.push((
            format!("rotate {rotation:?}"),
            "odd_size_853x479_yuv444.mp4",
            vec![Filter::Rotate { rotation }],
        ));
    }
    for (horizontal, vertical) in [(true, false), (false, true), (true, true)] {
        list.push((
            format!("mirror h={horizontal} v={vertical}"),
            "basic_h264_720p.mp4",
            vec![Filter::Mirror(Mirror { horizontal, vertical })],
        ));
    }
    for algorithm in [
        ResizeAlgorithm::Bilinear,
        ResizeAlgorithm::Bicubic,
        ResizeAlgorithm::Lanczos,
        ResizeAlgorithm::Spline,
    ] {
        list.push((
            format!("resize to width 640 {algorithm:?}"),
            "hd_h264_1080p.mp4",
            vec![Filter::Resize {
                width: Some(640),
                height: None,
                algorithm,
            }],
        ));
    }
    list.push((
        "resize to height 360".into(),
        "portrait_h264_720x1280.mp4",
        vec![Filter::Resize {
            width: None,
            height: Some(360),
            algorithm: ResizeAlgorithm::default(),
        }],
    ));
    list.push((
        "resize to 320x240".into(),
        "basic_h264_720p.mp4",
        vec![Filter::Resize {
            width: Some(320),
            height: Some(240),
            algorithm: ResizeAlgorithm::default(),
        }],
    ));
    list.push((
        "every filter at once".into(),
        "rotated_90_metadata.mp4",
        vec![
            Filter::Deinterlace {
                deinterlacer: Deinterlacer::Bwdif,
                rate: DeinterlaceRate::Frame,
            },
            Filter::Crop(Crop::new(40, 10, 40, 10)),
            Filter::Deshake(DeshakeParams::default()),
            Filter::Rotate {
                rotation: Rotation::Clockwise,
            },
            Filter::Mirror(Mirror {
                horizontal: true,
                vertical: false,
            }),
            Filter::Resize {
                width: Some(480),
                height: None,
                algorithm: ResizeAlgorithm::Lanczos,
            },
        ],
    ));
    list.into_iter()
        .map(|(name, source, filters)| Case {
            filters: chain(&filters),
            ..case("filter", name, source, quick())
        })
        .collect()
}

/// Every test video with two sections (one when it is short), and the unreadable ones.
fn sources() -> Vec<Case> {
    let mut cases: Vec<Case> = SOURCES
        .iter()
        .map(|(source, (a, b))| Case {
            sections: vec![(*a, *b), (b + 1_000, b + 2_000)],
            ..case("source", *source, source, quick())
        })
        .collect();
    for (source, expect) in [
        ("broken/empty.mp4", Expect::Fails),
        ("broken/not_a_video.mp4", Expect::Fails),
        ("broken/truncated.mp4", Expect::Either),
    ] {
        cases.push(Case {
            expect,
            sections: vec![(0, 1_000)],
            ..case("source", source, source, quick())
        });
    }
    cases
}

/// The filters of the standard matrix with every built-in profile (`full` only).
fn filters_with_profiles() -> Vec<Case> {
    let mut cases = Vec::new();
    for profile in builtin_profiles() {
        for base in filters() {
            cases.push(Case {
                group: "filter x profile",
                name: format!("{} / {}", base.name, profile.name),
                profile: profile.clone(),
                ..base
            });
        }
    }
    cases
}

/// The cases of a run and the container combinations the profile check refused.
pub fn all(full: bool) -> (Vec<Case>, Vec<String>) {
    let (container_cases, mut refused) = containers();
    let (video_cases, video_refused) = video_in_containers();
    refused.extend(video_refused);
    let mut cases = profiles(full);
    cases.extend(encoders());
    cases.extend(rate_controls());
    cases.extend(container_cases);
    cases.extend(video_cases);
    cases.extend(audio_copies());
    cases.extend(filters());
    cases.extend(sources());
    if full {
        cases.extend(filters_with_profiles());
    }
    (cases, refused)
}

/// The enabled filters of `chain`, for the report.
pub fn filter_names(chain: &FilterChain) -> String {
    let names: Vec<String> = [
        FilterKind::Deinterlace,
        FilterKind::Crop,
        FilterKind::Deshake,
        FilterKind::Rotate,
        FilterKind::Mirror,
        FilterKind::Resize,
    ]
    .into_iter()
    .filter(|kind| chain.is_enabled(*kind))
    .map(|kind| format!("{kind:?}").to_lowercase())
    .collect();
    if names.is_empty() { "-".into() } else { names.join(", ") }
}
