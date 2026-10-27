//! The probe render (ADR-15, user decision Q-E1): a few seconds around the playhead with the current filters and the
//! working profile, rendered by [`super::render_job`] into the cache folder and played in the main player. It is the only
//! realistic preview of deshake (two passes) and shows what the encoder does to the picture. By default it uses the
//! encoder's fastest speed; "exact" keeps the profile's own speed and then also estimates the job's size.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use super::args::target_kbps;
use super::{EncodeContext, RateControl, RenderEnv, RenderProfile, Speed, VideoPass, WorkDirs};
use crate::edit::{FilterChain, Section};
use crate::job::{JOB_SPEC_VERSION, JobId, JobMetadata, JobSpec, TargetSpec};
use crate::time::Timestamp;

/// The job id of probe renders: real jobs start at 1 (`AUTOINCREMENT`), and the probe's side files live in its own
/// folder, so nothing collides.
pub const PROBE_JOB: JobId = JobId(0);
/// Default length of a probe in seconds.
pub const DEFAULT_PROBE_SECS: u16 = 10;
/// Lengths a probe may have (seconds).
pub const PROBE_SECS: RangeInclusive<u16> = 2..=60;
/// File name stem of the probe output.
const PROBE_STEM: &str = "probe";

/// What a probe render needs.
#[derive(Debug, Clone, Copy)]
pub struct ProbeInput<'a> {
    pub source: &'a Path,
    /// Length of the source.
    pub duration: Timestamp,
    pub playhead: Timestamp,
    pub seconds: u16,
    pub filters: &'a FilterChain,
    /// The final size rule the jobs get (Settings > Filters > Final size), so the probe shows their size.
    pub final_multiple: u8,
    pub profile: &'a RenderProfile,
    pub exact: bool,
    /// Length of the job (the sections, or the whole source): a target size needs it.
    pub job_duration: Timestamp,
    /// The probe folder ([`probe_dir`]).
    pub folder: &'a Path,
}

/// A probe ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeJob {
    pub spec: JobSpec,
    /// The part of the source it shows.
    pub window: Section,
}

/// `<cache>/probe`.
pub fn probe_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("probe")
}

/// The part of the source a probe shows: `seconds` long (at most the source), starting a quarter of it before the
/// playhead, moved inside the source. `None` for a source without length.
pub fn probe_window(playhead: Timestamp, seconds: u16, duration: Timestamp) -> Option<Section> {
    let wanted = Timestamp::from_secs(i64::from(seconds.clamp(*PROBE_SECS.start(), *PROBE_SECS.end())));
    let length = wanted.min(duration);
    if length.as_micros() <= 0 {
        return None;
    }
    let lead = Timestamp::from_micros(length.as_micros() / 4);
    let latest = duration.saturating_sub(length);
    let start = playhead.saturating_sub(lead).max(Timestamp::ZERO).min(latest);
    Section::new(start, start.saturating_add(length)).ok()
}

/// The profile a probe renders with: one pass (a bit rate instead of two passes; a target size becomes the bit rate
/// it means for the whole job), and unless `exact` the encoder's fastest speed when that is faster than the profile's.
pub fn probe_profile(profile: &RenderProfile, exact: bool, job_duration: Timestamp) -> RenderProfile {
    let mut probe = profile.clone();
    let v = &mut probe.video;
    let spec = v.encoder.spec();
    v.rate_control = match v.rate_control {
        RateControl::TwoPass { kbps } => RateControl::AverageBitrate { kbps },
        RateControl::TargetSize { mebibytes } => {
            let ctx = EncodeContext {
                duration: job_duration,
                audio_streams: 1,
                pass: VideoPass::Single,
            };
            target_kbps(mebibytes, &ctx, &profile.audio)
                .ok()
                .and_then(|kbps| u32::try_from(kbps).ok())
                .map_or(
                    RateControl::Quality {
                        value: spec.quality.default,
                    },
                    |kbps| RateControl::AverageBitrate { kbps },
                )
        }
        other @ (RateControl::Quality { .. } | RateControl::ConstantQp { .. } | RateControl::AverageBitrate { .. }) => {
            other
        }
    };
    if !exact && let Some(fast) = spec.speed_preset(Speed::Fastest) {
        // Presets are listed slowest first: a later one is faster.
        let index = |value: &str| spec.presets.iter().position(|p| p.value == value);
        let current = v.effective_preset().and_then(index);
        if index(fast) > current {
            v.preset = Some(fast.to_owned());
        }
    }
    probe
}

/// The job of a probe: the window around the playhead with the filters, written to `probe.<ext>` in the probe folder
/// (replacing the last probe). `None` when the source has no length.
pub fn probe_job(input: &ProbeInput<'_>) -> Option<ProbeJob> {
    let window = probe_window(input.playhead, input.seconds, input.duration)?;
    let spec = JobSpec {
        version: JOB_SPEC_VERSION,
        source: input.source.to_path_buf(),
        source_info: None,
        sections: vec![window].into(),
        filters: input.filters.clone(),
        final_multiple: input.final_multiple,
        profile: probe_profile(input.profile, input.exact, input.job_duration),
        target: TargetSpec {
            folder: input.folder.to_path_buf(),
            stem: PROBE_STEM.to_owned(),
            overwrite: true,
            naming: None,
        },
        metadata: JobMetadata::default(),
    };
    Some(ProbeJob { spec, window })
}

/// The render environment of probes: like the jobs', with the side files (deshake transforms, pass logs) in the probe
/// folder.
pub fn probe_env(env: &RenderEnv, folder: &Path) -> RenderEnv {
    RenderEnv {
        dirs: WorkDirs {
            deshake: folder.to_path_buf(),
            passlog: folder.join("passlog"),
        },
        ..env.clone()
    }
}

/// Empties the probe folder (creates it when missing): earlier probes, also of other containers, and side files of an
/// interrupted one. Errors are logged; a probe then still overwrites its own file.
pub async fn clear_probe_dir(folder: &Path) {
    // A probe render a crashed RustCut left behind may still write here (this instance's own runs are left alone).
    let owned = folder.to_path_buf();
    if let Err(e) = tokio::task::spawn_blocking(move || super::stop_orphans(&[owned])).await {
        tracing::warn!(error = %e, "[Render] The search for orphaned probe renders failed");
    }
    if let Err(e) = tokio::fs::create_dir_all(folder).await {
        tracing::warn!(folder = %folder.display(), error = %e, "[Render] The probe folder could not be created");
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(folder).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let removed = match entry.file_type().await {
            Ok(kind) if kind.is_dir() => tokio::fs::remove_dir_all(&path).await,
            Ok(_) => tokio::fs::remove_file(&path).await,
            Err(e) => Err(e),
        };
        if let Err(e) = removed {
            tracing::debug!(path = %path.display(), error = %e, "[Render] Old probe file not removed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{VideoEncoder, builtin_profiles};

    fn t(secs: i64) -> Timestamp {
        Timestamp::from_secs(secs)
    }

    #[test]
    fn the_window_leads_in_and_stays_inside() {
        let w = |playhead, seconds, duration| {
            probe_window(t(playhead), seconds, t(duration))
                .map(|s| (s.start().as_micros() / 1_000_000, s.end().as_micros() / 1_000_000))
        };
        assert_eq!(w(100, 8, 600), Some((98, 106)), "a quarter before the playhead");
        assert_eq!(w(1, 8, 600), Some((0, 8)));
        assert_eq!(w(599, 8, 600), Some((592, 600)));
        assert_eq!(w(3, 10, 6), Some((0, 6)), "a short source is probed whole");
        assert_eq!(w(0, 1, 600), Some((0, 2)), "at least two seconds");
        assert_eq!(w(0, 10, 0), None);
    }

    #[test]
    fn the_probe_profile_is_one_fast_pass() {
        let base = builtin_profiles().remove(0);
        let fast = probe_profile(&base, false, t(60));
        assert_eq!(fast.video.preset.as_deref(), Some("10"));
        assert_eq!(probe_profile(&base, true, t(60)).video.preset.as_deref(), Some("6"));

        let mut x264 = base;
        x264.set_encoder(VideoEncoder::X264);
        x264.video.preset = Some("ultrafast".into());
        assert_eq!(
            probe_profile(&x264, false, t(60)).video.preset.as_deref(),
            Some("ultrafast"),
            "never slower than the profile"
        );
        x264.video.rate_control = RateControl::TwoPass { kbps: 4000 };
        assert_eq!(
            probe_profile(&x264, false, t(60)).video.rate_control,
            RateControl::AverageBitrate { kbps: 4000 }
        );
        // 60 MiB over 60 s: 8388 kbit/s minus the audio.
        x264.video.rate_control = RateControl::TargetSize { mebibytes: 60 };
        let audio = x264.audio.bitrate_kbps.unwrap();
        assert_eq!(
            probe_profile(&x264, false, t(60)).video.rate_control,
            RateControl::AverageBitrate { kbps: 8388 - audio }
        );
        assert!(matches!(
            probe_profile(&x264, false, Timestamp::ZERO).video.rate_control,
            RateControl::Quality { .. }
        ));
    }

    #[test]
    fn the_job_writes_into_the_probe_folder() {
        let profile = builtin_profiles().remove(3);
        let filters = FilterChain::default();
        let folder = probe_dir(Path::new("/cache"));
        let job = probe_job(&ProbeInput {
            source: Path::new("/videos/in.mkv"),
            duration: t(600),
            playhead: t(60),
            seconds: 10,
            filters: &filters,
            final_multiple: 2,
            profile: &profile,
            exact: false,
            job_duration: t(600),
            folder: &folder,
        })
        .unwrap();
        assert_eq!(job.spec.target_path(), PathBuf::from("/cache/probe/probe.mp4"));
        assert!(job.spec.target.overwrite);
        assert_eq!(job.spec.output_duration(), t(10));
        assert_eq!(job.window.start(), Timestamp::from_millis(57_500));
        let env = probe_env(
            &RenderEnv::new(
                &crate::settings::Settings::default(),
                &crate::settings::AppPaths::below(Path::new("/p")),
                super::super::HardwareContext::default(),
            ),
            &folder,
        );
        assert_eq!(env.dirs.deshake, folder);
    }

    #[tokio::test]
    async fn clearing_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = probe_dir(dir.path());
        clear_probe_dir(&folder).await;
        std::fs::write(folder.join("probe.mkv"), "x").unwrap();
        std::fs::create_dir_all(folder.join("passlog/job-0")).unwrap();
        clear_probe_dir(&folder).await;
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }
}
