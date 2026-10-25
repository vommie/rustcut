//! What the Render Settings panel shows, computed from its state (pure, tested); the bridge copies it into the
//! `RenderSettings` global. Rows exist only for options the current encoder has.

use rustcut_core::render::{AudioCodec, OptionId, ProfileField, ProfileIssue, RateControl, Severity, VideoEncoder};

use super::state::{CheckStatus, PanelState, Row, params_text};

/// How a row is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Choice = 0,
    Number = 1,
    Switch = 2,
    Text = 3,
    Slider = 4,
}

/// The worst issue of a row's field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    #[default]
    None = 0,
    Warning = 1,
    Error = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub row: Row,
    pub kind: RowKind,
    pub label: String,
    pub tooltip: String,
    pub items: Vec<String>,
    pub index: usize,
    /// A number or text field's value; the slider's caption.
    pub text: String,
    pub placeholder: String,
    /// A number's unit, shown inside its field.
    pub unit: &'static str,
    pub checked: bool,
    pub steps: u16,
    pub position: u16,
    pub enabled: bool,
    pub mark: Mark,
}

/// An issue of the settings: its bubble stands at the first row of its field (M6.5c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueView {
    pub error: bool,
    pub text: String,
    pub field: ProfileField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    /// Profile names; the selected one says "(modified)" while the working copy differs.
    pub profiles: Vec<String>,
    pub profile_index: usize,
    pub modified: bool,
    /// The selection is the user's own profile (save, rename, delete).
    pub user_profile: bool,
    pub issues: Vec<IssueView>,
    /// The real encoder's check, in a few words ("" when there is nothing to say).
    pub check_text: String,
    pub check_running: bool,
    /// Which encoders work: still checking, or unknown.
    pub encoders_note: String,
    pub video: Vec<RowView>,
    pub audio: Vec<RowView>,
    pub expert: Vec<RowView>,
}

/// The panel's view of `state`.
pub fn panel(state: &PanelState) -> PanelView {
    let issues = state.issues();
    let modified = state.is_modified();
    let base = &state.selection.base;
    let profiles = state
        .library
        .all()
        .map(|p| {
            if p.id == *base && modified {
                format!("{} (modified)", p.name)
            } else {
                p.name.clone()
            }
        })
        .collect();
    let profile_index = state.library.all().position(|p| p.id == *base).unwrap_or(0);
    let rows =
        |list: &[Row]| -> Vec<RowView> { list.iter().filter_map(|row| row_view(state, *row, &issues)).collect() };
    let (check_text, check_running) = match state.check_status() {
        CheckStatus::Running => ("Checking with ffmpeg...".to_owned(), true),
        CheckStatus::Done if issues.is_empty() => ("ffmpeg accepts these settings".to_owned(), false),
        CheckStatus::Failed => (
            format!(
                "Not checked: {}",
                state.check_failed.as_deref().unwrap_or("ffmpeg could not run")
            ),
            false,
        ),
        CheckStatus::Done | CheckStatus::Pending | CheckStatus::Skipped => (String::new(), false),
    };
    let encoders_note = match state.encoders {
        super::state::Encoders::Checking => "Checking which encoders work here...".to_owned(),
        super::state::Encoders::Unchecked => "Encoders not checked: ffmpeg could not run".to_owned(),
        super::state::Encoders::Known(_) => String::new(),
    };
    PanelView {
        profiles,
        profile_index,
        modified,
        user_profile: state.library.is_user(base),
        issues: issues
            .iter()
            .map(|i| IssueView {
                error: i.severity == Severity::Error,
                text: sentence(&i.message),
                field: i.field,
            })
            .collect(),
        check_text,
        check_running,
        encoders_note,
        video: rows(&[Row::Encoder, Row::Quality, Row::Speed, Row::Container]),
        audio: rows(&[
            Row::AudioCodec,
            Row::AudioBitrate,
            Row::AudioChannels,
            Row::AudioSampleRate,
        ]),
        expert: rows(&[
            Row::RateMode,
            Row::RateValue,
            Row::Preset,
            Row::Tune,
            Row::CodecProfile,
            Row::Level,
            Row::BitDepth,
            Row::Keyint,
            Row::FilmGrain,
            Row::VarianceBoost,
            Row::AqMode,
            Row::SceneCuts,
            Row::ColorTags,
            Row::ColorRange,
            Row::CodecParams,
        ]),
    }
}

/// An issue message as a sentence (capital letter first).
fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// The label of a row as the panel shows it (for the rate value: as in quality mode).
pub fn row_label(row: Row) -> &'static str {
    option_of(row, RateControl::Quality { value: 0 }).help().label
}

/// The help entry of a row (the rate value's depends on the mode).
fn option_of(row: Row, rc: RateControl) -> OptionId {
    match row {
        Row::Encoder => OptionId::Encoder,
        Row::Quality => OptionId::Quality,
        Row::Speed => OptionId::Speed,
        Row::Container => OptionId::Container,
        Row::AudioCodec => OptionId::AudioCodec,
        Row::AudioBitrate => OptionId::AudioBitrate,
        Row::AudioChannels => OptionId::AudioChannels,
        Row::AudioSampleRate => OptionId::AudioSampleRate,
        Row::RateMode => OptionId::RateControl,
        Row::RateValue => match rc {
            RateControl::Quality { .. } => OptionId::Quality,
            RateControl::ConstantQp { .. } => OptionId::ConstantQp,
            RateControl::AverageBitrate { .. } | RateControl::TwoPass { .. } => OptionId::Bitrate,
            RateControl::TargetSize { .. } => OptionId::TargetSize,
        },
        Row::Preset => OptionId::Preset,
        Row::Tune => OptionId::Tune,
        Row::CodecProfile => OptionId::CodecProfile,
        Row::Level => OptionId::Level,
        Row::BitDepth => OptionId::BitDepth,
        Row::Keyint => OptionId::Keyint,
        Row::FilmGrain => OptionId::FilmGrain,
        Row::VarianceBoost => OptionId::VarianceBoost,
        Row::AqMode => OptionId::AqMode,
        Row::SceneCuts => OptionId::SceneCuts,
        Row::ColorTags => OptionId::ColorTags,
        Row::ColorRange => OptionId::ColorRange,
        Row::CodecParams => OptionId::CodecParams,
    }
}

fn row_view(state: &PanelState, row: Row, issues: &[ProfileIssue]) -> Option<RowView> {
    let p = state.profile();
    let v = &p.video;
    let spec = v.encoder.spec();
    let option = option_of(row, v.rate_control);
    let shown = match row {
        Row::AudioBitrate => p.audio.codec.takes_bitrate(),
        Row::AudioChannels | Row::AudioSampleRate => p.audio.codec != AudioCodec::Copy,
        Row::Encoder | Row::Quality | Row::Container | Row::AudioCodec | Row::RateMode | Row::RateValue => true,
        _ => option.applies_to(v.encoder),
    };
    if !shown {
        return None;
    }
    let help = option.help();
    let mark = row.field().map(|field| mark_of(issues, field)).unwrap_or_default();
    let mut view = RowView {
        row,
        kind: RowKind::Choice,
        label: help.label.to_owned(),
        tooltip: help.tooltip.to_owned(),
        items: Vec::new(),
        index: 0,
        text: String::new(),
        placeholder: String::new(),
        unit: "",
        checked: false,
        steps: 1,
        position: 0,
        enabled: true,
        mark,
    };
    if let Some(items) = state.choices(row) {
        view.index = state.current_choice(row, &items);
        view.items = items.into_iter().map(|(label, _)| label).collect();
    }
    match row {
        Row::Quality => {
            view.kind = RowKind::Slider;
            if let RateControl::Quality { value } = v.rate_control {
                let slider = spec.quality_slider(value);
                view.steps = slider.steps;
                view.position = slider.position;
                let outside = if slider.inside { "" } else { ", outside the slider" };
                view.text = format!("{} · {} {value}{outside}", slider.band.label(), spec.quality.label);
            } else {
                let slider = spec.quality_slider(spec.quality.default);
                view.steps = slider.steps;
                view.position = slider.position;
                view.enabled = false;
                view.text = format!("{} (see Expert)", rate_summary(v.encoder, v.rate_control));
            }
        }
        Row::Speed => view.enabled = spec.speeds.is_some(),
        Row::AudioSampleRate if p.audio.codec == AudioCodec::Opus => {
            view.enabled = false;
            view.items = vec!["48 kHz (Opus)".to_owned()];
            view.index = 0;
        }
        Row::RateValue => {
            view.kind = RowKind::Number;
            let (value, _, min, max) = super::state::rate_value(v.encoder, v.rate_control);
            view.text = value.to_string();
            (view.label, view.unit) = match v.rate_control {
                RateControl::Quality { .. } => (spec.quality.label.to_owned(), ""),
                RateControl::ConstantQp { .. } => ("QP".to_owned(), ""),
                RateControl::AverageBitrate { .. } | RateControl::TwoPass { .. } => ("Bit rate".to_owned(), "kbit/s"),
                RateControl::TargetSize { .. } => ("Target size".to_owned(), "MiB"),
            };
            view.placeholder = format!("{min}-{max}");
        }
        // A text like "4.1": no steps, so no spin arrows.
        Row::Level => {
            view.kind = RowKind::Text;
            view.text = v.level.clone().unwrap_or_default();
            "auto".clone_into(&mut view.placeholder);
        }
        Row::Keyint => {
            view.kind = RowKind::Number;
            view.unit = "frames";
            view.text = v.keyint.map(|k| k.to_string()).unwrap_or_default();
            "auto".clone_into(&mut view.placeholder);
        }
        Row::FilmGrain => {
            view.kind = RowKind::Number;
            view.text = v.film_grain.map(|g| g.to_string()).unwrap_or_default();
            "off".clone_into(&mut view.placeholder);
        }
        Row::SceneCuts => {
            view.kind = RowKind::Switch;
            view.checked = v.scene_cuts != Some(false);
        }
        Row::CodecParams => {
            view.kind = RowKind::Text;
            view.text = params_text(&v.codec_params);
            "key=value:key=value".clone_into(&mut view.placeholder);
        }
        _ => {}
    }
    Some(view)
}

fn mark_of(issues: &[ProfileIssue], field: ProfileField) -> Mark {
    issues
        .iter()
        .filter(|i| i.field == field)
        .map(|i| match i.severity {
            Severity::Error => Mark::Error,
            Severity::Warning => Mark::Warning,
        })
        .max()
        .unwrap_or_default()
}

/// A short text of a rate control that is not constant quality.
fn rate_summary(encoder: VideoEncoder, rc: RateControl) -> String {
    match rc {
        RateControl::Quality { value } => format!("{} {value}", encoder.spec().quality.label),
        RateControl::ConstantQp { qp } => format!("Constant QP {qp}"),
        RateControl::AverageBitrate { kbps } => format!("{kbps} kbit/s"),
        RateControl::TwoPass { kbps } => format!("{kbps} kbit/s, two passes"),
        RateControl::TargetSize { mebibytes } => format!("{mebibytes} MiB, two passes"),
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::render::{DEFAULT_PROFILE_ID, ProfileLibrary, Selection};

    use super::*;

    fn state() -> PanelState {
        let library = ProfileLibrary::new(Vec::new());
        let selection = Selection::restore(&library, None, None, DEFAULT_PROFILE_ID);
        PanelState::new(library, selection)
    }

    #[test]
    fn rows_follow_the_encoder() {
        let mut st = state();
        let view = panel(&st);
        assert_eq!(view.profiles.first().map(String::as_str), Some("AV1 balanced"));
        assert!(!view.modified);
        let keys = |rows: &[RowView]| rows.iter().map(|r| r.row.key()).collect::<Vec<_>>();
        assert_eq!(keys(&view.video), ["encoder", "quality", "speed", "container"]);
        assert_eq!(
            keys(&view.audio),
            ["audio-codec", "audio-bitrate", "audio-channels", "audio-rate"]
        );
        let expert = keys(&view.expert);
        assert!(expert.contains(&"film-grain") && expert.contains(&"variance-boost"));
        assert!(!expert.contains(&"scene-cuts") && !expert.contains(&"codec-profile"));
        let quality = view.video.get(1).unwrap();
        assert_eq!(quality.text, "High · CRF 26");
        assert_eq!((quality.steps, quality.position), (34, 24));

        let x264 = st
            .choices(Row::Encoder)
            .unwrap()
            .iter()
            .position(|(l, _)| l.contains("x264"))
            .unwrap();
        assert!(st.choose(Row::Encoder, x264));
        let view = panel(&st);
        assert_eq!(
            view.profiles.first().map(String::as_str),
            Some("AV1 balanced (modified)")
        );
        let expert = keys(&view.expert);
        assert!(expert.contains(&"scene-cuts") && expert.contains(&"codec-profile") && expert.contains(&"level"));
        assert!(!expert.contains(&"film-grain"));
        let audio_rate = view.audio.iter().find(|r| r.row == Row::AudioSampleRate).unwrap();
        assert!(!audio_rate.enabled, "Opus is always 48 kHz");
    }

    #[test]
    fn issues_mark_rows_and_lead_the_list() {
        let mut st = state();
        st.selection.working.container = rustcut_core::render::Container::Mp4;
        st.selection.working.video.film_grain = Some(60);
        let view = panel(&st);
        assert_eq!(view.issues.len(), 2);
        assert!(
            view.issues
                .first()
                .is_some_and(|i| i.error && i.text.starts_with("Film grain"))
        );
        let container = view.video.iter().find(|r| r.row == Row::Container).unwrap();
        assert_eq!(container.mark, Mark::Warning);
        let grain = view.expert.iter().find(|r| r.row == Row::FilmGrain).unwrap();
        assert_eq!((grain.mark, grain.text.as_str()), (Mark::Error, "60"));
    }

    #[test]
    fn a_bit_rate_disables_the_slider() {
        let mut st = state();
        let modes = st.choices(Row::RateMode).unwrap();
        let average = modes.iter().position(|(l, _)| l == "Average bit rate").unwrap();
        assert!(st.choose(Row::RateMode, average));
        let view = panel(&st);
        let quality = view.video.get(1).unwrap();
        assert!(!quality.enabled);
        assert_eq!(quality.text, "4000 kbit/s (see Expert)");
        let value = view.expert.iter().find(|r| r.row == Row::RateValue).unwrap();
        assert_eq!(
            (value.label.as_str(), value.text.as_str(), value.unit),
            ("Bit rate", "4000", "kbit/s")
        );
    }
}
