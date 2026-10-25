use std::sync::Arc;

use rustcut_core::render::{
    Availability, BitDepth, Capabilities, CodecParam, DEFAULT_PROFILE_ID, EncoderStatus, HardwareContext, ProfileField,
    ProfileIssue, ProfileLibrary, RateControl, RenderProfile, Selection, Speed, VideoEncoder,
};

use super::*;

fn state() -> PanelState {
    let library = ProfileLibrary::new(Vec::new());
    let selection = Selection::restore(&library, None, None, DEFAULT_PROFILE_ID);
    PanelState::new(library, selection)
}

fn caps(working: &[VideoEncoder], failed: &[(VideoEncoder, &str)]) -> Arc<Capabilities> {
    let mut encoders: Vec<EncoderStatus> = working
        .iter()
        .map(|e| EncoderStatus {
            encoder: *e,
            availability: Availability::Works,
        })
        .collect();
    encoders.extend(failed.iter().map(|(e, reason)| EncoderStatus {
        encoder: *e,
        availability: Availability::Failed((*reason).to_owned()),
    }));
    Arc::new(Capabilities {
        version: None,
        encoders,
        vidstab: true,
        hardware: HardwareContext::default(),
    })
}

fn pick(st: &PanelState, row: Row, label: &str) -> usize {
    st.choices(row)
        .unwrap()
        .iter()
        .position(|(l, _)| l == label)
        .unwrap_or_else(|| panic!("{label} not in {:?}", st.choices(row)))
}

#[test]
fn offered_encoders_follow_the_check() {
    let mut st = state();
    let software = st.offered_encoders();
    assert_eq!(software.len(), 5, "while checking: the software encoders");
    st.encoders = Encoders::Known(caps(
        &[VideoEncoder::X264, VideoEncoder::X265],
        &[
            (VideoEncoder::SvtAv1, "no luck"),
            (VideoEncoder::HevcVaapi, "no device"),
        ],
    ));
    assert_eq!(
        st.offered_encoders(),
        [VideoEncoder::SvtAv1, VideoEncoder::X265, VideoEncoder::X264],
        "the current encoder stays listed"
    );
    let issue = st.availability_issue().unwrap();
    assert_eq!(issue.message, "AV1 (SVT-AV1) does not work here: no luck");
    let items = st.choices(Row::Encoder).unwrap();
    assert_eq!(
        items.first().map(|(l, _)| l.as_str()),
        Some("AV1 (SVT-AV1) (not available here)")
    );
    assert_eq!(
        st.check_status(),
        CheckStatus::Skipped,
        "no check for an encoder that does not work"
    );

    st.choose(Row::Encoder, 2);
    assert_eq!(st.profile().video.encoder, VideoEncoder::X264);
    assert_eq!(st.offered_encoders(), [VideoEncoder::X265, VideoEncoder::X264]);
    assert_eq!(st.check_status(), CheckStatus::Pending);
}

#[test]
fn speeds_and_the_custom_item() {
    let mut st = state();
    assert_eq!(st.current_choice(Row::Speed, &st.choices(Row::Speed).unwrap()), 2);
    st.choose(Row::Speed, 4);
    assert_eq!(st.profile().video.preset.as_deref(), Some("2"));
    st.selection.working.video.preset = Some("5".into());
    let items = st.choices(Row::Speed).unwrap();
    assert_eq!(items.len(), 6);
    assert_eq!(items.last().map(|(l, _)| l.as_str()), Some("Custom: 5 - Slow quality"));
    assert_eq!(st.current_choice(Row::Speed, &items), 5);
    assert!(!st.choose(Row::Speed, 5), "the custom item changes nothing");
    assert!(st.choose(Row::Speed, 0));
    assert_eq!(st.profile().video.speed(), Some(Speed::Fastest));
}

#[test]
fn rate_modes_values_and_steps() {
    let mut st = state();
    st.choose(Row::Encoder, pick(&st, Row::Encoder, "H.264 / AVC (x264)"));
    let qp = pick(&st, Row::RateMode, "Constant QP");
    assert!(st.choose(Row::RateMode, qp));
    assert_eq!(st.profile().video.rate_control, RateControl::ConstantQp { qp: 22 });
    assert_eq!(
        st.type_text(Row::RateValue, "70"),
        Err(NotAValue),
        "beyond x264's QP range"
    );
    assert_eq!(st.type_text(Row::RateValue, "18"), Ok(true));
    assert!(st.step(Row::RateValue, -3));
    assert_eq!(st.profile().video.rate_control, RateControl::ConstantQp { qp: 15 });

    let two = pick(&st, Row::RateMode, "Bit rate, two passes");
    st.choose(Row::RateMode, two);
    assert_eq!(st.profile().video.rate_control, RateControl::TwoPass { kbps: 4_000 });
    st.step(Row::RateValue, 5);
    assert_eq!(st.profile().video.rate_control, RateControl::TwoPass { kbps: 4_500 });
    let average = pick(&st, Row::RateMode, "Average bit rate");
    st.choose(Row::RateMode, average);
    assert_eq!(
        st.profile().video.rate_control,
        RateControl::AverageBitrate { kbps: 4_500 },
        "the bit rate carries over"
    );
    assert_eq!(st.type_text(Row::RateValue, "abc"), Err(NotAValue));

    assert!(st.slide(Row::Quality, 3));
    assert_eq!(st.profile().video.rate_control, RateControl::Quality { value: 29 });
}

#[test]
fn fields_switches_and_params() {
    let mut st = state();
    assert_eq!(st.type_text(Row::FilmGrain, "51"), Err(NotAValue));
    assert_eq!(st.type_text(Row::FilmGrain, ""), Ok(true));
    assert_eq!(st.profile().video.film_grain, None);
    assert!(st.step(Row::FilmGrain, 1));
    assert_eq!(
        st.profile().video.film_grain,
        Some(8),
        "a first step starts at the usual value"
    );
    assert_eq!(st.type_text(Row::Keyint, "0"), Err(NotAValue));
    assert!(st.step(Row::Keyint, 1));
    assert_eq!(st.profile().video.keyint, Some(250));
    assert!(st.step(Row::Keyint, -2));
    assert_eq!(st.profile().video.keyint, Some(230));
    assert_eq!(st.type_text(Row::Level, "4.x"), Err(NotAValue));

    assert_eq!(
        st.type_text(Row::CodecParams, " enable-overlays=1 : scm = 0 "),
        Ok(true)
    );
    assert_eq!(
        st.profile().video.codec_params,
        [CodecParam::new("enable-overlays", "1"), CodecParam::new("scm", "0")]
    );
    assert_eq!(params_text(&st.profile().video.codec_params), "enable-overlays=1:scm=0");
    assert_eq!(st.type_text(Row::CodecParams, "nokey"), Err(NotAValue));

    st.choose(Row::Encoder, pick(&st, Row::Encoder, "H.265 / HEVC (x265)"));
    assert!(st.toggle(Row::SceneCuts));
    assert_eq!(st.profile().video.scene_cuts, Some(false));
    assert!(st.toggle(Row::SceneCuts));
    assert_eq!(st.profile().video.scene_cuts, None);
}

#[test]
fn bit_depth_and_codec_profile_follow_each_other() {
    let mut st = state();
    st.choose(Row::Encoder, pick(&st, Row::Encoder, "H.264 / AVC (x264)"));
    st.choose(Row::BitDepth, 0);
    st.choose(Row::CodecProfile, pick(&st, Row::CodecProfile, "high"));
    st.choose(Row::BitDepth, 1);
    assert_eq!(st.profile().video.profile.as_deref(), Some("high10"));
    st.choose(Row::CodecProfile, pick(&st, Row::CodecProfile, "main"));
    assert_eq!(st.profile().video.bit_depth, BitDepth::Eight);
    assert!(st.profile().is_usable());
}

#[test]
fn audio_codec_drops_what_it_does_not_take() {
    let mut st = state();
    st.choose(Row::AudioCodec, pick(&st, Row::AudioCodec, "AAC"));
    assert!(st.slide(Row::AudioBitrate, 6), "AAC's last step: 320 kbit/s");
    st.choose(Row::AudioSampleRate, pick(&st, Row::AudioSampleRate, "44.1 kHz"));
    st.choose(Row::AudioChannels, pick(&st, Row::AudioChannels, "Stereo"));
    st.choose(Row::AudioCodec, pick(&st, Row::AudioCodec, "Opus"));
    let a = st.profile().audio;
    assert_eq!((a.bitrate_kbps, a.sample_rate, a.channels), (Some(320), None, Some(2)));
    st.choose(Row::AudioCodec, pick(&st, Row::AudioCodec, "Copy (no re-encode)"));
    let a = st.profile().audio;
    assert_eq!((a.bitrate_kbps, a.sample_rate, a.channels), (None, None, None));
}

#[test]
fn the_check_counts_only_for_its_settings() {
    let mut st = state();
    st.encoders = Encoders::Known(caps(&[VideoEncoder::SvtAv1, VideoEncoder::X264], &[]));
    assert_eq!(st.check_status(), CheckStatus::Pending);
    let checked: RenderProfile = st.profile().clone();
    st.check = Some(Check {
        profile: checked,
        issues: vec![ProfileIssue::warning(ProfileField::Encoder, "AV1 (SVT-AV1): a warning")],
    });
    assert_eq!(st.check_status(), CheckStatus::Done);
    assert_eq!(st.issues().len(), 1);
    st.choose(Row::Speed, 3);
    assert_eq!(st.check_status(), CheckStatus::Pending);
    assert!(st.issues().is_empty(), "the old report is for other settings");
}
