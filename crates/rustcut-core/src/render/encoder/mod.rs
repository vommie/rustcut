//! The video encoder catalog: one table entry per ffmpeg encoder RustCut offers (ADR-14), like RustView's `FORMAT_SPECS`.
//! Hardware entries are offered only when the capability check finds them working (M2.4).

use serde::{Deserialize, Serialize};

use super::audio::AudioCodec;

mod presets;
mod specs;

/// The video encoders RustCut knows. Software first, then the hardware families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoEncoder {
    SvtAv1,
    Aom,
    Rav1e,
    X265,
    X264,
    Av1Vaapi,
    HevcVaapi,
    H264Vaapi,
    Av1Nvenc,
    HevcNvenc,
    H264Nvenc,
    Av1Qsv,
    HevcQsv,
    H264Qsv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodecFamily {
    Av1,
    Hevc,
    H264,
}

impl CodecFamily {
    /// The name the UI shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Av1 => "AV1",
            Self::Hevc => "HEVC",
            Self::H264 => "H.264",
        }
    }

    /// The block size the encoders of this codec code a picture in: another width or height is padded up to it
    /// (checked with ffmpeg's `trace_headers`: x264 codes 1918x1078 as 120x68 macroblocks of 16 and crops; x265 codes
    /// 1920x1080, its smallest coding block being 8; AV1's mode info covers 8x8 pixels).
    pub const fn block_multiple(self) -> u32 {
        match self {
            Self::H264 => 16,
            Self::Hevc | Self::Av1 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderBackend {
    Software,
    Vaapi,
    Nvenc,
    Qsv,
}

/// The constant-quality scale of an encoder (lower values = better quality, bigger files, for every entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityScale {
    /// ffmpeg option that takes the value (`-crf`, `-qp`, `-cq`, `-global_quality`).
    pub option: &'static str,
    /// Short name for the UI ("CRF", "QP", "CQ").
    pub label: &'static str,
    pub min: u16,
    pub max: u16,
    pub default: u16,
    /// Options that must accompany constant quality (`-b:v 0` for libaom and NVENC, `-rc_mode CQP` for VAAPI).
    pub extra: &'static [&'static str],
}

/// One speed preset, slowest first in every list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub value: &'static str,
    pub label: &'static str,
}

/// Where an option's value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Its own ffmpeg option (`-aq-mode <value>`).
    Option(&'static str),
    /// A key inside the codec params (`-x264-params aq-mode=<value>`).
    Param(&'static str),
}

/// The adaptive quantization modes of an encoder (how bits move between flat and detailed areas of a frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AqSpec {
    pub setting: Setting,
    /// The modes; the label of the encoder's default says so.
    pub modes: &'static [Preset],
}

/// Where the tune value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuneTarget {
    /// `-tune <value>`.
    Option,
    /// A key inside the codec params (`-svtav1-params tune=<value>`).
    CodecParam(&'static str),
}

/// Everything RustCut needs to know about one encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderSpec {
    pub encoder: VideoEncoder,
    pub ffmpeg_name: &'static str,
    pub display_name: &'static str,
    pub family: CodecFamily,
    pub backend: EncoderBackend,
    pub quality: QualityScale,
    /// ffmpeg option of the speed preset (`-preset`, `-cpu-used`, `-speed`); `None` when the encoder has none.
    pub preset_option: Option<&'static str>,
    pub presets: &'static [Preset],
    pub default_preset: Option<&'static str>,
    pub tunes: &'static [Preset],
    pub tune_target: TuneTarget,
    pub profiles: &'static [&'static str],
    /// Pixel formats for 8 and 10 bit (`None` = no 10-bit output). For VAAPI it is the software format that the command
    /// builder uploads with `hwupload` (M2.4), so it is not passed as `-pix_fmt`.
    pub pix_fmt_8: &'static str,
    pub pix_fmt_10: Option<&'static str>,
    /// Option for `key=value:...` passthrough parameters (`-svtav1-params`).
    pub params_option: Option<&'static str>,
    /// Whether ffmpeg's generic two-pass flags (`-pass`, `-passlogfile`) work with it.
    pub two_pass: bool,
    /// Whether the encoder supports average bit rate (`-b:v`).
    pub average_bitrate: bool,
    /// Whether it synthesizes film grain (SVT-AV1 `film-grain`).
    pub film_grain: bool,
    pub recommended_audio: AudioCodec,
    /// A constant quantizer (`-qp`) next to constant quality; `None` when the encoder has none or its quality scale
    /// already is a quantizer (rav1e, VAAPI).
    pub constant_qp: Option<QualityScale>,
    /// Adaptive quantization modes; `None` = not adjustable.
    pub aq: Option<AqSpec>,
    /// Whether keyframes at scene cuts can be switched off (`scenecut=0`, x264 and x265).
    pub scene_cuts: bool,
    /// Whether it has SVT-AV1's variance boost (more bits for low-contrast areas).
    pub variance_boost: bool,
    /// Codec parameter keys that conflict with options RustCut always passes; the passthrough refuses them.
    pub reserved_params: &'static [&'static str],
    /// The simple view's quality slider: the value at its "smallest file" end and at its "best quality" end (a useful
    /// part of the scale, chosen so the same slider position looks about the same with every encoder).
    pub simple_quality: (u16, u16),
    /// The simple view's speeds, fastest first (see [`super::Speed`]); `None` when the encoder has no presets.
    pub speeds: Option<[&'static str; 5]>,
}

impl VideoEncoder {
    pub const ALL: [Self; 14] = [
        Self::SvtAv1,
        Self::Aom,
        Self::Rav1e,
        Self::X265,
        Self::X264,
        Self::Av1Vaapi,
        Self::HevcVaapi,
        Self::H264Vaapi,
        Self::Av1Nvenc,
        Self::HevcNvenc,
        Self::H264Nvenc,
        Self::Av1Qsv,
        Self::HevcQsv,
        Self::H264Qsv,
    ];

    /// PyCutRv's default and RustCut's: SVT-AV1.
    pub const DEFAULT: Self = Self::SvtAv1;

    pub const fn spec(self) -> &'static EncoderSpec {
        match self {
            Self::SvtAv1 => &specs::SVT_AV1,
            Self::Aom => &specs::AOM,
            Self::Rav1e => &specs::RAV1E,
            Self::X265 => &specs::X265,
            Self::X264 => &specs::X264,
            Self::Av1Vaapi => &specs::AV1_VAAPI,
            Self::HevcVaapi => &specs::HEVC_VAAPI,
            Self::H264Vaapi => &specs::H264_VAAPI,
            Self::Av1Nvenc => &specs::AV1_NVENC,
            Self::HevcNvenc => &specs::HEVC_NVENC,
            Self::H264Nvenc => &specs::H264_NVENC,
            Self::Av1Qsv => &specs::AV1_QSV,
            Self::HevcQsv => &specs::HEVC_QSV,
            Self::H264Qsv => &specs::H264_QSV,
        }
    }

    /// The encoder with this ffmpeg name (`libsvtav1`), as listed by `ffmpeg -encoders`.
    pub fn from_ffmpeg_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.spec().ffmpeg_name == name)
    }
}

impl EncoderSpec {
    pub fn has_preset(&self, value: &str) -> bool {
        self.presets.iter().any(|p| p.value == value)
    }

    pub fn has_tune(&self, value: &str) -> bool {
        self.tunes.iter().any(|p| p.value == value)
    }

    /// The label of a speed preset value (`slow` -> "Slow").
    pub fn preset_label(&self, value: &str) -> Option<&'static str> {
        self.presets.iter().find(|p| p.value == value).map(|p| p.label)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_encoder_has_a_consistent_entry() {
        let mut names = HashSet::new();
        for encoder in VideoEncoder::ALL {
            let spec = encoder.spec();
            assert_eq!(spec.encoder, encoder);
            assert!(names.insert(spec.ffmpeg_name), "duplicate {}", spec.ffmpeg_name);
            assert_eq!(VideoEncoder::from_ffmpeg_name(spec.ffmpeg_name), Some(encoder));
            let q = spec.quality;
            assert!(q.min <= q.default && q.default <= q.max, "{}", spec.ffmpeg_name);
            assert_eq!(
                q.extra.len() % 2,
                0,
                "extra options come in pairs: {}",
                spec.ffmpeg_name
            );
            if let Some(default) = spec.default_preset {
                assert!(
                    spec.has_preset(default),
                    "{}: default preset {default}",
                    spec.ffmpeg_name
                );
            }
            assert_eq!(
                spec.presets.is_empty(),
                spec.preset_option.is_none(),
                "{}",
                spec.ffmpeg_name
            );
            assert_eq!(spec.film_grain, encoder == VideoEncoder::SvtAv1);
            assert_eq!(spec.variance_boost, encoder == VideoEncoder::SvtAv1);
            assert_eq!(spec.speeds.is_some(), !spec.presets.is_empty(), "{}", spec.ffmpeg_name);
            for preset in spec.speeds.iter().flatten() {
                assert!(spec.has_preset(preset), "{}: speed {preset}", spec.ffmpeg_name);
            }
            let (low, high) = spec.simple_quality;
            assert!(
                q.min <= high && high < low && low <= q.max,
                "{}: slider range",
                spec.ffmpeg_name
            );
            assert!(
                (high..=low).contains(&q.default),
                "{}: the default is on the slider",
                spec.ffmpeg_name
            );
            if let Some(qp) = spec.constant_qp {
                assert!(qp.min <= qp.default && qp.default <= qp.max, "{}", spec.ffmpeg_name);
            }
            if spec.scene_cuts || spec.variance_boost || !spec.reserved_params.is_empty() {
                assert!(spec.params_option.is_some(), "{}", spec.ffmpeg_name);
            }
            if let Some(AqSpec {
                setting: Setting::Param(_),
                ..
            }) = spec.aq
            {
                assert!(spec.params_option.is_some(), "{}", spec.ffmpeg_name);
            }
            assert_eq!(
                spec.backend == EncoderBackend::Software,
                encoder.spec().pix_fmt_8 == "yuv420p"
            );
        }
    }

    #[test]
    fn pycutrv_defaults_are_kept() {
        let svt = VideoEncoder::DEFAULT.spec();
        assert_eq!(
            (svt.ffmpeg_name, svt.quality.default, svt.default_preset),
            ("libsvtav1", 26, Some("6"))
        );
        assert_eq!(VideoEncoder::X265.spec().quality.default, 22);
        assert_eq!(VideoEncoder::X264.spec().quality.default, 20);
    }
}
