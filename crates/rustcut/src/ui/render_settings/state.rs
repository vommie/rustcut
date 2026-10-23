//! The Render Settings panel's state (pure, tested): the profile library with the working copy, which encoders work
//! here, the real encoder's opinion of the working copy, and every edit a row of the panel can make. A row's choices
//! are computed from the working copy each time, so an index from the panel always means the list it saw.

use std::sync::Arc;

use rustcut_core::render::{
    Availability, Capabilities, EncoderBackend, MAX_FILM_GRAIN, ProfileField, ProfileIssue, ProfileLibrary,
    RateControl, RenderProfile, Selection, Severity, VideoEncoder,
};

pub use super::rows::{Pick, RateMode, Row};

/// A typed text that is not a value of its field (the field stays open and marked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a value for this field")]
pub struct NotAValue;

/// What is known about the encoders of this computer.
#[derive(Debug, Clone, Default)]
pub enum Encoders {
    /// The check at start is still running.
    #[default]
    Checking,
    Known(Arc<Capabilities>),
    /// ffmpeg could not be run: nothing is known.
    Unchecked,
}

/// The real encoder's report for a set of settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub profile: RenderProfile,
    pub issues: Vec<ProfileIssue>,
}

/// Where the check by ffmpeg stands for the working copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Not needed (the settings have errors already, or the encoder does not work here).
    Skipped,
    Pending,
    Running,
    Done,
    /// ffmpeg could not run the check.
    Failed,
}

const MAX_KBPS: u32 = 1_000_000;
const MAX_MEBIBYTES: u32 = 10_000_000;
/// Keyframe interval a first wheel step starts from.
const DEFAULT_KEYINT: u32 = 250;
/// SVT-AV1 film grain a first wheel step starts from (PyCutRv's value).
const DEFAULT_FILM_GRAIN: u8 = 8;

#[derive(Debug, Clone)]
pub struct PanelState {
    pub library: ProfileLibrary,
    pub selection: Selection,
    pub encoders: Encoders,
    /// The last report of the real encoder.
    pub check: Option<Check>,
    pub checking: bool,
    /// ffmpeg could not run the last check (its error).
    pub check_failed: Option<String>,
}

impl PanelState {
    pub fn new(library: ProfileLibrary, selection: Selection) -> Self {
        Self {
            library,
            selection,
            encoders: Encoders::Checking,
            check: None,
            checking: false,
            check_failed: None,
        }
    }

    /// The working copy: what the next job and the probe render get.
    pub fn profile(&self) -> &RenderProfile {
        &self.selection.working
    }

    pub fn is_modified(&self) -> bool {
        self.selection.is_modified(&self.library)
    }

    /// The encoders the panel offers: those that work here (software encoders while the check runs or when it could
    /// not run), plus the current one so a profile with an unavailable encoder still shows what it has.
    pub fn offered_encoders(&self) -> Vec<VideoEncoder> {
        let current = self.profile().video.encoder;
        VideoEncoder::ALL
            .into_iter()
            .filter(|e| {
                *e == current
                    || match &self.encoders {
                        Encoders::Known(caps) => caps.works(*e),
                        Encoders::Checking | Encoders::Unchecked => e.spec().backend == EncoderBackend::Software,
                    }
            })
            .collect()
    }

    /// Why the current encoder cannot be used here (`None` while unknown or when it works).
    pub fn availability_issue(&self) -> Option<ProfileIssue> {
        let Encoders::Known(caps) = &self.encoders else {
            return None;
        };
        let encoder = self.profile().video.encoder;
        let name = encoder.spec().display_name;
        let status = caps.encoders.iter().find(|s| s.encoder == encoder)?;
        let message = match &status.availability {
            Availability::Works => return None,
            Availability::NotBuilt => format!("{name} is not part of this ffmpeg"),
            Availability::Failed(reason) => format!("{name} does not work here: {reason}"),
        };
        Some(ProfileIssue::error(ProfileField::Encoder, message))
    }

    /// Every issue of the working copy, errors first: the catalog's checks, the encoder's availability and the real
    /// encoder's report when it is for these settings.
    pub fn issues(&self) -> Vec<ProfileIssue> {
        let profile = self.profile();
        let mut issues = profile.validate();
        issues.extend(self.availability_issue());
        if let Some(check) = self.check.as_ref().filter(|c| c.profile.same_settings(profile)) {
            for issue in &check.issues {
                if !issues.contains(issue) {
                    issues.push(issue.clone());
                }
            }
        }
        issues.sort_by_key(|i| std::cmp::Reverse(i.severity));
        issues
    }

    /// Where the real encoder's check stands for the working copy.
    pub fn check_status(&self) -> CheckStatus {
        let profile = self.profile();
        if self.checking {
            return CheckStatus::Running;
        }
        if self.check.as_ref().is_some_and(|c| c.profile.same_settings(profile)) {
            return CheckStatus::Done;
        }
        if self.check_failed.is_some() {
            return CheckStatus::Failed;
        }
        let blocked = profile.validate().iter().any(|i| i.severity == Severity::Error)
            || self.availability_issue().is_some()
            || matches!(self.encoders, Encoders::Checking);
        if blocked {
            CheckStatus::Skipped
        } else {
            CheckStatus::Pending
        }
    }

    /// Item `index` of a list row was chosen. `true` when the working copy changed.
    pub fn choose(&mut self, row: Row, index: usize) -> bool {
        let Some(pick) = self
            .choices(row)
            .and_then(|items| items.into_iter().nth(index))
            .map(|(_, pick)| pick)
        else {
            return false;
        };
        let before = self.selection.working.clone();
        super::choices::apply(&mut self.selection.working, pick);
        self.selection.working != before
    }

    /// The quality slider moved to `position`.
    pub fn slide(&mut self, position: u16) -> bool {
        let before = self.selection.working.video.rate_control;
        self.selection.working.set_quality_position(position);
        self.selection.working.video.rate_control != before
    }

    /// The switch of a row was flipped.
    pub fn toggle(&mut self, row: Row) -> bool {
        let v = &mut self.selection.working.video;
        match row {
            // Off writes `scenecut=0`; on is the encoder's default (nothing written).
            Row::SceneCuts if v.encoder.spec().scene_cuts => {
                v.scene_cuts = if v.scene_cuts == Some(false) { None } else { Some(false) };
                true
            }
            _ => false,
        }
    }

    /// A typed value of a field row. `Err` keeps the field open and marked (the text is not a value).
    pub fn type_text(&mut self, row: Row, text: &str) -> Result<bool, NotAValue> {
        let text = text.trim();
        let before = self.selection.working.clone();
        let v = &mut self.selection.working.video;
        match row {
            Row::RateValue => {
                let value: u32 = text.parse().ok().ok_or(NotAValue)?;
                v.rate_control = rate_with_value(v.encoder, v.rate_control, value).ok_or(NotAValue)?;
            }
            Row::Level => {
                if !text.is_empty() && !text.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return Err(NotAValue);
                }
                v.level = (!text.is_empty()).then(|| text.to_owned());
            }
            Row::Keyint => {
                v.keyint = if text.is_empty() {
                    None
                } else {
                    Some(text.parse::<u32>().ok().filter(|k| *k > 0).ok_or(NotAValue)?)
                };
            }
            Row::FilmGrain => {
                v.film_grain = if text.is_empty() {
                    None
                } else {
                    Some(
                        text.parse::<u8>()
                            .ok()
                            .filter(|g| *g <= MAX_FILM_GRAIN)
                            .ok_or(NotAValue)?,
                    )
                };
            }
            Row::CodecParams => v.codec_params = parse_params(text)?,
            _ => return Err(NotAValue),
        }
        Ok(self.selection.working != before)
    }

    /// The mouse wheel over a number row: `steps` up (positive) or down.
    pub fn step(&mut self, row: Row, steps: i32) -> bool {
        let before = self.selection.working.clone();
        let v = &mut self.selection.working.video;
        let add = |value: u32, by: i64, min: u32, max: u32| {
            let next = i64::from(value)
                .saturating_add(by)
                .clamp(i64::from(min), i64::from(max));
            u32::try_from(next).unwrap_or(min)
        };
        match row {
            Row::RateValue => {
                let (value, unit, min, max) = rate_value(v.encoder, v.rate_control);
                let next = add(value, i64::from(steps).saturating_mul(unit), min, max);
                if let Some(rc) = rate_with_value(v.encoder, v.rate_control, next) {
                    v.rate_control = rc;
                }
            }
            Row::Keyint => {
                v.keyint = Some(
                    v.keyint
                        .map_or(DEFAULT_KEYINT, |k| add(k, i64::from(steps) * 10, 1, 100_000)),
                );
            }
            Row::FilmGrain if v.encoder.spec().film_grain => {
                let grain = v.film_grain.map_or(DEFAULT_FILM_GRAIN, |g| {
                    u8::try_from(add(u32::from(g), i64::from(steps), 0, u32::from(MAX_FILM_GRAIN))).unwrap_or(g)
                });
                v.film_grain = Some(grain);
            }
            _ => {}
        }
        self.selection.working != before
    }
}

/// `(value, wheel step, min, max)` of the rate control's number.
pub fn rate_value(encoder: VideoEncoder, rc: RateControl) -> (u32, i64, u32, u32) {
    let spec = encoder.spec();
    match rc {
        RateControl::Quality { value } => (
            u32::from(value),
            1,
            u32::from(spec.quality.min),
            u32::from(spec.quality.max),
        ),
        RateControl::ConstantQp { qp } => {
            let scale = spec.constant_qp.unwrap_or(spec.quality);
            (u32::from(qp), 1, u32::from(scale.min), u32::from(scale.max))
        }
        RateControl::AverageBitrate { kbps } | RateControl::TwoPass { kbps } => (kbps, 100, 1, MAX_KBPS),
        RateControl::TargetSize { mebibytes } => (mebibytes, 10, 1, MAX_MEBIBYTES),
    }
}

/// The rate control with a new number, when the number is in its range.
fn rate_with_value(encoder: VideoEncoder, rc: RateControl, value: u32) -> Option<RateControl> {
    let (_, _, min, max) = rate_value(encoder, rc);
    if !(min..=max).contains(&value) {
        return None;
    }
    let small = || u16::try_from(value).ok();
    Some(match rc {
        RateControl::Quality { .. } => RateControl::Quality { value: small()? },
        RateControl::ConstantQp { .. } => RateControl::ConstantQp { qp: small()? },
        RateControl::AverageBitrate { .. } => RateControl::AverageBitrate { kbps: value },
        RateControl::TwoPass { .. } => RateControl::TwoPass { kbps: value },
        RateControl::TargetSize { .. } => RateControl::TargetSize { mebibytes: value },
    })
}

/// `key=value:key=value` (spaces around the pieces allowed); an empty text is no parameter.
pub fn parse_params(text: &str) -> Result<Vec<rustcut_core::render::CodecParam>, NotAValue> {
    text.split(':')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            let (key, value) = piece.split_once('=').ok_or(NotAValue)?;
            Ok(rustcut_core::render::CodecParam::new(key.trim(), value.trim()))
        })
        .collect()
}

/// The parameters as the field shows them.
pub fn params_text(params: &[rustcut_core::render::CodecParam]) -> String {
    params
        .iter()
        .map(|p| format!("{}={}", p.key, p.value))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
