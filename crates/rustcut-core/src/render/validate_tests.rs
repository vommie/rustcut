use super::*;
use crate::render::{AudioSettings, CodecParam, Container, builtin_profiles};

fn base(encoder: VideoEncoder) -> RenderProfile {
    RenderProfile {
        id: "t".into(),
        name: "Test".into(),
        container: Container::Mkv,
        video: VideoSettings::new(encoder),
        audio: AudioSettings::default(),
    }
}

/// `(field, severity)` of every issue.
fn found(p: &RenderProfile) -> Vec<(ProfileField, Severity)> {
    p.validate().into_iter().map(|i| (i.field, i.severity)).collect()
}

fn one(field: ProfileField, severity: Severity) -> Vec<(ProfileField, Severity)> {
    vec![(field, severity)]
}

#[test]
fn defaults_of_every_encoder_are_clean() {
    for encoder in VideoEncoder::ALL {
        assert_eq!(found(&base(encoder)), vec![], "{encoder:?}");
    }
    for p in builtin_profiles() {
        assert!(issues_of(&p.validate(), Severity::Error).next().is_none(), "{}", p.id);
    }
}

#[test]
fn rate_control_ranges_and_modes() {
    let mut p = base(VideoEncoder::X264);
    p.video.rate_control = RateControl::Quality { value: 52 };
    assert_eq!(found(&p), one(ProfileField::RateControl, Severity::Error));
    p.video.rate_control = RateControl::ConstantQp { qp: 69 };
    assert_eq!(found(&p), vec![]);
    p.video.rate_control = RateControl::ConstantQp { qp: 70 };
    assert_eq!(found(&p), one(ProfileField::RateControl, Severity::Error));
    let mut aom = base(VideoEncoder::Aom);
    aom.video.rate_control = RateControl::ConstantQp { qp: 20 };
    assert_eq!(
        found(&aom),
        one(ProfileField::RateControl, Severity::Error),
        "no QP mode"
    );
    let mut svt = base(VideoEncoder::SvtAv1);
    svt.video.rate_control = RateControl::TargetSize { mebibytes: 100 };
    assert_eq!(
        found(&svt),
        one(ProfileField::RateControl, Severity::Error),
        "no two passes"
    );
    svt.video.rate_control = RateControl::AverageBitrate { kbps: 0 };
    assert_eq!(found(&svt), one(ProfileField::RateControl, Severity::Error));
}

#[test]
fn level_aq_scene_cuts_and_variance_boost() {
    let mut p = base(VideoEncoder::SvtAv1);
    p.video.level = Some("4.1".into());
    p.video.scene_cuts = Some(false);
    p.video.aq_mode = Some("3".into());
    p.video.variance_boost = Some(5);
    assert_eq!(
        found(&p),
        vec![
            (ProfileField::Level, Severity::Error),
            (ProfileField::VarianceBoost, Severity::Error),
            (ProfileField::AqMode, Severity::Error),
            (ProfileField::SceneCuts, Severity::Error),
        ]
    );
    let mut x265 = base(VideoEncoder::X265);
    x265.video.level = Some("5.1".into());
    x265.video.scene_cuts = Some(false);
    x265.video.aq_mode = Some("4".into());
    assert_eq!(found(&x265), vec![]);
    x265.video.variance_boost = Some(1);
    x265.video.level = Some("high".into());
    assert_eq!(
        found(&x265),
        vec![
            (ProfileField::Level, Severity::Error),
            (ProfileField::VarianceBoost, Severity::Error)
        ]
    );
}

#[test]
fn film_grain_and_fast_presets() {
    let mut p = base(VideoEncoder::SvtAv1);
    p.video.film_grain = Some(8);
    assert_eq!(found(&p), vec![], "the default preset 6 is fine");
    p.video.preset = Some("8".into());
    assert_eq!(found(&p), one(ProfileField::FilmGrain, Severity::Warning));
    p.video.film_grain = Some(51);
    assert_eq!(found(&p), one(ProfileField::FilmGrain, Severity::Error));
}

#[test]
fn codec_params_are_checked_against_the_fields() {
    let mut p = base(VideoEncoder::SvtAv1);
    p.video.codec_params = vec![
        CodecParam::new("enable-overlays", "1"),
        CodecParam::new("crf", "20"),
        CodecParam::new("bad key", "1"),
        CodecParam::new("lp", "a:b"),
        CodecParam::new("lp", "2"),
    ];
    let messages: Vec<String> = p.validate().into_iter().map(|i| i.message).collect();
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(
        messages[0].contains("conflicts with RustCut's own settings"),
        "{messages:?}"
    );
    assert!(messages[1].contains("not a parameter name"), "{messages:?}");

    p.video.codec_params = vec![CodecParam::new("variance-boost-strength", "3")];
    assert_eq!(found(&p), vec![], "an unused field leaves its key to the passthrough");
    p.video.variance_boost = Some(2);
    let issues = p.validate();
    assert!(issues[0].message.contains("variance boost and parameter"), "{issues:?}");

    let mut x264 = base(VideoEncoder::X264);
    x264.video.keyint = Some(250);
    x264.video.codec_params = vec![CodecParam::new("keyint", "300"), CodecParam::new("preset", "slow")];
    let messages: Vec<String> = x264.validate().into_iter().map(|i| i.message).collect();
    assert_eq!(
        messages,
        [
            "`keyint` is set twice (keyframe interval and parameter)",
            "`preset` conflicts with RustCut's own settings; use the fields above"
        ]
    );

    let mut vaapi = base(VideoEncoder::HevcVaapi);
    vaapi.video.codec_params = vec![CodecParam::new("a", "1"), CodecParam::new("b", "2")];
    assert_eq!(
        found(&vaapi),
        one(ProfileField::CodecParams, Severity::Error),
        "reported once"
    );
}

#[test]
fn audio_details() {
    let mut p = base(VideoEncoder::X264);
    p.audio.sample_rate = Some(44_100);
    assert_eq!(
        found(&p),
        one(ProfileField::Audio, Severity::Warning),
        "Opus ignores it"
    );
    p.audio.codec = AudioCodec::Aac;
    assert_eq!(found(&p), vec![]);
    p.audio.sample_rate = Some(1_000);
    p.audio.channels = Some(12);
    assert_eq!(
        found(&p),
        vec![
            (ProfileField::Audio, Severity::Warning),
            (ProfileField::Audio, Severity::Error)
        ]
    );
    p.audio = AudioSettings {
        codec: AudioCodec::Copy,
        bitrate_kbps: Some(128),
        channels: Some(2),
        sample_rate: None,
    };
    assert_eq!(found(&p).len(), 2);
}
