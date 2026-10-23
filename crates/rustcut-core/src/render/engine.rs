//! The render of one job from start to end (ADR-3, ADR-9): probe the source again, plan the passes, run them under
//! supervision, verify the temp file, put it onto the target, and remove what is left. The queue engine (M2.6) drives
//! this per job; the frozen `queue` interface gets its progress as [`JobProgress`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::watch;

use super::output::{FinalizeError, Leftovers, finalize};
use super::verify::{VerifyError, verify_file};
use super::{
    Control, HardwareContext, JobLog, PassError, PassEvent, PassKind, PassOptions, PlanError, PlanInput,
    RenderPriority, WorkDirs, plan, run_pass,
};
use crate::job::{JobId, JobSpec};
use crate::media::{ProbeError, ProbeOptions, probe};
use crate::queue::JobProgress;
use crate::settings::{AppPaths, Settings};
use crate::time::{Precision, Timestamp};

/// The program paths, folders and limits renders use (from the settings and the capability check).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderEnv {
    pub ffmpeg: PathBuf,
    pub probe: ProbeOptions,
    pub dirs: WorkDirs,
    pub hardware: HardwareContext,
    pub priority: RenderPriority,
    pub stall_timeout: Option<Duration>,
}

impl RenderEnv {
    pub fn new(settings: &Settings, paths: &AppPaths, hardware: HardwareContext) -> Self {
        let stall = u64::from(settings.render.stall_timeout_secs);
        Self {
            ffmpeg: settings
                .advanced
                .ffmpeg
                .clone()
                .unwrap_or_else(|| PathBuf::from("ffmpeg")),
            probe: ProbeOptions {
                program: settings.advanced.ffprobe.clone(),
                ..ProbeOptions::default()
            },
            dirs: WorkDirs {
                deshake: paths.deshake_dir(),
                passlog: paths.passlog_dir(),
            },
            hardware,
            priority: RenderPriority {
                nice: settings.render.nice,
                idle_io: settings.render.idle_io,
            },
            stall_timeout: (stall > 0).then(|| Duration::from_secs(stall)),
        }
    }
}

/// What a running render reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderUpdate {
    /// Pass `pass` of `passes` (1-based) starts.
    Pass {
        pass: u8,
        passes: u8,
        kind: PassKind,
    },
    Progress(JobProgress),
    /// ffmpeg was stopped (`true`) or continued (`false`).
    Paused(bool),
    /// The passes are done; the temp file is being checked.
    Verifying,
}

/// A finished, verified output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("the source cannot be read: {0}")]
    Source(ProbeError),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error("the target folder {0} does not exist")]
    TargetFolder(PathBuf),
    #[error("{0} exists already and overwriting was not chosen")]
    TargetExists(PathBuf),
    #[error("a work folder could not be created: {0}")]
    WorkFolder(String),
    #[error("{kind} failed: {error}", kind = kind.label())]
    Pass { kind: PassKind, error: PassError },
    #[error("the render was not saved: {0}")]
    Verify(#[from] VerifyError),
    #[error(transparent)]
    Finalize(#[from] FinalizeError),
}

impl RenderError {
    /// The owner canceled the render (not a failure of the job).
    pub fn is_canceled(&self) -> bool {
        matches!(
            self,
            Self::Pass {
                error: PassError::Canceled,
                ..
            }
        )
    }
}

/// Renders `spec` as job `job`. `control` pauses, resumes or cancels it; `updates` gets the pass changes and the progress.
/// A failure or a cancel leaves an existing target untouched and deletes the temp file and the side files.
pub async fn render_job(
    job: JobId,
    spec: &JobSpec,
    env: &RenderEnv,
    mut control: watch::Receiver<Control>,
    log: &mut JobLog,
    mut updates: impl FnMut(RenderUpdate) + Send,
) -> Result<RenderedFile, RenderError> {
    let target = spec.target_path();
    log.note(&format!(
        "[Render] Job {job}: {} -> {}",
        spec.source.display(),
        target.display()
    ));
    let result = render(job, spec, env, &mut control, log, &mut updates).await;
    match &result {
        Ok(done) => {
            tracing::info!(job = job.0, target = %done.path.display(), bytes = done.bytes, "[Render] Finished");
            log.note(&format!(
                "[Render] Finished: {} ({} bytes)",
                done.path.display(),
                done.bytes
            ));
        }
        Err(e) if e.is_canceled() => {
            tracing::info!(job = job.0, "[Render] Canceled");
            log.note("[Render] Canceled; the temp file was removed");
        }
        Err(e) => {
            tracing::warn!(job = job.0, error = %e, "[Render] Failed");
            log.note(&format!("[Render] Failed: {e}"));
        }
    }
    result
}

async fn render(
    job: JobId,
    spec: &JobSpec,
    env: &RenderEnv,
    control: &mut watch::Receiver<Control>,
    log: &mut JobLog,
    updates: &mut (impl FnMut(RenderUpdate) + Send),
) -> Result<RenderedFile, RenderError> {
    let source = probe(&spec.source, &env.probe).await.map_err(RenderError::Source)?;
    let plan = plan(PlanInput {
        job,
        spec,
        source: &source,
        dirs: &env.dirs,
        hardware: &env.hardware,
    })?;
    let leftovers = Leftovers::new(
        std::iter::once(plan.temp.clone())
            .chain(plan.transforms.clone())
            .collect(),
        plan.passlog.iter().cloned().collect(),
    );
    let result = run_plan(job, spec, env, &plan, control, log, updates).await;
    leftovers.remove().await;
    result
}

async fn run_plan(
    job: JobId,
    spec: &JobSpec,
    env: &RenderEnv,
    plan: &super::RenderPlan,
    control: &mut watch::Receiver<Control>,
    log: &mut JobLog,
    updates: &mut (impl FnMut(RenderUpdate) + Send),
) -> Result<RenderedFile, RenderError> {
    if !is_folder(&spec.target.folder).await {
        return Err(RenderError::TargetFolder(spec.target.folder.clone()));
    }
    if !spec.target.overwrite && tokio::fs::try_exists(&plan.target).await.unwrap_or(true) {
        return Err(RenderError::TargetExists(plan.target.clone()));
    }
    for folder in plan
        .transforms
        .iter()
        .filter_map(|t| t.parent())
        .chain(plan.passlog.as_deref())
    {
        tokio::fs::create_dir_all(folder)
            .await
            .map_err(|e| RenderError::WorkFolder(format!("{}: {e}", folder.display())))?;
    }
    let options = PassOptions {
        program: &env.ffmpeg,
        priority: env.priority,
        stall_timeout: env.stall_timeout,
    };
    let passes = plan.pass_count();
    for (number, pass) in (1..=passes).zip(&plan.passes) {
        updates(RenderUpdate::Pass {
            pass: number,
            passes,
            kind: pass.kind,
        });
        let total = pass.duration;
        let events = |event| match event {
            PassEvent::Progress(sample) => updates(RenderUpdate::Progress(JobProgress {
                id: job,
                pass: number,
                passes,
                done: sample.out_time.unwrap_or_default().min(total),
                total,
                fps: sample.fps.unwrap_or_default(),
                speed: sample.speed.unwrap_or_default(),
                size_bytes: sample.total_size.unwrap_or_default(),
                eta_secs: estimate_eta(number, passes, sample.out_time.unwrap_or_default(), total, sample.speed),
            })),
            PassEvent::Paused(paused) => updates(RenderUpdate::Paused(paused)),
        };
        run_pass(pass, &options, control, log, events)
            .await
            .map_err(|error| RenderError::Pass { kind: pass.kind, error })?;
    }
    updates(RenderUpdate::Verifying);
    let bytes = verify_file(&plan.temp, &plan.expected, &env.probe).await?;
    log.note(&format!(
        "[Render] Verified: {bytes} bytes, {} with {} audio tracks",
        plan.expected.duration.format(Precision::Millis),
        plan.expected.audio_streams
    ));
    finalize(&plan.temp, &plan.target, spec.target.overwrite).await?;
    Ok(RenderedFile {
        path: plan.target.clone(),
        bytes,
    })
}

async fn is_folder(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_dir())
}

/// Seconds left for the whole job: the rest of this pass and the passes after it, at the current speed (ffmpeg's
/// `speed` = output seconds per second). `None` while the speed is unknown or about zero.
pub fn estimate_eta(pass: u8, passes: u8, done: Timestamp, total: Timestamp, speed: Option<f32>) -> Option<u32> {
    let speed = f64::from(speed.filter(|s| *s > 0.01)?);
    let left_here = total.saturating_sub(done.min(total)).as_secs_f64();
    let later = f64::from(passes.saturating_sub(pass)) * total.as_secs_f64();
    let secs = ((left_here + later) / speed).ceil();
    az::checked_cast::<f64, u32>(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_covers_the_later_passes() {
        let t = Timestamp::from_secs;
        assert_eq!(estimate_eta(1, 1, t(30), t(60), Some(2.0)), Some(15));
        // Pass 1 of 3 at half way: 30 s left here plus two passes of 60 s, at 1x.
        assert_eq!(estimate_eta(1, 3, t(30), t(60), Some(1.0)), Some(150));
        assert_eq!(
            estimate_eta(3, 3, t(70), t(60), Some(1.0)),
            Some(0),
            "done beyond the total counts as done"
        );
        assert_eq!(estimate_eta(1, 1, t(0), t(60), None), None);
        assert_eq!(estimate_eta(1, 1, t(0), t(60), Some(0.0)), None);
    }

    #[test]
    fn the_environment_follows_the_settings() {
        let mut settings = Settings::default();
        settings.render.nice = 15;
        settings.render.stall_timeout_secs = 0;
        settings.advanced.ffmpeg = Some(PathBuf::from("/opt/ffmpeg/bin/ffmpeg"));
        let env = RenderEnv::new(
            &settings,
            &AppPaths::below(Path::new("/scratch")),
            HardwareContext::default(),
        );
        assert_eq!(env.ffmpeg, PathBuf::from("/opt/ffmpeg/bin/ffmpeg"));
        assert_eq!(env.priority.nice, 15);
        assert_eq!(env.stall_timeout, None);
        assert_eq!(env.dirs.deshake, PathBuf::from("/scratch/data/deshake"));
        assert_eq!(
            RenderEnv::new(
                &Settings::default(),
                &AppPaths::below(Path::new("/s")),
                HardwareContext::default()
            )
            .stall_timeout,
            Some(Duration::from_secs(300))
        );
    }
}
