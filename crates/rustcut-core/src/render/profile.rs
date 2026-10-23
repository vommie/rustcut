//! Render profiles: named, user-editable encoding presets; a job keeps a snapshot of its profile (ADR-14).

use serde::{Deserialize, Serialize};

use super::color::ColorSettings;
use super::container::Severity;
use super::{AudioCodec, Container, VideoEncoder};

/// A complete set of encoding settings under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderProfile {
    /// Stable id (`av1-balanced`); built-in ids are reserved (see [`super::builtin`]).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub container: Container,
    pub video: VideoSettings,
    #[serde(default)]
    pub audio: AudioSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoSettings {
    pub encoder: VideoEncoder,
    pub rate_control: RateControl,
    /// Speed preset value of the encoder's list; `None` = the encoder's default.
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub tune: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    /// Codec level (`4.1`); `None` = chosen by the encoder.
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub bit_depth: BitDepth,
    /// Maximum distance between keyframes in frames (`-g`); `None` = the encoder's default.
    #[serde(default)]
    pub keyint: Option<u32>,
    /// SVT-AV1 film grain synthesis strength, 0 to 50.
    #[serde(default)]
    pub film_grain: Option<u8>,
    /// Extra `key=value` parameters for the encoder's params option (`-svtav1-params`, `-x265-params`, ...).
    #[serde(default)]
    pub codec_params: Vec<CodecParam>,
    /// Adaptive quantization mode (a value of the encoder's list); `None` = the encoder's default.
    #[serde(default)]
    pub aq_mode: Option<String>,
    /// `Some(false)` = no extra keyframes at scene cuts (x264/x265 `scenecut=0`); `None` = the encoder's default (on).
    #[serde(default)]
    pub scene_cuts: Option<bool>,
    /// SVT-AV1 variance boost: `Some(0)` = off, 1 to 4 = its strength; `None` = the encoder's default.
    #[serde(default)]
    pub variance_boost: Option<u8>,
    /// Color tags of the output; the default passes the source's on.
    #[serde(default)]
    pub color: ColorSettings,
}

impl VideoSettings {
    /// `encoder` at its default quality and preset, everything else left to the encoder.
    pub fn new(encoder: VideoEncoder) -> Self {
        Self {
            encoder,
            rate_control: RateControl::Quality {
                value: encoder.spec().quality.default,
            },
            preset: None,
            tune: None,
            profile: None,
            level: None,
            bit_depth: BitDepth::Eight,
            keyint: None,
            film_grain: None,
            codec_params: Vec::new(),
            aq_mode: None,
            scene_cuts: None,
            variance_boost: None,
            color: ColorSettings::default(),
        }
    }
}

/// How the encoder decides the bit rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RateControl {
    /// Constant quality on the encoder's scale (CRF / QP / CQ, see [`super::QualityScale`]).
    Quality { value: u16 },
    /// A constant quantizer for every frame (`-qp`): predictable, but larger than constant quality at the same look.
    ConstantQp { qp: u16 },
    /// Average bit rate in kbit/s, one pass.
    AverageBitrate { kbps: u32 },
    /// Average bit rate in kbit/s, two passes (better distribution, twice the time).
    TwoPass { kbps: u32 },
    /// A target file size in MiB, two passes; the bit rate follows from the output duration.
    TargetSize { mebibytes: u32 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    #[default]
    Eight,
    /// Less banding at the same size; not every player decodes 10-bit H.264.
    Ten,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CodecParam {
    pub key: String,
    pub value: String,
}

impl CodecParam {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub codec: AudioCodec,
    /// kbit/s per stream; `None` = the codec's default (ignored by FLAC and copy).
    pub bitrate_kbps: Option<u32>,
    /// Downmix to this many channels (`-ac`); `None` keeps the source layout.
    pub channels: Option<u8>,
    /// Resample (`-ar`); `None` keeps the source rate (Opus always uses 48 kHz).
    pub sample_rate: Option<u32>,
}

/// Which part of a profile an issue is about (the editor highlights that field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileField {
    Name,
    /// The encoder itself (not available here).
    Encoder,
    Container,
    RateControl,
    Preset,
    Tune,
    Profile,
    Level,
    BitDepth,
    Keyint,
    FilmGrain,
    VarianceBoost,
    AqMode,
    SceneCuts,
    Color,
    CodecParams,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProfileIssue {
    pub field: ProfileField,
    pub severity: Severity,
    pub message: String,
}

impl ProfileIssue {
    pub fn error(field: ProfileField, message: impl Into<String>) -> Self {
        Self {
            field,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub fn warning(field: ProfileField, message: impl Into<String>) -> Self {
        Self {
            field,
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

impl RenderProfile {
    /// Everything that is wrong or questionable; an [`Severity::Error`] blocks rendering with this profile.
    pub fn validate(&self) -> Vec<ProfileIssue> {
        super::validate::validate(self)
    }

    /// Whether [`RenderProfile::validate`] found no error (warnings allowed).
    pub fn is_usable(&self) -> bool {
        self.validate().iter().all(|i| i.severity != Severity::Error)
    }

    /// Whether the encoding settings (container, video, audio) are the same, whatever the names.
    pub fn same_settings(&self, other: &Self) -> bool {
        self.container == other.container && self.video == other.video && self.audio == other.audio
    }
}
