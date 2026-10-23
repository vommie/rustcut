//! Render profiles: named, user-editable encoding presets; a job keeps a snapshot of its profile (ADR-14).

use serde::{Deserialize, Serialize};

use super::container::{CompatIssue, Severity};
use super::{AudioCodec, Container, TuneTarget, VideoEncoder};

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
}

/// How the encoder decides the bit rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RateControl {
    /// Constant quality on the encoder's scale (CRF / QP / CQ, see [`super::QualityScale`]).
    Quality { value: u16 },
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
    Container,
    RateControl,
    Preset,
    Tune,
    Profile,
    Level,
    BitDepth,
    Keyint,
    FilmGrain,
    CodecParams,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileIssue {
    pub field: ProfileField,
    pub severity: Severity,
    pub message: String,
}

impl RenderProfile {
    /// Everything that is wrong or questionable; an [`Severity::Error`] blocks rendering with this profile.
    pub fn validate(&self) -> Vec<ProfileIssue> {
        let mut issues = Vec::new();
        let mut push = |field, severity, message: String| {
            issues.push(ProfileIssue {
                field,
                severity,
                message,
            });
        };
        if self.name.trim().is_empty() {
            push(
                ProfileField::Name,
                Severity::Error,
                "the profile needs a name".to_owned(),
            );
        }
        let v = &self.video;
        let spec = v.encoder.spec();
        match v.rate_control {
            RateControl::Quality { value } => {
                let q = spec.quality;
                if !(q.min..=q.max).contains(&value) {
                    push(
                        ProfileField::RateControl,
                        Severity::Error,
                        format!("{} of {} must be {} to {}", q.label, spec.display_name, q.min, q.max),
                    );
                }
            }
            RateControl::AverageBitrate { kbps } | RateControl::TwoPass { kbps } if kbps == 0 => {
                push(
                    ProfileField::RateControl,
                    Severity::Error,
                    "the bit rate must be above 0".to_owned(),
                );
            }
            RateControl::TargetSize { mebibytes: 0 } => {
                push(
                    ProfileField::RateControl,
                    Severity::Error,
                    "the target size must be above 0".to_owned(),
                );
            }
            RateControl::AverageBitrate { .. } | RateControl::TwoPass { .. } | RateControl::TargetSize { .. } => {}
        }
        if matches!(
            v.rate_control,
            RateControl::TwoPass { .. } | RateControl::TargetSize { .. }
        ) && !spec.two_pass
        {
            push(
                ProfileField::RateControl,
                Severity::Error,
                format!("{} has no two-pass mode", spec.display_name),
            );
        }
        if let Some(preset) = &v.preset
            && !spec.has_preset(preset)
        {
            push(
                ProfileField::Preset,
                Severity::Error,
                format!("{} has no preset `{preset}`", spec.display_name),
            );
        }
        if let Some(tune) = &v.tune
            && !spec.has_tune(tune)
        {
            push(
                ProfileField::Tune,
                Severity::Error,
                format!("{} has no tune `{tune}`", spec.display_name),
            );
        }
        if let Some(profile) = &v.profile {
            if spec.profiles.is_empty() {
                push(
                    ProfileField::Profile,
                    Severity::Error,
                    format!("{} takes no profile", spec.display_name),
                );
            } else if !spec.profiles.contains(&profile.as_str()) {
                push(
                    ProfileField::Profile,
                    Severity::Error,
                    format!("{} has no profile `{profile}`", spec.display_name),
                );
            } else if v.bit_depth == BitDepth::Ten && !profile.ends_with("10") {
                push(
                    ProfileField::Profile,
                    Severity::Error,
                    format!("profile `{profile}` cannot encode 10-bit"),
                );
            } else if v.bit_depth == BitDepth::Eight && profile.ends_with("10") {
                push(
                    ProfileField::Profile,
                    Severity::Warning,
                    format!("profile `{profile}` is meant for 10-bit"),
                );
            }
        }
        if let Some(level) = &v.level
            && (level.is_empty() || !level.chars().all(|c| c.is_ascii_digit() || c == '.'))
        {
            push(
                ProfileField::Level,
                Severity::Error,
                format!("`{level}` is not a level (expected e.g. 4.1)"),
            );
        }
        if v.bit_depth == BitDepth::Ten && spec.pix_fmt_10.is_none() {
            push(
                ProfileField::BitDepth,
                Severity::Error,
                format!("{} cannot encode 10-bit", spec.display_name),
            );
        }
        if v.bit_depth == BitDepth::Ten && v.encoder == VideoEncoder::X264 {
            push(
                ProfileField::BitDepth,
                Severity::Warning,
                "10-bit H.264 does not play on many devices".to_owned(),
            );
        }
        if v.keyint == Some(0) {
            push(
                ProfileField::Keyint,
                Severity::Error,
                "the keyframe interval must be above 0".to_owned(),
            );
        }
        if let Some(grain) = v.film_grain {
            if !spec.film_grain {
                push(
                    ProfileField::FilmGrain,
                    Severity::Error,
                    format!("{} has no film grain synthesis", spec.display_name),
                );
            } else if grain > 50 {
                push(
                    ProfileField::FilmGrain,
                    Severity::Error,
                    "film grain must be 0 to 50".to_owned(),
                );
            }
        }
        for (i, p) in v.codec_params.iter().enumerate() {
            if spec.params_option.is_none() {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    format!("{} takes no codec parameters", spec.display_name),
                );
                break;
            }
            if p.key.is_empty() || !p.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    format!("`{}` is not a parameter name", p.key),
                );
            }
            if p.value.is_empty() || p.value.contains([':', '=']) || p.value.chars().any(char::is_whitespace) {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    format!("the value of `{}` must be non-empty without `:`, `=` or spaces", p.key),
                );
            }
            if v.codec_params.iter().skip(i + 1).any(|q| q.key == p.key) {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    format!("`{}` is set twice", p.key),
                );
            }
            if v.film_grain.is_some() && p.key == "film-grain" {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    "film grain is set twice (field and parameter)".to_owned(),
                );
            }
            if v.tune.is_some() && matches!(spec.tune_target, TuneTarget::CodecParam(key) if key == p.key) {
                push(
                    ProfileField::CodecParams,
                    Severity::Error,
                    format!("`{}` is set twice (tune and parameter)", p.key),
                );
            }
        }
        if let Some(rate) = self.audio.bitrate_kbps
            && (rate == 0 || !self.audio.codec.takes_bitrate())
        {
            let message = if rate == 0 {
                "the audio bit rate must be above 0".to_owned()
            } else {
                format!("{} takes no bit rate", self.audio.codec.spec().display_name)
            };
            push(ProfileField::Audio, Severity::Warning, message);
        }
        if self.audio.channels == Some(0) {
            push(
                ProfileField::Audio,
                Severity::Error,
                "the channel count must be above 0".to_owned(),
            );
        }
        for CompatIssue { severity, message } in self.container.check(v.encoder, self.audio.codec) {
            push(ProfileField::Container, severity, message);
        }
        issues
    }

    /// Whether [`RenderProfile::validate`] found no error (warnings allowed).
    pub fn is_usable(&self) -> bool {
        self.validate().iter().all(|i| i.severity != Severity::Error)
    }
}
