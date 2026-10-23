//! What is wrong or questionable about a render profile, field by field (ADR-14). The checks only need the catalog; the
//! real encoder's opinion (unknown codec parameters, its own warnings) comes from [`super::check_profile`].

use super::container::{CompatIssue, Severity};
use super::{
    AudioCodec, BitDepth, ProfileField, ProfileIssue, RateControl, RenderProfile, Setting, TuneTarget, VideoEncoder,
    VideoSettings,
};

/// Strongest SVT-AV1 variance boost.
pub const MAX_VARIANCE_BOOST: u8 = 4;
/// Strongest SVT-AV1 film grain synthesis.
pub const MAX_FILM_GRAIN: u8 = 50;
/// SVT-AV1 presets above this make film grain synthesis very slow (SVT-AV1's own advice).
const FILM_GRAIN_FAST_PRESET: u32 = 6;
/// Sample rates that make sense for an output (Hz).
const SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

pub(super) fn validate(profile: &RenderProfile) -> Vec<ProfileIssue> {
    let mut issues = Vec::new();
    if profile.name.trim().is_empty() {
        issues.push(ProfileIssue::error(ProfileField::Name, "the profile needs a name"));
    }
    let v = &profile.video;
    rate_control(v, &mut issues);
    preset_and_tune(v, &mut issues);
    profile_level_depth(v, &mut issues);
    tools(v, &mut issues);
    codec_params(v, &mut issues);
    audio(profile, &mut issues);
    for CompatIssue { severity, message } in profile.container.check(v.encoder, profile.audio.codec) {
        issues.push(ProfileIssue {
            field: ProfileField::Container,
            severity,
            message,
        });
    }
    issues
}

fn rate_control(v: &VideoSettings, issues: &mut Vec<ProfileIssue>) {
    let spec = v.encoder.spec();
    let field = ProfileField::RateControl;
    match v.rate_control {
        RateControl::Quality { value } => {
            let q = spec.quality;
            if !(q.min..=q.max).contains(&value) {
                issues.push(ProfileIssue::error(
                    field,
                    format!("{} of {} must be {} to {}", q.label, spec.display_name, q.min, q.max),
                ));
            }
        }
        RateControl::ConstantQp { qp } => match spec.constant_qp {
            None => issues.push(ProfileIssue::error(
                field,
                format!("{} has no constant QP mode", spec.display_name),
            )),
            Some(q) if !(q.min..=q.max).contains(&qp) => issues.push(ProfileIssue::error(
                field,
                format!("QP of {} must be {} to {}", spec.display_name, q.min, q.max),
            )),
            Some(_) => {}
        },
        RateControl::AverageBitrate { kbps } | RateControl::TwoPass { kbps } if kbps == 0 => {
            issues.push(ProfileIssue::error(field, "the bit rate must be above 0"));
        }
        RateControl::TargetSize { mebibytes: 0 } => {
            issues.push(ProfileIssue::error(field, "the target size must be above 0"));
        }
        RateControl::AverageBitrate { .. } | RateControl::TwoPass { .. } | RateControl::TargetSize { .. } => {}
    }
    if v.rate_control.is_two_pass() && !spec.two_pass {
        issues.push(ProfileIssue::error(
            field,
            format!("{} has no two-pass mode", spec.display_name),
        ));
    }
    if matches!(
        v.rate_control,
        RateControl::AverageBitrate { .. } | RateControl::TwoPass { .. } | RateControl::TargetSize { .. }
    ) && !spec.average_bitrate
    {
        issues.push(ProfileIssue::error(
            field,
            format!("{} has no bit rate mode", spec.display_name),
        ));
    }
}

fn preset_and_tune(v: &VideoSettings, issues: &mut Vec<ProfileIssue>) {
    let spec = v.encoder.spec();
    if let Some(preset) = &v.preset
        && !spec.has_preset(preset)
    {
        issues.push(ProfileIssue::error(
            ProfileField::Preset,
            format!("{} has no preset `{preset}`", spec.display_name),
        ));
    }
    if let Some(tune) = &v.tune
        && !spec.has_tune(tune)
    {
        issues.push(ProfileIssue::error(
            ProfileField::Tune,
            format!("{} has no tune `{tune}`", spec.display_name),
        ));
    }
}

fn profile_level_depth(v: &VideoSettings, issues: &mut Vec<ProfileIssue>) {
    let spec = v.encoder.spec();
    if let Some(profile) = &v.profile {
        let field = ProfileField::Profile;
        if spec.profiles.is_empty() {
            issues.push(ProfileIssue::error(
                field,
                format!("{} takes no profile", spec.display_name),
            ));
        } else if !spec.profiles.contains(&profile.as_str()) {
            issues.push(ProfileIssue::error(
                field,
                format!("{} has no profile `{profile}`", spec.display_name),
            ));
        } else if v.bit_depth == BitDepth::Ten && !profile.ends_with("10") {
            issues.push(ProfileIssue::error(
                field,
                format!("profile `{profile}` cannot encode 10-bit"),
            ));
        } else if v.bit_depth == BitDepth::Eight && profile.ends_with("10") {
            issues.push(ProfileIssue::warning(
                field,
                format!("profile `{profile}` is meant for 10-bit"),
            ));
        }
    }
    if let Some(level) = &v.level {
        if v.encoder.level_param().is_none() {
            issues.push(ProfileIssue::error(
                ProfileField::Level,
                format!("{} takes no level", spec.display_name),
            ));
        } else if level.is_empty() || !level.chars().all(|c| c.is_ascii_digit() || c == '.') {
            issues.push(ProfileIssue::error(
                ProfileField::Level,
                format!("`{level}` is not a level (expected e.g. 4.1)"),
            ));
        }
    }
    if v.bit_depth == BitDepth::Ten && spec.pix_fmt_10.is_none() {
        issues.push(ProfileIssue::error(
            ProfileField::BitDepth,
            format!("{} cannot encode 10-bit", spec.display_name),
        ));
    }
    if v.bit_depth == BitDepth::Ten && v.encoder == VideoEncoder::X264 {
        issues.push(ProfileIssue::warning(
            ProfileField::BitDepth,
            "10-bit H.264 does not play on many devices",
        ));
    }
}

/// GOP, film grain, variance boost, adaptive quantization and scene cuts.
fn tools(v: &VideoSettings, issues: &mut Vec<ProfileIssue>) {
    let spec = v.encoder.spec();
    if v.keyint == Some(0) {
        issues.push(ProfileIssue::error(
            ProfileField::Keyint,
            "the keyframe interval must be above 0",
        ));
    }
    if let Some(grain) = v.film_grain {
        if !spec.film_grain {
            issues.push(ProfileIssue::error(
                ProfileField::FilmGrain,
                format!("{} has no film grain synthesis", spec.display_name),
            ));
        } else if grain > MAX_FILM_GRAIN {
            issues.push(ProfileIssue::error(
                ProfileField::FilmGrain,
                format!("film grain must be 0 to {MAX_FILM_GRAIN}"),
            ));
        } else if grain > 0
            && v.preset
                .as_deref()
                .or(spec.default_preset)
                .and_then(|p| p.parse::<u32>().ok())
                .is_some_and(|p| p > FILM_GRAIN_FAST_PRESET)
        {
            issues.push(ProfileIssue::warning(
                ProfileField::FilmGrain,
                format!("film grain makes presets above {FILM_GRAIN_FAST_PRESET} much slower"),
            ));
        }
    }
    if let Some(boost) = v.variance_boost {
        if !spec.variance_boost {
            issues.push(ProfileIssue::error(
                ProfileField::VarianceBoost,
                format!("{} has no variance boost", spec.display_name),
            ));
        } else if boost > MAX_VARIANCE_BOOST {
            issues.push(ProfileIssue::error(
                ProfileField::VarianceBoost,
                format!("variance boost must be off or 1 to {MAX_VARIANCE_BOOST}"),
            ));
        }
    }
    if let Some(mode) = &v.aq_mode {
        match spec.aq {
            None => issues.push(ProfileIssue::error(
                ProfileField::AqMode,
                format!("{} has no adaptive quantization modes", spec.display_name),
            )),
            Some(aq) if !aq.modes.iter().any(|m| m.value == mode) => issues.push(ProfileIssue::error(
                ProfileField::AqMode,
                format!("{} has no AQ mode `{mode}`", spec.display_name),
            )),
            Some(_) => {}
        }
    }
    if v.scene_cuts.is_some() && !spec.scene_cuts {
        issues.push(ProfileIssue::error(
            ProfileField::SceneCuts,
            format!("{} has no scene cut setting", spec.display_name),
        ));
    }
}

fn codec_params(v: &VideoSettings, issues: &mut Vec<ProfileIssue>) {
    let spec = v.encoder.spec();
    let field = ProfileField::CodecParams;
    if spec.params_option.is_none() {
        if !v.codec_params.is_empty() {
            issues.push(ProfileIssue::error(
                field,
                format!("{} takes no codec parameters", spec.display_name),
            ));
        }
        return;
    }
    for (i, p) in v.codec_params.iter().enumerate() {
        if p.key.is_empty() || !p.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            issues.push(ProfileIssue::error(
                field,
                format!("`{}` is not a parameter name", p.key),
            ));
        }
        if p.value.is_empty() || p.value.contains([':', '=']) || p.value.chars().any(char::is_whitespace) {
            issues.push(ProfileIssue::error(
                field,
                format!("the value of `{}` must be non-empty without `:`, `=` or spaces", p.key),
            ));
        }
        if v.codec_params.iter().skip(i + 1).any(|q| q.key == p.key) {
            issues.push(ProfileIssue::error(field, format!("`{}` is set twice", p.key)));
        }
        if let Some(own) = own_field(v, &p.key) {
            issues.push(ProfileIssue::error(
                field,
                format!("`{}` is set twice ({own} and parameter)", p.key),
            ));
        } else if spec.reserved_params.contains(&p.key.as_str()) {
            issues.push(ProfileIssue::error(
                field,
                format!(
                    "`{}` conflicts with RustCut's own settings; use the fields above",
                    p.key
                ),
            ));
        }
    }
}

/// The field that already sets parameter `key`, when that field is in use (unused fields leave the key to the
/// passthrough: profiles of 0.0.8 set variance boost that way).
fn own_field(v: &VideoSettings, key: &str) -> Option<&'static str> {
    let spec = v.encoder.spec();
    let tune = match spec.tune_target {
        TuneTarget::CodecParam(k) => k,
        TuneTarget::Option => "tune",
    };
    let aq = match spec.aq.map(|a| a.setting) {
        Some(Setting::Param(k)) => k,
        Some(Setting::Option(_)) | None => "aq-mode",
    };
    let keyint = if v.encoder == VideoEncoder::Aom {
        "kf-max-dist"
    } else {
        "keyint"
    };
    let level = v.encoder.level_param().unwrap_or("level");
    let fields: [(bool, &[&str], &'static str); 8] = [
        (v.film_grain.is_some(), &["film-grain"], "film grain"),
        (v.tune.is_some(), &[tune], "tune"),
        (
            v.variance_boost.is_some(),
            &["enable-variance-boost", "variance-boost-strength"],
            "variance boost",
        ),
        (v.aq_mode.is_some(), &[aq], "AQ mode"),
        (v.scene_cuts.is_some(), &["scenecut"], "scene cuts"),
        (v.level.is_some(), &[level], "level"),
        (v.keyint.is_some(), &[keyint], "keyframe interval"),
        (v.profile.is_some(), &["profile"], "profile"),
    ];
    fields
        .into_iter()
        .find(|(used, keys, _)| *used && keys.contains(&key))
        .map(|(_, _, name)| name)
}

fn audio(profile: &RenderProfile, issues: &mut Vec<ProfileIssue>) {
    let a = &profile.audio;
    let field = ProfileField::Audio;
    if let Some(rate) = a.bitrate_kbps
        && (rate == 0 || !a.codec.takes_bitrate())
    {
        let message = if rate == 0 {
            "the audio bit rate must be above 0".to_owned()
        } else {
            format!("{} takes no bit rate", a.codec.spec().display_name)
        };
        issues.push(ProfileIssue::warning(field, message));
    }
    if a.channels == Some(0) {
        issues.push(ProfileIssue::error(field, "the channel count must be above 0"));
    } else if a.channels.is_some_and(|c| c > 8) {
        issues.push(ProfileIssue::warning(
            field,
            "more than 8 audio channels play almost nowhere",
        ));
    }
    if let Some(rate) = a.sample_rate {
        if !SAMPLE_RATES.contains(&rate) {
            issues.push(ProfileIssue::error(
                field,
                format!("a sample rate of {rate} Hz is not usable (8000 to 192000)"),
            ));
        } else if a.codec == AudioCodec::Opus {
            issues.push(ProfileIssue::warning(
                field,
                "Opus always uses 48 kHz; the sample rate is ignored",
            ));
        } else if a.codec == AudioCodec::Copy {
            issues.push(ProfileIssue::warning(field, "copied audio keeps its sample rate"));
        }
    }
    if a.codec == AudioCodec::Copy && a.channels.is_some() {
        issues.push(ProfileIssue::warning(field, "copied audio keeps its channels"));
    }
}

/// Only the issues of `severity`.
pub fn issues_of(issues: &[ProfileIssue], severity: Severity) -> impl Iterator<Item = &ProfileIssue> {
    issues.iter().filter(move |i| i.severity == severity)
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
