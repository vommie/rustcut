//! The simple view over a render profile (ADR-14, user decisions Q-E2/Q-E3): a codec-neutral quality slider and five
//! codec-neutral speeds, both mapped onto the encoder's own scale and presets. Changing the encoder keeps the slider
//! position and the speed, so the result looks and takes about the same; options the new encoder does not have are
//! dropped.

use super::{BitDepth, CodecFamily, EncoderSpec, RateControl, RenderProfile, VideoEncoder, VideoSettings};

/// The five speeds of the simple view, fastest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speed {
    Fastest,
    Fast,
    Balanced,
    Slow,
    Slowest,
}

impl Speed {
    pub const ALL: [Self; 5] = [Self::Fastest, Self::Fast, Self::Balanced, Self::Slow, Self::Slowest];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Fastest => "Fastest (largest files)",
            Self::Fast => "Fast",
            Self::Balanced => "Balanced",
            Self::Slow => "Slow (smaller files)",
            Self::Slowest => "Slowest (smallest files)",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Fastest => 0,
            Self::Fast => 1,
            Self::Balanced => 2,
            Self::Slow => 3,
            Self::Slowest => 4,
        }
    }
}

/// The named parts of the quality slider (codec-neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QualityBand {
    /// Visible loss; for sharing small files.
    Small,
    Good,
    /// Hard to tell from the source for most material (the encoders' defaults are here).
    High,
    VeryHigh,
    /// Near the source; big files.
    Archive,
}

impl QualityBand {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Good => "Good",
            Self::High => "High",
            Self::VeryHigh => "Very high",
            Self::Archive => "Archive",
        }
    }

    /// The band of a slider fraction (0 = smallest file, 1 = best quality).
    pub fn of(fraction: f64) -> Self {
        match fraction {
            f if f < 0.30 => Self::Small,
            f if f < 0.55 => Self::Good,
            f if f < 0.80 => Self::High,
            f if f < 0.95 => Self::VeryHigh,
            _ => Self::Archive,
        }
    }
}

/// What the quality slider shows for a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualitySlider {
    /// Number of steps; position 0 = smallest file, `steps` = best quality.
    pub steps: u16,
    pub position: u16,
    /// The value is on the slider's part of the scale (an expert value outside it pins the slider to an end).
    pub inside: bool,
    pub band: QualityBand,
}

impl EncoderSpec {
    /// The slider for a constant quality `value`.
    pub fn quality_slider(&self, value: u16) -> QualitySlider {
        let (low, high) = self.simple_quality;
        let steps = low.saturating_sub(high).max(1);
        let clamped = value.clamp(high, low);
        let position = low.saturating_sub(clamped);
        QualitySlider {
            steps,
            position,
            inside: clamped == value,
            band: QualityBand::of(f64::from(position) / f64::from(steps)),
        }
    }

    /// The constant quality value at slider `position` (clamped to the slider).
    pub fn quality_at(&self, position: u16) -> u16 {
        let (low, high) = self.simple_quality;
        low.saturating_sub(position).max(high)
    }

    /// The value at slider fraction `fraction` (0 = smallest file, 1 = best quality), rounded to the nearest step.
    pub fn quality_at_fraction(&self, fraction: f64) -> u16 {
        let (low, high) = self.simple_quality;
        let steps = f64::from(low.saturating_sub(high));
        let offset = (fraction.clamp(0.0, 1.0) * steps).round();
        // `offset` is between 0 and `steps`, which came from a u16.
        let offset = az::checked_cast::<f64, u16>(offset).unwrap_or(0);
        low.saturating_sub(offset).max(high)
    }

    /// The preset of `speed`; `None` when the encoder has no presets.
    pub fn speed_preset(&self, speed: Speed) -> Option<&'static str> {
        self.speeds.and_then(|s| s.get(speed.index()).copied())
    }

    /// The speed a preset value stands for (`None` = a preset between the five, or no presets).
    pub fn speed_of(&self, preset: &str) -> Option<Speed> {
        Speed::ALL.into_iter().find(|s| self.speed_preset(*s) == Some(preset))
    }
}

impl VideoSettings {
    /// The preset in effect (the chosen one or the encoder's default).
    pub fn effective_preset(&self) -> Option<&str> {
        self.preset.as_deref().or(self.encoder.spec().default_preset)
    }

    /// The simple view's speed; `None` = a preset between the five (shown as "Custom"), or no presets.
    pub fn speed(&self) -> Option<Speed> {
        self.effective_preset().and_then(|p| self.encoder.spec().speed_of(p))
    }

    /// The slider position of the constant quality, or `None` in the other rate control modes.
    pub fn quality_slider(&self) -> Option<QualitySlider> {
        match self.rate_control {
            RateControl::Quality { value } => Some(self.encoder.spec().quality_slider(value)),
            RateControl::ConstantQp { .. }
            | RateControl::AverageBitrate { .. }
            | RateControl::TwoPass { .. }
            | RateControl::TargetSize { .. } => None,
        }
    }
}

impl RenderProfile {
    /// Moves the quality slider: constant quality at `position` (other rate control modes switch to it).
    pub fn set_quality_position(&mut self, position: u16) {
        let value = self.video.encoder.spec().quality_at(position);
        self.video.rate_control = RateControl::Quality { value };
    }

    /// Picks one of the five speeds (a no-op for encoders without presets).
    pub fn set_speed(&mut self, speed: Speed) {
        if let Some(preset) = self.video.encoder.spec().speed_preset(speed) {
            self.video.preset = Some(preset.to_owned());
        }
    }

    /// Switches to `encoder` keeping what carries over: the slider position, the speed, a bit rate or target size, the
    /// bit depth, GOP and color tags. Options only the old encoder had (tune, profile, level, film grain, variance
    /// boost, AQ mode, scene cuts, codec parameters) are dropped; a 10-bit choice falls back to 8-bit when the new
    /// encoder has none, and for H.264 (10-bit H.264 plays on few devices; it stays available in the expert view).
    pub fn set_encoder(&mut self, encoder: VideoEncoder) {
        let old = &self.video;
        if old.encoder == encoder {
            return;
        }
        let old_spec = old.encoder.spec();
        let spec = encoder.spec();
        let quality_fraction = |value: u16| {
            let slider = old_spec.quality_slider(value);
            f64::from(slider.position) / f64::from(slider.steps)
        };
        let rate_control = match old.rate_control {
            RateControl::Quality { value } => RateControl::Quality {
                value: spec.quality_at_fraction(quality_fraction(value)),
            },
            RateControl::ConstantQp { qp } => match (old_spec.constant_qp, spec.constant_qp) {
                (Some(from), Some(to)) => RateControl::ConstantQp {
                    qp: rescale(qp, from.max, to.max),
                },
                _ => RateControl::Quality {
                    value: spec.quality.default,
                },
            },
            RateControl::TwoPass { kbps } if !spec.two_pass => RateControl::AverageBitrate { kbps },
            RateControl::TargetSize { .. } if !spec.two_pass => RateControl::Quality {
                value: spec.quality.default,
            },
            other @ (RateControl::AverageBitrate { .. }
            | RateControl::TwoPass { .. }
            | RateControl::TargetSize { .. }) => other,
        };
        let speed = old.speed();
        let bit_depth = if spec.pix_fmt_10.is_some() && spec.family != CodecFamily::H264 {
            old.bit_depth
        } else {
            BitDepth::Eight
        };
        let mut video = VideoSettings {
            rate_control,
            bit_depth,
            keyint: old.keyint,
            color: old.color,
            ..VideoSettings::new(encoder)
        };
        if let Some(preset) = speed.and_then(|s| spec.speed_preset(s)) {
            video.preset = Some(preset.to_owned());
        }
        self.video = video;
    }
}

/// `value` on a scale of `0..=from_max` moved to `0..=to_max`.
fn rescale(value: u16, from_max: u16, to_max: u16) -> u16 {
    let scaled = u32::from(value.min(from_max)) * u32::from(to_max) / u32::from(from_max.max(1));
    u16::try_from(scaled).unwrap_or(to_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{CodecParam, builtin_profiles};

    #[test]
    fn the_slider_maps_onto_each_scale() {
        let x264 = VideoEncoder::X264.spec();
        assert_eq!(x264.simple_quality, (32, 14));
        let s = x264.quality_slider(20);
        assert_eq!(
            (s.steps, s.position, s.inside, s.band),
            (18, 12, true, QualityBand::High)
        );
        assert_eq!(x264.quality_at(12), 20);
        assert_eq!(x264.quality_at(500), 14, "clamped to the best end");
        let outside = x264.quality_slider(40);
        assert_eq!(
            (outside.position, outside.inside, outside.band),
            (0, false, QualityBand::Small)
        );
        assert_eq!(x264.quality_slider(0).position, 18);
    }

    #[test]
    fn every_default_is_in_a_middle_band() {
        for encoder in VideoEncoder::ALL {
            let spec = encoder.spec();
            let band = spec.quality_slider(spec.quality.default).band;
            assert!(
                matches!(band, QualityBand::Good | QualityBand::High),
                "{}: {band:?}",
                spec.ffmpeg_name
            );
        }
    }

    #[test]
    fn speeds_map_both_ways() {
        let x265 = VideoEncoder::X265.spec();
        assert_eq!(x265.speed_preset(Speed::Slowest), Some("veryslow"));
        assert_eq!(x265.speed_of("medium"), Some(Speed::Balanced));
        assert_eq!(x265.speed_of("slower"), None, "between two speeds: custom");
        assert_eq!(VideoEncoder::HevcVaapi.spec().speed_preset(Speed::Fast), None);
        let v = VideoSettings::new(VideoEncoder::SvtAv1);
        assert_eq!(v.speed(), Some(Speed::Balanced), "the default preset 6 is balanced");
    }

    #[test]
    fn switching_the_encoder_keeps_slider_and_speed() {
        let mut p = builtin_profiles().remove(0);
        p.video.codec_params = vec![CodecParam::new("enable-overlays", "1")];
        p.video.aq_mode = Some("1".into());
        p.video.keyint = Some(240);
        let before = p.video.quality_slider().unwrap();
        p.set_encoder(VideoEncoder::X264);
        let v = &p.video;
        assert_eq!(v.encoder, VideoEncoder::X264);
        let after = v.quality_slider().unwrap();
        let fraction = |s: QualitySlider| f64::from(s.position) / f64::from(s.steps);
        assert!(
            (fraction(before) - fraction(after)).abs() < 0.04,
            "{before:?} {after:?}"
        );
        assert_eq!(v.speed(), Some(Speed::Balanced));
        assert_eq!(v.bit_depth, BitDepth::Eight, "10-bit H.264 plays on few devices");
        assert_eq!(v.keyint, Some(240));
        assert!(v.codec_params.is_empty() && v.aq_mode.is_none() && v.tune.is_none() && v.film_grain.is_none());
        assert!(p.is_usable(), "{:?}", p.validate());

        p.video.rate_control = RateControl::TwoPass { kbps: 3000 };
        p.set_encoder(VideoEncoder::SvtAv1);
        assert_eq!(p.video.rate_control, RateControl::AverageBitrate { kbps: 3000 });
        p.video.rate_control = RateControl::ConstantQp { qp: 63 };
        p.video.bit_depth = BitDepth::Ten;
        p.set_encoder(VideoEncoder::X265);
        assert_eq!(p.video.rate_control, RateControl::ConstantQp { qp: 51 });
        assert_eq!(p.video.bit_depth, BitDepth::Ten, "x265 keeps 10-bit");
        p.set_encoder(VideoEncoder::H264Vaapi);
        assert_eq!(p.video.bit_depth, BitDepth::Eight, "no 10-bit H.264 in VAAPI");
        assert_eq!(p.video.preset, None);
        assert_eq!(
            p.video.rate_control,
            RateControl::Quality {
                value: VideoEncoder::H264Vaapi.spec().quality.default
            }
        );
    }

    #[test]
    fn slider_and_speed_edits() {
        let mut p = builtin_profiles().remove(3);
        assert_eq!(p.video.encoder, VideoEncoder::X264);
        p.video.rate_control = RateControl::AverageBitrate { kbps: 1000 };
        assert_eq!(p.video.quality_slider(), None);
        p.set_quality_position(18);
        assert_eq!(p.video.rate_control, RateControl::Quality { value: 14 });
        p.set_speed(Speed::Slow);
        assert_eq!(p.video.preset.as_deref(), Some("slow"));
    }
}
