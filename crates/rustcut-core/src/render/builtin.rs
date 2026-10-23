//! The built-in render profiles (ADR-14). Users copy and edit them; the built-ins themselves stay as shipped.

use super::{AudioCodec, AudioSettings, BitDepth, Container, RateControl, RenderProfile, VideoEncoder, VideoSettings};

/// Id of the profile new installations start with: PyCutRv's settings (SVT-AV1 preset 6, CRF 26, 10-bit, film grain 8).
pub const DEFAULT_PROFILE_ID: &str = "av1-balanced";

fn svt(value: u16, preset: &str) -> VideoSettings {
    VideoSettings {
        rate_control: RateControl::Quality { value },
        preset: Some(preset.to_owned()),
        tune: Some("0".to_owned()),
        bit_depth: BitDepth::Ten,
        film_grain: Some(8),
        variance_boost: Some(2),
        ..VideoSettings::new(VideoEncoder::SvtAv1)
    }
}

fn x26x(encoder: VideoEncoder, value: u16, preset: &str) -> VideoSettings {
    VideoSettings {
        rate_control: RateControl::Quality { value },
        preset: Some(preset.to_owned()),
        ..VideoSettings::new(encoder)
    }
}

fn audio(codec: AudioCodec, kbps: u32) -> AudioSettings {
    AudioSettings {
        codec,
        bitrate_kbps: Some(kbps),
        channels: None,
        sample_rate: None,
    }
}

/// All built-in profiles, the default first.
pub fn builtin_profiles() -> Vec<RenderProfile> {
    vec![
        RenderProfile {
            id: DEFAULT_PROFILE_ID.to_owned(),
            name: "AV1 balanced".to_owned(),
            container: Container::Mkv,
            video: svt(26, "6"),
            audio: audio(AudioCodec::Opus, 128),
        },
        RenderProfile {
            id: "av1-archive".to_owned(),
            name: "AV1 archive (10-bit, slow)".to_owned(),
            container: Container::Mkv,
            video: svt(24, "4"),
            audio: audio(AudioCodec::Opus, 160),
        },
        RenderProfile {
            id: "x265-quality".to_owned(),
            name: "x265 quality".to_owned(),
            container: Container::Mkv,
            video: VideoSettings {
                bit_depth: BitDepth::Ten,
                profile: Some("main10".to_owned()),
                ..x26x(VideoEncoder::X265, 20, "slow")
            },
            audio: audio(AudioCodec::Opus, 128),
        },
        RenderProfile {
            id: "x264-fast-share".to_owned(),
            name: "x264 fast share".to_owned(),
            container: Container::Mp4,
            video: x26x(VideoEncoder::X264, 23, "veryfast"),
            audio: audio(AudioCodec::Aac, 160),
        },
        RenderProfile {
            id: "x264-compatible".to_owned(),
            name: "x264 compatible (yuv420p, high profile)".to_owned(),
            container: Container::Mp4,
            video: VideoSettings {
                profile: Some("high".to_owned()),
                level: Some("4.1".to_owned()),
                ..x26x(VideoEncoder::X264, 20, "medium")
            },
            audio: audio(AudioCodec::Aac, 192),
        },
    ]
}

/// Whether `id` belongs to a built-in profile (user profiles must use other ids).
pub fn is_builtin_id(id: &str) -> bool {
    builtin_profiles().iter().any(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::render::Severity;

    #[test]
    fn built_ins_are_valid_and_unique() {
        let profiles = builtin_profiles();
        let mut ids = HashSet::new();
        for p in &profiles {
            assert!(ids.insert(p.id.clone()), "duplicate id {}", p.id);
            let errors: Vec<_> = p
                .validate()
                .into_iter()
                .filter(|i| i.severity == Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", p.id);
            assert!(is_builtin_id(&p.id));
        }
        assert_eq!(profiles.first().map(|p| p.id.as_str()), Some(DEFAULT_PROFILE_ID));
        assert!(!is_builtin_id("my-profile"));
    }

    #[test]
    fn profiles_survive_json() {
        for p in builtin_profiles() {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<RenderProfile>(&json).unwrap(), p);
        }
    }
}
