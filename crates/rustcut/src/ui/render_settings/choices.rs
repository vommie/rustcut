//! The items of the panel's list rows, which one is current, and what choosing one does to the working copy (with the
//! couplings a user expects).

use rustcut_core::render::{
    AudioCodec, BitDepth, ColorRange, ColorTags, Container, MAX_VARIANCE_BOOST, RateControl, RenderProfile, Speed,
    VideoEncoder,
};

use super::rows::{Pick, RateMode, Row};
use super::state::PanelState;

/// Default of a new average bit rate (kbit/s) and target size (MiB).
const DEFAULT_KBPS: u32 = 4_000;
const DEFAULT_MEBIBYTES: u32 = 100;

impl PanelState {
    /// The choices of a list row with what each means; `None` for rows that are not lists.
    pub fn choices(&self, row: Row) -> Option<Vec<(String, Pick)>> {
        let p = self.profile();
        let v = &p.video;
        let spec = v.encoder.spec();
        let none = |label: String| (label, None::<String>);
        let items = match row {
            Row::Encoder => self
                .offered_encoders()
                .into_iter()
                .map(|e| {
                    let mut label = e.spec().display_name.to_owned();
                    if e == v.encoder && self.availability_issue().is_some() {
                        label.push_str(" (not available here)");
                    }
                    (label, Pick::Encoder(e))
                })
                .collect(),
            Row::Speed => {
                let mut items: Vec<(String, Pick)> = Speed::ALL
                    .into_iter()
                    .map(|s| (s.label().to_owned(), Pick::Speed(s)))
                    .collect();
                if v.speed().is_none()
                    && let Some(preset) = v.effective_preset()
                {
                    let label = spec.preset_label(preset).unwrap_or(preset);
                    items.push((format!("Custom: {label}"), Pick::Keep));
                }
                items
            }
            Row::Container => Container::ALL
                .into_iter()
                .map(|c| (c.display_name().to_owned(), Pick::Container(c)))
                .collect(),
            Row::AudioCodec => AudioCodec::ALL
                .into_iter()
                .map(|c| (c.spec().display_name.to_owned(), Pick::AudioCodec(c)))
                .collect(),
            Row::AudioChannels => [
                ("As the source", None),
                ("Mono", Some(1)),
                ("Stereo", Some(2)),
                ("5.1", Some(6)),
            ]
            .into_iter()
            .map(|(l, c)| (l.to_owned(), Pick::AudioChannels(c)))
            .collect(),
            Row::AudioSampleRate => [
                ("As the source", None),
                ("44.1 kHz", Some(44_100)),
                ("48 kHz", Some(48_000)),
                ("96 kHz", Some(96_000)),
            ]
            .into_iter()
            .map(|(l, r)| (l.to_owned(), Pick::AudioSampleRate(r)))
            .collect(),
            Row::RateMode => {
                let mut items = vec![(format!("Constant quality ({})", spec.quality.label), RateMode::Quality)];
                if spec.constant_qp.is_some() {
                    items.push(("Constant QP".to_owned(), RateMode::ConstantQp));
                }
                if spec.average_bitrate {
                    items.push(("Average bit rate".to_owned(), RateMode::AverageBitrate));
                }
                if spec.two_pass {
                    items.push(("Bit rate, two passes".to_owned(), RateMode::TwoPass));
                    items.push(("Target size, two passes".to_owned(), RateMode::TargetSize));
                }
                items.into_iter().map(|(l, m)| (l, Pick::RateMode(m))).collect()
            }
            Row::Preset => {
                let default = spec
                    .default_preset
                    .and_then(|d| spec.preset_label(d))
                    .map_or_else(|| "Default".to_owned(), |l| format!("Default ({l})"));
                std::iter::once(none(default))
                    .chain(
                        spec.presets
                            .iter()
                            .map(|p| (p.label.to_owned(), Some(p.value.to_owned()))),
                    )
                    .map(|(l, v)| (l, Pick::Preset(v)))
                    .collect()
            }
            Row::Tune => std::iter::once(none("Default".to_owned()))
                .chain(
                    spec.tunes
                        .iter()
                        .map(|t| (t.label.to_owned(), Some(t.value.to_owned()))),
                )
                .map(|(l, v)| (l, Pick::Tune(v)))
                .collect(),
            Row::CodecProfile => std::iter::once(none("Auto".to_owned()))
                .chain(spec.profiles.iter().map(|p| ((*p).to_owned(), Some((*p).to_owned()))))
                .map(|(l, v)| (l, Pick::CodecProfile(v)))
                .collect(),
            Row::BitDepth => vec![
                ("8-bit".to_owned(), Pick::BitDepth(BitDepth::Eight)),
                ("10-bit".to_owned(), Pick::BitDepth(BitDepth::Ten)),
            ],
            Row::VarianceBoost => std::iter::once(("Encoder default".to_owned(), None))
                .chain(std::iter::once(("Off".to_owned(), Some(0))))
                .chain((1..=MAX_VARIANCE_BOOST).map(|s| (format!("Strength {s}"), Some(s))))
                .map(|(l, v)| (l, Pick::VarianceBoost(v)))
                .collect(),
            Row::AqMode => std::iter::once(none("Encoder default".to_owned()))
                .chain(
                    spec.aq
                        .iter()
                        .flat_map(|aq| aq.modes)
                        .map(|m| (m.label.to_owned(), Some(m.value.to_owned()))),
                )
                .map(|(l, v)| (l, Pick::AqMode(v)))
                .collect(),
            Row::ColorTags => ColorTags::ALL
                .into_iter()
                .map(|t| (t.label().to_owned(), Pick::ColorTags(t)))
                .collect(),
            Row::ColorRange => ColorRange::ALL
                .into_iter()
                .map(|r| (r.label().to_owned(), Pick::ColorRange(r)))
                .collect(),
            // Sliders and fields (the audio bit rate is a slider since P9.10, D-P9i).
            Row::Quality
            | Row::AudioBitrate
            | Row::RateValue
            | Row::Level
            | Row::Keyint
            | Row::FilmGrain
            | Row::SceneCuts
            | Row::CodecParams => return None,
        };
        Some(items)
    }

    /// The index of the current value in [`PanelState::choices`] (the "Custom" item, else the first).
    pub fn current_choice(&self, row: Row, items: &[(String, Pick)]) -> usize {
        let p = self.profile();
        let v = &p.video;
        let current = match row {
            Row::Encoder => Pick::Encoder(v.encoder),
            Row::Speed => v.speed().map_or(Pick::Keep, Pick::Speed),
            Row::Container => Pick::Container(p.container),
            Row::AudioCodec => Pick::AudioCodec(p.audio.codec),
            Row::AudioChannels => Pick::AudioChannels(p.audio.channels),
            Row::AudioSampleRate => Pick::AudioSampleRate(p.audio.sample_rate),
            Row::RateMode => Pick::RateMode(RateMode::of(v.rate_control)),
            Row::Preset => Pick::Preset(v.preset.clone()),
            Row::Tune => Pick::Tune(v.tune.clone()),
            Row::CodecProfile => Pick::CodecProfile(v.profile.clone()),
            Row::BitDepth => Pick::BitDepth(v.bit_depth),
            Row::VarianceBoost => Pick::VarianceBoost(v.variance_boost),
            Row::AqMode => Pick::AqMode(v.aq_mode.clone()),
            Row::ColorTags => Pick::ColorTags(v.color.tags),
            Row::ColorRange => Pick::ColorRange(v.color.range),
            Row::Quality
            | Row::AudioBitrate
            | Row::RateValue
            | Row::Level
            | Row::Keyint
            | Row::FilmGrain
            | Row::SceneCuts
            | Row::CodecParams => Pick::Keep,
        };
        items.iter().position(|(_, pick)| *pick == current).unwrap_or(0)
    }
}

/// Applies a choice to the working copy (with the couplings a user expects: a 10-bit codec profile switches to
/// 10-bit, 10-bit picks the codec's 10-bit profile, an audio codec drops what it does not take).
pub(super) fn apply(p: &mut RenderProfile, pick: Pick) {
    let v = &mut p.video;
    match pick {
        Pick::Encoder(e) => p.set_encoder(e),
        Pick::Speed(s) => p.set_speed(s),
        Pick::Container(c) => p.container = c,
        Pick::AudioCodec(c) => {
            p.audio.codec = c;
            if !c.spec().bitrates.contains(&p.audio.bitrate_kbps.unwrap_or(0)) {
                p.audio.bitrate_kbps = None;
            }
            if c == AudioCodec::Copy {
                p.audio.channels = None;
                p.audio.sample_rate = None;
            }
            if c == AudioCodec::Opus {
                p.audio.sample_rate = None;
            }
        }
        Pick::AudioChannels(c) => p.audio.channels = c,
        Pick::AudioSampleRate(r) => p.audio.sample_rate = r,
        Pick::RateMode(mode) => {
            let spec = v.encoder.spec();
            let kbps = match v.rate_control {
                RateControl::AverageBitrate { kbps } | RateControl::TwoPass { kbps } => kbps,
                RateControl::Quality { .. } | RateControl::ConstantQp { .. } | RateControl::TargetSize { .. } => {
                    DEFAULT_KBPS
                }
            };
            if mode != RateMode::of(v.rate_control) {
                v.rate_control = match mode {
                    RateMode::Quality => RateControl::Quality {
                        value: spec.quality.default,
                    },
                    RateMode::ConstantQp => RateControl::ConstantQp {
                        qp: spec.constant_qp.map_or(spec.quality.default, |q| q.default),
                    },
                    RateMode::AverageBitrate => RateControl::AverageBitrate { kbps },
                    RateMode::TwoPass => RateControl::TwoPass { kbps },
                    RateMode::TargetSize => RateControl::TargetSize {
                        mebibytes: DEFAULT_MEBIBYTES,
                    },
                };
            }
        }
        Pick::Preset(preset) => v.preset = preset,
        Pick::Tune(tune) => v.tune = tune,
        Pick::CodecProfile(profile) => {
            if let Some(name) = &profile {
                v.bit_depth = if name.ends_with("10") {
                    BitDepth::Ten
                } else {
                    BitDepth::Eight
                };
            }
            v.profile = profile;
        }
        Pick::BitDepth(depth) => {
            v.bit_depth = depth;
            v.profile = matching_profile(v.encoder, v.profile.take(), depth);
        }
        Pick::VarianceBoost(b) => v.variance_boost = b,
        Pick::AqMode(mode) => v.aq_mode = mode,
        Pick::ColorTags(t) => v.color.tags = t,
        Pick::ColorRange(r) => v.color.range = r,
        Pick::Keep => {}
    }
}

/// The codec profile that fits `depth`: x264 high <-> high10 (main and baseline become high10), x265 main <-> main10.
fn matching_profile(encoder: VideoEncoder, profile: Option<String>, depth: BitDepth) -> Option<String> {
    let profile = profile?;
    let ten = profile.ends_with("10");
    let fitting = match (encoder, depth, ten) {
        (VideoEncoder::X264, BitDepth::Ten, false) => "high10",
        (VideoEncoder::X264, BitDepth::Eight, true) => "high",
        (VideoEncoder::X265, BitDepth::Ten, false) => "main10",
        (VideoEncoder::X265, BitDepth::Eight, true) => "main",
        _ => return Some(profile),
    };
    Some(fitting.to_owned())
}
