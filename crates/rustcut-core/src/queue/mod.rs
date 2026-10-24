//! The queue engine (M2.6): it owns the jobs, schedules them on N workers, persists through the store actor and
//! publishes [`QueueEvent`]s that the `UiDispatcher` coalesces for the queue panel. This file is the frozen interface;
//! [`model`] is the pure state (scheduler, moves), [`actor`] runs it.

mod actor;
mod model;
#[cfg(test)]
mod model_tests;
mod summary;

use std::path::PathBuf;

use crate::actor::Reply;
use crate::job::{JobId, JobMetadata, JobSpec, JobState};
use crate::render::RenderEnv;
use crate::render::RenderProfile;
use crate::rustview::SyncState;
use crate::settings::WhenDone;
use crate::store::LogLine;
use crate::time::Timestamp;

pub use actor::{
    QueueHandle, QueueOptions, QueueService, RECOVERED_NOTE, STOPPED_NOTE, SyncLink, spawn_queue, spawn_queue_linked,
    validate as validate_spec,
};
pub use model::{MAX_WORKERS, Model, Refused, Tally, moved};
pub use summary::{StaleFiles, summarize};

/// Where selected jobs move to. Several selected jobs keep their relative order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    Up,
    Down,
    Top,
    Bottom,
    /// In front of this job (drag and drop); at the end when it is one of the moved jobs or unknown.
    Before(JobId),
}

/// One row of the queue panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSummary {
    pub id: JobId,
    pub state: JobState,
    pub attempt: u32,
    /// Target stem (what the row shows as the name).
    pub name: String,
    pub target: PathBuf,
    pub source: PathBuf,
    pub profile: String,
    pub sections: usize,
    pub duration: Timestamp,
    pub note: Option<String>,
    /// Unix milliseconds.
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub sync: SyncState,
}

/// Progress of a rendering job (coalesced by the UI to about 10 Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JobProgress {
    pub id: JobId,
    pub pass: u8,
    pub passes: u8,
    /// Output time reached in this pass, and the pass's total.
    pub done: Timestamp,
    pub total: Timestamp,
    pub fps: f32,
    /// Encoding speed relative to real time (ffmpeg's `speed=`).
    pub speed: f32,
    pub size_bytes: u64,
    /// Estimate for the whole job (all passes), seconds.
    pub eta_secs: Option<u32>,
}

impl JobProgress {
    /// Fraction of the whole job, 0 to 1 (passes weigh equally).
    pub fn fraction(&self) -> f64 {
        let total = self.total.as_micros().max(1) as f64;
        let in_pass = (self.done.as_micros().max(0) as f64 / total).min(1.0);
        let passes = f64::from(self.passes.max(1));
        ((f64::from(self.pass.max(1)) - 1.0 + in_pass) / passes).clamp(0.0, 1.0)
    }
}

/// The processing switches of the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingState {
    pub paused: bool,
    /// Finish the running jobs, then pause.
    pub pause_after_current: bool,
    pub workers: u8,
    pub running: Vec<JobId>,
    pub when_done: WhenDone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub jobs: Vec<JobSummary>,
    pub processing: ProcessingState,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("job {0} does not exist")]
    NoSuchJob(JobId),
    #[error("job {0} was written by another RustCut version and cannot be read")]
    Unreadable(JobId),
    #[error("job {id} cannot be changed while it is {state}")]
    NotEditable { id: JobId, state: crate::job::JobStateKind },
    #[error("the source and the target are the same file")]
    SameSourceAndTarget,
    #[error("the job has no section")]
    NoSections,
    #[error("the render profile is not usable: {0}")]
    InvalidProfile(String),
    #[error("the queue could not be saved: {0}")]
    Store(String),
}

/// Commands of the queue engine (frozen interface; new commands are added, existing ones keep their shape).
#[derive(Debug)]
pub enum QueueCommand {
    /// Adds a job at the end (held = postponed right away).
    Enqueue {
        spec: Box<JobSpec>,
        hold: bool,
        reply: Reply<Result<JobId, QueueError>>,
    },
    /// Replaces the spec of a waiting or held job ("Update job" in the editor).
    UpdateSpec {
        id: JobId,
        spec: Box<JobSpec>,
        reply: Reply<Result<(), QueueError>>,
    },
    Move {
        ids: Vec<JobId>,
        to: MoveTarget,
    },
    Hold {
        ids: Vec<JobId>,
    },
    Release {
        ids: Vec<JobId>,
    },
    /// Retry / reset / requeue: done jobs become waiting again as a new attempt, optionally with another profile.
    Restart {
        ids: Vec<JobId>,
        profile: Option<Box<RenderProfile>>,
    },
    Duplicate {
        ids: Vec<JobId>,
        reply: Reply<Result<Vec<JobId>, QueueError>>,
    },
    /// Deletes jobs that are not rendering (their RustView stubs are cleaned up; source sidecars are never touched).
    Remove {
        ids: Vec<JobId>,
        reply: Reply<Result<usize, QueueError>>,
    },
    RemoveFinished {
        reply: Reply<Result<usize, QueueError>>,
    },
    /// Stops rendering jobs (the temp file is deleted, an existing target stays) and cancels waiting ones.
    Cancel {
        ids: Vec<JobId>,
    },
    SetPaused(bool),
    SetPauseAfterCurrent(bool),
    /// The user wants to quit (RustCut's close question is open): no new job starts while it is on, the running ones
    /// go on; off lets the queue go on. Not stored, not part of `ProcessingState`.
    HoldStarts(bool),
    SetWorkers(u8),
    SetWhenDone(WhenDone),
    Snapshot {
        reply: Reply<QueueSnapshot>,
    },
    Log {
        id: JobId,
        limit: usize,
        reply: Reply<Result<Vec<LogLine>, QueueError>>,
    },
    /// The ffmpeg path, work folders, hardware and priorities for renders started from now on (settings changed, the
    /// capability check found a VAAPI device).
    SetRenderEnv(Box<RenderEnv>),
    /// The RustView metadata (tags, rating, source hash) a job wants (the Tag & Rate panel edits it, M3.2).
    Metadata {
        id: JobId,
        reply: Reply<Result<JobMetadata, QueueError>>,
    },
    /// Replaces the RustView metadata of a job in any state (user decision Q-G3). A job still to render re-syncs as
    /// edited, a finished job's record is written once more (its file exists), a failed or canceled job only keeps the
    /// values for a restart.
    UpdateMetadata {
        id: JobId,
        metadata: JobMetadata,
        reply: Reply<Result<(), QueueError>>,
    },
    /// The whole spec of a job in any state (the Job panel shows it, the editor loads it; M4.1).
    Spec {
        id: JobId,
        reply: Reply<Result<Box<JobSpec>, QueueError>>,
    },
    /// Stops rendering jobs (they wait again at the next start) and ends the actor.
    Shutdown {
        reply: Reply<()>,
    },
}

/// Events of the queue engine.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueEvent {
    Added(Box<JobSummary>),
    Updated(Box<JobSummary>),
    Removed(Vec<JobId>),
    /// The complete new order of all jobs.
    Reordered(Vec<JobId>),
    Progress(JobProgress),
    Processing(ProcessingState),
    /// The last waiting job finished; the "when done" action follows.
    Done {
        finished: u32,
        failed: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(pass: u8, passes: u8, done: i64, total: i64) -> JobProgress {
        JobProgress {
            id: JobId(1),
            pass,
            passes,
            done: Timestamp::from_secs(done),
            total: Timestamp::from_secs(total),
            fps: 0.0,
            speed: 0.0,
            size_bytes: 0,
            eta_secs: None,
        }
    }

    #[test]
    fn fraction_spans_all_passes() {
        assert!((progress(1, 1, 5, 10).fraction() - 0.5).abs() < 1e-9);
        assert!((progress(2, 2, 5, 10).fraction() - 0.75).abs() < 1e-9);
        assert!(
            (progress(1, 2, 20, 10).fraction() - 0.5).abs() < 1e-9,
            "a pass never counts more than 100 %"
        );
        assert!((progress(0, 0, 0, 0).fraction()).abs() < 1e-9);
    }
}
