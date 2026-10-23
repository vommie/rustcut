//! A profile checked by the real encoder (ADR-14, "codec-params passthrough with validation"): a few frames of a test
//! picture plus silence are encoded with the profile's arguments into its container. A refused setting is an error; the
//! params encoders only warn about keys they do not know and then ignore them (x264 "Error parsing option", x265
//! "Unknown option", SVT-AV1 "Error parsing option", rav1e "Invalid value"), so those lines are errors too; the
//! encoder's other warnings (SVT-AV1's preset advice, for example) are passed on as warnings.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use super::runner::without_context;
use super::{
    EncodeContext, EncoderBackend, HardwareContext, JobLog, ProfileField, ProfileIssue, RateControl, RenderProfile,
    TEST_TIMEOUT, VideoPass, profile_args,
};
use crate::media::{ProbeError, run_tool};
use crate::store::LogStream;
use crate::time::Timestamp;

const MAX_OUTPUT: u64 = 1024 * 1024;
/// Stand-in bit rate for modes that need the job (two passes, target size).
const CHECK_KBPS: u32 = 2_000;
/// Warnings caused by the small test picture, not by the profile.
const NOISE: [&str; 2] = ["lookahead-slices", "Source height"];
/// What the params encoders say about keys or values they ignore.
const IGNORED_PARAM: [&str; 3] = ["Error parsing option", "Unknown option", "Invalid value for"];

/// The arguments of the check of `profile`, writing to `output`.
pub fn check_args(profile: &RenderProfile, hardware: &HardwareContext, output: &Path) -> Vec<OsString> {
    let mut checked = profile.clone();
    checked.video.rate_control = match checked.video.rate_control {
        RateControl::TwoPass { kbps } => RateControl::AverageBitrate { kbps },
        RateControl::TargetSize { .. } => RateControl::AverageBitrate { kbps: CHECK_KBPS },
        other @ (RateControl::Quality { .. } | RateControl::ConstantQp { .. } | RateControl::AverageBitrate { .. }) => {
            other
        }
    };
    let spec = checked.video.encoder.spec();
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "level+warning", "-y"]
        .map(OsString::from)
        .to_vec();
    if spec.backend == EncoderBackend::Vaapi
        && let Some(device) = &hardware.vaapi_device
    {
        args.push("-vaapi_device".into());
        args.push(device.as_os_str().to_owned());
    }
    args.extend(
        [
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=320x240:r=25:d=0.2,format=yuv420p",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-t",
            "0.2",
        ]
        .map(OsString::from),
    );
    if spec.backend == EncoderBackend::Vaapi {
        let pix_fmt = match checked.video.bit_depth {
            super::BitDepth::Eight => spec.pix_fmt_8,
            super::BitDepth::Ten => spec.pix_fmt_10.unwrap_or(spec.pix_fmt_8),
        };
        args.extend(["-vf".into(), format!("format={pix_fmt},hwupload").into()]);
    }
    let ctx = EncodeContext {
        duration: Timestamp::from_millis(200),
        audio_streams: 1,
        pass: VideoPass::Single,
    };
    // Single-pass arguments cannot fail: the rate control was made single-pass above.
    args.extend(
        profile_args(&checked, &ctx)
            .unwrap_or_default()
            .into_iter()
            .map(OsString::from),
    );
    let mut url = OsString::from("file:");
    url.push(output.as_os_str());
    args.push(url);
    args
}

/// Encodes the test with `ffmpeg` into `<folder>/check.<ext>` and reports what the encoder said. Fails only when
/// ffmpeg cannot run at all.
pub async fn check_profile(
    ffmpeg: &Path,
    profile: &RenderProfile,
    hardware: &HardwareContext,
    folder: &Path,
) -> Result<Vec<ProfileIssue>, ProbeError> {
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| ProbeError::Output(format!("{}: {e}", folder.display())))?;
    let output = folder.join(format!("check.{}", profile.container.extension()));
    let out = run_tool(ffmpeg, check_args(profile, hardware, &output), TEST_TIMEOUT, MAX_OUTPUT).await;
    if let Err(e) = tokio::fs::remove_file(&output).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::debug!(error = %e, "[Render] The check output was not removed");
    }
    let out = out?;
    Ok(read_report(profile, out.status.success(), &out.stderr))
}

/// The issues in ffmpeg's report of a check (`success` = its exit status).
pub fn read_report(profile: &RenderProfile, success: bool, stderr: &str) -> Vec<ProfileIssue> {
    let name = profile.video.encoder.spec().display_name;
    let mut issues = Vec::new();
    if !success {
        let mut log = JobLog::new();
        for line in stderr.lines() {
            log.push(LogStream::Ffmpeg, line);
        }
        let cause = log.last_error().unwrap_or_else(|| "no message".to_owned());
        issues.push(ProfileIssue::error(
            ProfileField::Encoder,
            format!("{name} refuses these settings: {cause}"),
        ));
        return issues;
    }
    let mut seen = BTreeSet::new();
    for line in stderr.lines() {
        let text = without_context(line).trim();
        let text = text.strip_prefix("[warning]").map_or(text, str::trim);
        if text.is_empty() || NOISE.iter().any(|n| text.contains(n)) || !seen.insert(text.to_owned()) {
            continue;
        }
        if IGNORED_PARAM.iter().any(|p| text.contains(p)) {
            // The level travels as a codec parameter too; anything else came from the passthrough.
            let level = profile.video.level.is_some()
                && profile
                    .video
                    .encoder
                    .level_param()
                    .is_some_and(|key| text.contains(&format!("'{key} ")));
            let field = if level {
                ProfileField::Level
            } else {
                ProfileField::CodecParams
            };
            issues.push(ProfileIssue::error(
                field,
                format!("{name} ignores a parameter: {text}"),
            ));
        } else if let Some(warning) = encoder_warning(line, text) {
            issues.push(ProfileIssue::warning(
                ProfileField::Encoder,
                format!("{name}: {warning}"),
            ));
        }
    }
    issues
}

/// The message of an encoder warning (`Svt[warn]: ...`, `x265 [warning]: ...`, `[libx264 @ 0x..] [warning] ...`).
fn encoder_warning<'a>(line: &'a str, text: &'a str) -> Option<&'a str> {
    let line = line.trim();
    for prefix in ["Svt[warn]:", "x265 [warning]:", "x264 [warning]:"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    let from_encoder = [
        "[libx264",
        "[libx265",
        "[libsvtav1",
        "[libaom",
        "[librav1e",
        "_nvenc",
        "_vaapi",
        "_qsv",
    ]
    .iter()
    .any(|e| line.contains(e));
    (from_encoder && line.contains("[warning]")).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Severity, builtin_profiles};

    #[test]
    fn the_report() {
        let p = builtin_profiles().remove(0);
        let ok = "Svt[info]: SVT [version]\nSvt[warn]: It is recommended to not use Film Grain for presets greater \
                  than 6\n[libsvtav1 @ 0x55] [warning] Error parsing option bogus: 1.\nx265 [warning]: Source height \
                  < 720p; disabling lookahead-slices\n[out#0 @ 0x1] [warning] something about the muxer\n";
        let issues = read_report(&p, true, ok);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(issues[0].message.contains("Film Grain"), "{issues:?}");
        assert_eq!(
            (issues[1].field, issues[1].severity),
            (ProfileField::CodecParams, Severity::Error)
        );
        assert!(
            issues[1].message.ends_with("Error parsing option bogus: 1."),
            "{issues:?}"
        );

        let failed = "x264 [error]: invalid preset 'nope'\n[libx264 @ 0x56] [error] Error setting preset/tune nope/(null).\n\
                      [vost#0:0/libx264 @ 0x5] [enc:libx264 @ 0x6] [error] Error while opening encoder\n";
        let issues = read_report(&p, false, failed);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].field, ProfileField::Encoder);
        assert!(issues[0].message.contains("invalid preset 'nope'"), "{issues:?}");
    }

    #[test]
    fn the_arguments() {
        let mut p = builtin_profiles().remove(3);
        p.video.rate_control = RateControl::TargetSize { mebibytes: 10 };
        let args: Vec<String> = check_args(&p, &HardwareContext::default(), Path::new("/c/check.mp4"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        insta::assert_snapshot!(args.join(" "));
    }
}
