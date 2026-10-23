//! The entries of the encoder catalog.

use super::presets::{
    AOM_AQ, AOM_PRESETS, AOM_TUNES, NONE, NVENC_PRESETS, NVENC_TUNES, QSV_PRESETS, RAV1E_PRESETS, SVT_AQ, SVT_PRESETS,
    SVT_TUNES, X26X_PRESETS, X264_AQ, X264_TUNES, X265_AQ, X265_TUNES,
};
use super::{
    AqSpec, AudioCodec, CodecFamily, EncoderBackend, EncoderSpec, QualityScale, Setting, TuneTarget, VideoEncoder,
};

/// Speeds of x264, x265 and Quick Sync (fastest first).
const X26X_SPEEDS: [&str; 5] = ["veryfast", "fast", "medium", "slow", "veryslow"];

/// Keys that conflict with options RustCut always passes (rate control, preset, passes, pixel format). Keys of the
/// optional fields (tune, AQ mode, ...) are only refused when that field is set too (`validate::own_field`).
const X264_RESERVED: &[&str] = &["crf", "qp", "bitrate", "preset", "pass", "stats"];
const X265_RESERVED: &[&str] = &[
    "crf",
    "qp",
    "bitrate",
    "preset",
    "pass",
    "stats",
    "input-depth",
    "output-depth",
];

const fn q(
    option: &'static str,
    label: &'static str,
    max: u16,
    default: u16,
    extra: &'static [&'static str],
) -> QualityScale {
    QualityScale {
        option,
        label,
        min: 0,
        max,
        default,
        extra,
    }
}

const fn hw(
    encoder: VideoEncoder,
    ffmpeg_name: &'static str,
    display_name: &'static str,
    family: CodecFamily,
    backend: EncoderBackend,
    quality: QualityScale,
    simple_quality: (u16, u16),
) -> EncoderSpec {
    let (preset_option, presets, default_preset, tunes, speeds) = match backend {
        EncoderBackend::Nvenc => (
            Some("-preset"),
            NVENC_PRESETS,
            Some("p5"),
            NVENC_TUNES,
            Some(["p1", "p3", "p4", "p5", "p7"]),
        ),
        EncoderBackend::Qsv => (Some("-preset"), QSV_PRESETS, Some("medium"), NONE, Some(X26X_SPEEDS)),
        EncoderBackend::Vaapi | EncoderBackend::Software => (None, NONE, None, NONE, None),
    };
    let pix_fmt_10 = if matches!(family, CodecFamily::H264) {
        None
    } else {
        Some("p010le")
    };
    EncoderSpec {
        encoder,
        ffmpeg_name,
        display_name,
        family,
        backend,
        quality,
        preset_option,
        presets,
        default_preset,
        tunes,
        tune_target: TuneTarget::Option,
        profiles: &[],
        pix_fmt_8: "nv12",
        pix_fmt_10,
        params_option: None,
        two_pass: false,
        average_bitrate: true,
        film_grain: false,
        recommended_audio: AudioCodec::Opus,
        constant_qp: None,
        aq: None,
        scene_cuts: false,
        variance_boost: false,
        reserved_params: &[],
        simple_quality,
        speeds,
    }
}

pub(super) const SVT_AV1: EncoderSpec = EncoderSpec {
    encoder: VideoEncoder::SvtAv1,
    ffmpeg_name: "libsvtav1",
    display_name: "AV1 (SVT-AV1)",
    family: CodecFamily::Av1,
    backend: EncoderBackend::Software,
    quality: q("-crf", "CRF", 63, 26, &[]),
    preset_option: Some("-preset"),
    presets: SVT_PRESETS,
    default_preset: Some("6"),
    tunes: SVT_TUNES,
    tune_target: TuneTarget::CodecParam("tune"),
    profiles: &[],
    pix_fmt_8: "yuv420p",
    pix_fmt_10: Some("yuv420p10le"),
    params_option: Some("-svtav1-params"),
    two_pass: false,
    average_bitrate: true,
    film_grain: true,
    recommended_audio: AudioCodec::Opus,
    constant_qp: Some(q("-qp", "QP", 63, 30, &[])),
    aq: Some(AqSpec {
        setting: Setting::Param("aq-mode"),
        modes: SVT_AQ,
    }),
    scene_cuts: false,
    variance_boost: true,
    reserved_params: &["crf", "qp", "rc", "tbr", "preset", "pass", "stats", "input-depth"],
    simple_quality: (50, 16),
    speeds: Some(["10", "8", "6", "4", "2"]),
};

pub(super) const AOM: EncoderSpec = EncoderSpec {
    encoder: VideoEncoder::Aom,
    ffmpeg_name: "libaom-av1",
    display_name: "AV1 (libaom)",
    family: CodecFamily::Av1,
    backend: EncoderBackend::Software,
    quality: q("-crf", "CRF", 63, 30, &["-b:v", "0"]),
    preset_option: Some("-cpu-used"),
    presets: AOM_PRESETS,
    default_preset: Some("4"),
    tunes: AOM_TUNES,
    tune_target: TuneTarget::Option,
    profiles: &[],
    pix_fmt_8: "yuv420p",
    pix_fmt_10: Some("yuv420p10le"),
    params_option: Some("-aom-params"),
    two_pass: true,
    average_bitrate: true,
    film_grain: false,
    recommended_audio: AudioCodec::Opus,
    constant_qp: None,
    aq: Some(AqSpec {
        setting: Setting::Option("-aq-mode"),
        modes: AOM_AQ,
    }),
    scene_cuts: false,
    variance_boost: false,
    reserved_params: &["end-usage", "cq-level", "target-bitrate", "cpu-used", "passes", "pass"],
    simple_quality: (50, 16),
    speeds: Some(["8", "6", "4", "3", "2"]),
};

pub(super) const RAV1E: EncoderSpec = EncoderSpec {
    encoder: VideoEncoder::Rav1e,
    ffmpeg_name: "librav1e",
    display_name: "AV1 (rav1e)",
    family: CodecFamily::Av1,
    backend: EncoderBackend::Software,
    quality: q("-qp", "QP", 255, 100, &[]),
    preset_option: Some("-speed"),
    presets: RAV1E_PRESETS,
    default_preset: Some("6"),
    tunes: &[],
    tune_target: TuneTarget::Option,
    profiles: &[],
    pix_fmt_8: "yuv420p",
    pix_fmt_10: Some("yuv420p10le"),
    params_option: Some("-rav1e-params"),
    two_pass: false,
    average_bitrate: true,
    film_grain: false,
    recommended_audio: AudioCodec::Opus,
    constant_qp: None,
    aq: None,
    scene_cuts: false,
    variance_boost: false,
    reserved_params: &["quantizer", "bitrate", "speed"],
    simple_quality: (200, 40),
    speeds: Some(["10", "8", "6", "4", "2"]),
};

pub(super) const X265: EncoderSpec = EncoderSpec {
    encoder: VideoEncoder::X265,
    ffmpeg_name: "libx265",
    display_name: "H.265 / HEVC (x265)",
    family: CodecFamily::Hevc,
    backend: EncoderBackend::Software,
    quality: q("-crf", "CRF", 51, 22, &[]),
    preset_option: Some("-preset"),
    presets: X26X_PRESETS,
    default_preset: Some("medium"),
    tunes: X265_TUNES,
    tune_target: TuneTarget::Option,
    profiles: &["main", "main10"],
    pix_fmt_8: "yuv420p",
    pix_fmt_10: Some("yuv420p10le"),
    params_option: Some("-x265-params"),
    // libx265 takes two-pass through its own params (pass=1:stats=...), see `args`.
    two_pass: true,
    average_bitrate: true,
    film_grain: false,
    recommended_audio: AudioCodec::Aac,
    constant_qp: Some(q("-qp", "QP", 51, 26, &[])),
    aq: Some(AqSpec {
        setting: Setting::Param("aq-mode"),
        modes: X265_AQ,
    }),
    scene_cuts: true,
    variance_boost: false,
    reserved_params: X265_RESERVED,
    simple_quality: (36, 16),
    speeds: Some(X26X_SPEEDS),
};

pub(super) const X264: EncoderSpec = EncoderSpec {
    encoder: VideoEncoder::X264,
    ffmpeg_name: "libx264",
    display_name: "H.264 / AVC (x264)",
    family: CodecFamily::H264,
    backend: EncoderBackend::Software,
    quality: q("-crf", "CRF", 51, 20, &[]),
    preset_option: Some("-preset"),
    presets: X26X_PRESETS,
    default_preset: Some("medium"),
    tunes: X264_TUNES,
    tune_target: TuneTarget::Option,
    profiles: &["baseline", "main", "high", "high10"],
    pix_fmt_8: "yuv420p",
    pix_fmt_10: Some("yuv420p10le"),
    params_option: Some("-x264-params"),
    two_pass: true,
    average_bitrate: true,
    film_grain: false,
    recommended_audio: AudioCodec::Aac,
    constant_qp: Some(q("-qp", "QP", 69, 22, &[])),
    aq: Some(AqSpec {
        setting: Setting::Param("aq-mode"),
        modes: X264_AQ,
    }),
    scene_cuts: true,
    variance_boost: false,
    reserved_params: X264_RESERVED,
    simple_quality: (32, 14),
    speeds: Some(X26X_SPEEDS),
};

const VAAPI_Q: QualityScale = q("-qp", "QP", 52, 25, &["-rc_mode", "CQP"]);
const NVENC_Q: QualityScale = q("-cq", "CQ", 51, 28, &["-rc", "vbr", "-b:v", "0"]);
const QSV_Q: QualityScale = QualityScale {
    min: 1,
    ..q("-global_quality", "Quality", 51, 25, &[])
};

pub(super) const AV1_VAAPI: EncoderSpec = hw(
    VideoEncoder::Av1Vaapi,
    "av1_vaapi",
    "AV1 (VAAPI)",
    CodecFamily::Av1,
    EncoderBackend::Vaapi,
    q("-global_quality", "QP", 255, 120, &["-rc_mode", "CQP"]),
    (220, 60),
);
pub(super) const HEVC_VAAPI: EncoderSpec = hw(
    VideoEncoder::HevcVaapi,
    "hevc_vaapi",
    "H.265 / HEVC (VAAPI)",
    CodecFamily::Hevc,
    EncoderBackend::Vaapi,
    VAAPI_Q,
    (36, 16),
);
pub(super) const H264_VAAPI: EncoderSpec = hw(
    VideoEncoder::H264Vaapi,
    "h264_vaapi",
    "H.264 / AVC (VAAPI)",
    CodecFamily::H264,
    EncoderBackend::Vaapi,
    VAAPI_Q,
    (36, 16),
);
pub(super) const AV1_NVENC: EncoderSpec = hw(
    VideoEncoder::Av1Nvenc,
    "av1_nvenc",
    "AV1 (NVENC)",
    CodecFamily::Av1,
    EncoderBackend::Nvenc,
    QualityScale {
        max: 63,
        default: 32,
        ..NVENC_Q
    },
    (50, 20),
);
pub(super) const HEVC_NVENC: EncoderSpec = hw(
    VideoEncoder::HevcNvenc,
    "hevc_nvenc",
    "H.265 / HEVC (NVENC)",
    CodecFamily::Hevc,
    EncoderBackend::Nvenc,
    NVENC_Q,
    (38, 16),
);
pub(super) const H264_NVENC: EncoderSpec = hw(
    VideoEncoder::H264Nvenc,
    "h264_nvenc",
    "H.264 / AVC (NVENC)",
    CodecFamily::H264,
    EncoderBackend::Nvenc,
    NVENC_Q,
    (38, 16),
);
pub(super) const AV1_QSV: EncoderSpec = hw(
    VideoEncoder::Av1Qsv,
    "av1_qsv",
    "AV1 (Quick Sync)",
    CodecFamily::Av1,
    EncoderBackend::Qsv,
    QSV_Q,
    (36, 16),
);
pub(super) const HEVC_QSV: EncoderSpec = hw(
    VideoEncoder::HevcQsv,
    "hevc_qsv",
    "H.265 / HEVC (Quick Sync)",
    CodecFamily::Hevc,
    EncoderBackend::Qsv,
    QSV_Q,
    (36, 16),
);
pub(super) const H264_QSV: EncoderSpec = hw(
    VideoEncoder::H264Qsv,
    "h264_qsv",
    "H.264 / AVC (Quick Sync)",
    CodecFamily::H264,
    EncoderBackend::Qsv,
    QSV_Q,
    (36, 16),
);
