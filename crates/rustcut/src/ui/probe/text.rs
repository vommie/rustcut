//! The words of the probe render: its progress, its result with the size estimate, and the banner over the video
//! (pure, tested).

use rustcut_core::media::format_size;
use rustcut_core::render::{PassKind, RenderProfile, scale_size};
use rustcut_core::time::{Precision, Timestamp};

/// `0:12.000-0:22.000` without the hours when they are zero.
pub fn span(start: Timestamp, end: Timestamp) -> String {
    format!("{}-{}", short(start), short(end))
}

/// A time without leading `0:` hours and with tenths (`1:02.5`, `1:00:00.0`).
pub fn short(t: Timestamp) -> String {
    let full = t.format(Precision::Millis);
    // `0:01:02.500` -> `1:02.500`; with hours the minutes keep their two digits.
    let trimmed = full.strip_prefix("0:").map_or(full.as_str(), |rest| {
        rest.strip_prefix('0').filter(|r| !r.starts_with(':')).unwrap_or(rest)
    });
    // Drop the last two digits of the milliseconds: tenths are enough here.
    trimmed
        .get(..trimmed.len().saturating_sub(2))
        .unwrap_or(trimmed)
        .to_owned()
}

/// "x264, veryfast" / "SVT-AV1, preset 10".
pub fn encoder_label(profile: &RenderProfile) -> String {
    let v = &profile.video;
    let spec = v.encoder.spec();
    let name = spec
        .display_name
        .split_once('(')
        .map_or(spec.display_name, |(_, inner)| inner.trim_end_matches(')'));
    match v.effective_preset() {
        Some(preset) if preset.chars().all(|c| c.is_ascii_digit()) => format!("{name}, preset {preset}"),
        Some(preset) => format!("{name}, {preset}"),
        None => name.to_owned(),
    }
}

/// The banner over the video while the probe plays.
pub fn banner(start: Timestamp, end: Timestamp, profile: &RenderProfile) -> String {
    format!("Probe render · {} · {}", span(start, end), encoder_label(profile))
}

/// What a running probe does now.
pub fn pass(pass: u8, passes: u8, kind: PassKind) -> String {
    if passes > 1 {
        format!("Pass {pass} of {passes}: {}", kind.label())
    } else {
        "Rendering".to_owned()
    }
}

/// The job's size from `bytes` written for `rendered` of a probe, as "about 42 MB for the job (3:20.0)".
pub fn job_estimate(bytes: u64, rendered: Timestamp, job: Timestamp) -> Option<String> {
    let total = scale_size(bytes, rendered, job)?;
    Some(format!("about {} for the job ({})", format_size(total), short(job)))
}

/// The result of a finished probe.
pub fn finished(bytes: u64, length: Timestamp, job: Timestamp, exact: bool) -> String {
    let size = format!("{} for {}", format_size(bytes), seconds(length));
    if exact {
        job_estimate(bytes, length, job).map_or(size.clone(), |estimate| format!("{size} · {estimate}"))
    } else {
        format!("{size} at the fastest speed (exact settings estimate the job's size)")
    }
}

/// `10 s`, `2.5 s`.
fn seconds(t: Timestamp) -> String {
    let tenths = (t.as_micros().max(0) + 50_000) / 100_000;
    if tenths % 10 == 0 {
        format!("{} s", tenths / 10)
    } else {
        format!("{}.{} s", tenths / 10, tenths % 10)
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::render::{VideoEncoder, builtin_profiles, probe_profile};

    use super::*;

    #[test]
    fn times_and_labels() {
        let t = Timestamp::from_millis;
        assert_eq!(span(t(57_500), t(67_500)), "0:57.5-1:07.5");
        assert_eq!(short(t(3_723_400)), "1:02:03.4");
        let mut p = builtin_profiles().remove(0);
        assert_eq!(encoder_label(&p), "SVT-AV1, preset 6");
        p.set_encoder(VideoEncoder::X264);
        let fast = probe_profile(&p, false, t(1_000));
        assert_eq!(encoder_label(&fast), "x264, veryfast");
        assert_eq!(
            banner(t(0), t(10_000), &fast),
            "Probe render · 0:00.0-0:10.0 · x264, veryfast"
        );
        p.set_encoder(VideoEncoder::HevcVaapi);
        assert_eq!(encoder_label(&p), "VAAPI");
    }

    #[test]
    fn progress_and_results() {
        assert_eq!(pass(1, 2, PassKind::DeshakeAnalysis), "Pass 1 of 2: deshake analysis");
        assert_eq!(pass(1, 1, PassKind::Encode), "Rendering");
        let t = Timestamp::from_secs;
        assert_eq!(
            finished(2_000_000, t(10), t(200), true),
            "2 MB for 10 s · about 40 MB for the job (3:20.0)"
        );
        assert_eq!(
            finished(2_000_000, Timestamp::from_millis(2_500), t(200), false),
            "2 MB for 2.5 s at the fastest speed (exact settings estimate the job's size)"
        );
    }
}
