//! The option rows of the Render Settings panel and what a choice of a list row means.

use rustcut_core::render::{
    AudioCodec, BitDepth, ColorRange, ColorTags, Container, ProfileField, RateControl, Speed, VideoEncoder,
};

/// The rows of the panel (their keys are what Slint sends back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Row {
    Encoder,
    Quality,
    Speed,
    Container,
    AudioCodec,
    AudioBitrate,
    AudioChannels,
    AudioSampleRate,
    RateMode,
    RateValue,
    Preset,
    Tune,
    CodecProfile,
    Level,
    BitDepth,
    Keyint,
    FilmGrain,
    VarianceBoost,
    AqMode,
    SceneCuts,
    ColorTags,
    ColorRange,
    CodecParams,
}

impl Row {
    pub const ALL: [Self; 23] = [
        Self::Encoder,
        Self::Quality,
        Self::Speed,
        Self::Container,
        Self::AudioCodec,
        Self::AudioBitrate,
        Self::AudioChannels,
        Self::AudioSampleRate,
        Self::RateMode,
        Self::RateValue,
        Self::Preset,
        Self::Tune,
        Self::CodecProfile,
        Self::Level,
        Self::BitDepth,
        Self::Keyint,
        Self::FilmGrain,
        Self::VarianceBoost,
        Self::AqMode,
        Self::SceneCuts,
        Self::ColorTags,
        Self::ColorRange,
        Self::CodecParams,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Encoder => "encoder",
            Self::Quality => "quality",
            Self::Speed => "speed",
            Self::Container => "container",
            Self::AudioCodec => "audio-codec",
            Self::AudioBitrate => "audio-bitrate",
            Self::AudioChannels => "audio-channels",
            Self::AudioSampleRate => "audio-rate",
            Self::RateMode => "rate-mode",
            Self::RateValue => "rate-value",
            Self::Preset => "preset",
            Self::Tune => "tune",
            Self::CodecProfile => "codec-profile",
            Self::Level => "level",
            Self::BitDepth => "bit-depth",
            Self::Keyint => "keyint",
            Self::FilmGrain => "film-grain",
            Self::VarianceBoost => "variance-boost",
            Self::AqMode => "aq-mode",
            Self::SceneCuts => "scene-cuts",
            Self::ColorTags => "color-tags",
            Self::ColorRange => "color-range",
            Self::CodecParams => "codec-params",
        }
    }

    /// The card of the Codecs panel that shows the row (the split of `view::panel`).
    pub const fn card(self) -> &'static str {
        match self {
            Self::Encoder | Self::Quality | Self::Speed | Self::Container => "render-video",
            Self::AudioCodec | Self::AudioBitrate | Self::AudioChannels | Self::AudioSampleRate => "render-audio",
            _ => "render-expert",
        }
    }

    /// The first row that edits `field` (where a jump to an issue of the field leads).
    pub fn for_field(field: ProfileField) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.field() == Some(field))
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    /// The profile field whose issues mark this row.
    pub const fn field(self) -> Option<ProfileField> {
        Some(match self {
            Self::Encoder => ProfileField::Encoder,
            Self::Quality | Self::RateMode | Self::RateValue => ProfileField::RateControl,
            Self::Speed | Self::Preset => ProfileField::Preset,
            Self::Container => ProfileField::Container,
            Self::AudioCodec | Self::AudioBitrate | Self::AudioChannels | Self::AudioSampleRate => ProfileField::Audio,
            Self::Tune => ProfileField::Tune,
            Self::CodecProfile => ProfileField::Profile,
            Self::Level => ProfileField::Level,
            Self::BitDepth => ProfileField::BitDepth,
            Self::Keyint => ProfileField::Keyint,
            Self::FilmGrain => ProfileField::FilmGrain,
            Self::VarianceBoost => ProfileField::VarianceBoost,
            Self::AqMode => ProfileField::AqMode,
            Self::SceneCuts => ProfileField::SceneCuts,
            Self::ColorTags | Self::ColorRange => ProfileField::Color,
            Self::CodecParams => ProfileField::CodecParams,
        })
    }
}

/// The rate control modes of the expert view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateMode {
    Quality,
    ConstantQp,
    AverageBitrate,
    TwoPass,
    TargetSize,
}

impl RateMode {
    pub const fn of(rc: RateControl) -> Self {
        match rc {
            RateControl::Quality { .. } => Self::Quality,
            RateControl::ConstantQp { .. } => Self::ConstantQp,
            RateControl::AverageBitrate { .. } => Self::AverageBitrate,
            RateControl::TwoPass { .. } => Self::TwoPass,
            RateControl::TargetSize { .. } => Self::TargetSize,
        }
    }
}

/// What a choice of a list row means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Encoder(VideoEncoder),
    Speed(Speed),
    Container(Container),
    AudioCodec(AudioCodec),
    AudioChannels(Option<u8>),
    AudioSampleRate(Option<u32>),
    RateMode(RateMode),
    Preset(Option<String>),
    Tune(Option<String>),
    CodecProfile(Option<String>),
    BitDepth(BitDepth),
    VarianceBoost(Option<u8>),
    AqMode(Option<String>),
    ColorTags(ColorTags),
    ColorRange(ColorRange),
    /// The current value has no item of its own (a preset between the five speeds): choosing it changes nothing.
    Keep,
}
