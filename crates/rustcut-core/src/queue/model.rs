//! The queue as a pure model: the jobs in their order, the processing switches, which jobs start next and how a
//! selection moves. The actor ([`super::actor`]) persists what the model decides and publishes it; everything here is
//! tested without a runtime, a store or ffmpeg (property tests in `model_tests.rs`).

use std::path::Path;

use crate::job::{JobId, JobState, JobStateKind};
use crate::naming::same_path;
use crate::settings::WhenDone;
use crate::store::{JobRecord, SpecSlot};

use super::{MoveTarget, ProcessingState};

/// Most workers the queue runs at the same time (the settings are sanitized to this too).
pub const MAX_WORKERS: u8 = 16;

/// Why the model refused a state change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("job {0} does not exist")]
    NoSuchJob(JobId),
    #[error("job {id} cannot go from {from} to {to}")]
    Transition {
        id: JobId,
        from: JobStateKind,
        to: JobStateKind,
    },
}

/// Jobs that ended since the queue was last idle (for [`super::QueueEvent::Done`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub finished: u32,
    pub failed: u32,
}

impl Tally {
    pub const fn is_empty(self) -> bool {
        self.finished == 0 && self.failed == 0
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    jobs: Vec<JobRecord>,
    workers: u8,
    paused: bool,
    pause_after_current: bool,
    /// The user wants to quit (the close question is open): no new job starts, the running ones go on. Not stored.
    hold_starts: bool,
    when_done: WhenDone,
    tally: Tally,
}

impl Model {
    /// The jobs in queue order and the processing switches.
    pub fn new(jobs: Vec<JobRecord>, workers: u8, paused: bool, when_done: WhenDone) -> Self {
        Self {
            jobs,
            workers: workers.clamp(1, MAX_WORKERS),
            paused,
            pause_after_current: false,
            hold_starts: false,
            when_done,
            tally: Tally::default(),
        }
    }

    pub fn jobs(&self) -> &[JobRecord] {
        &self.jobs
    }

    pub fn get(&self, id: JobId) -> Option<&JobRecord> {
        self.jobs.iter().find(|j| j.id == id)
    }

    fn get_mut(&mut self, id: JobId) -> Option<&mut JobRecord> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn order(&self) -> Vec<JobId> {
        self.jobs.iter().map(|j| j.id).collect()
    }

    /// Jobs whose render runs (rendering or paused), in queue order.
    pub fn running(&self) -> Vec<JobId> {
        self.jobs
            .iter()
            .filter(|j| j.state.kind().is_active())
            .map(|j| j.id)
            .collect()
    }

    pub fn processing(&self) -> ProcessingState {
        ProcessingState {
            paused: self.paused,
            pause_after_current: self.pause_after_current,
            workers: self.workers,
            running: self.running(),
            when_done: self.when_done,
        }
    }

    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if !paused {
            self.pause_after_current = false;
        }
    }

    /// Finish the running jobs, then pause; with nothing running the queue pauses at once.
    pub fn set_pause_after_current(&mut self, on: bool) {
        self.pause_after_current = on;
        self.settle_pause();
    }

    /// While on, no new job starts (the running ones go on); off lets the queue go on as before.
    pub fn set_hold_starts(&mut self, on: bool) {
        self.hold_starts = on;
    }

    pub fn set_workers(&mut self, workers: u8) {
        self.workers = workers.clamp(1, MAX_WORKERS);
    }

    pub fn set_when_done(&mut self, when_done: WhenDone) {
        self.when_done = when_done;
    }

    /// "Pause after the current jobs" turns into a pause once nothing runs. `true` when it did.
    pub fn settle_pause(&mut self) -> bool {
        if self.pause_after_current && self.running().is_empty() {
            self.pause_after_current = false;
            self.paused = true;
            return true;
        }
        false
    }

    /// Adds a job at the end.
    pub fn push(&mut self, record: JobRecord) {
        self.jobs.push(record);
    }

    /// Replaces the stored row of a job (after the store stamped times or counted an attempt).
    pub fn replace(&mut self, record: JobRecord) {
        if let Some(slot) = self.get_mut(record.id) {
            *slot = record;
        }
    }

    /// Removes jobs that do not run; returns the ids that went.
    pub fn remove(&mut self, ids: &[JobId]) -> Vec<JobId> {
        let mut removed = Vec::new();
        self.jobs.retain(|j| {
            let goes = ids.contains(&j.id) && !j.state.kind().is_active();
            if goes {
                removed.push(j.id);
            }
            !goes
        });
        removed
    }

    /// Moves a job to `next` when the state machine allows it; returns the state it had.
    pub fn transition(&mut self, id: JobId, next: JobState, note: Option<String>) -> Result<JobState, Refused> {
        let job = self.get_mut(id).ok_or(Refused::NoSuchJob(id))?;
        if !job.state.can_become(&next) {
            return Err(Refused::Transition {
                id,
                from: job.state.kind(),
                to: next.kind(),
            });
        }
        job.note = note;
        Ok(std::mem::replace(&mut job.state, next))
    }

    /// Counts a job that ended (for the "queue done" event).
    pub fn count_end(&mut self, state: &JobState) {
        match state.kind() {
            JobStateKind::Finished => self.tally.finished = self.tally.finished.saturating_add(1),
            JobStateKind::Failed => self.tally.failed = self.tally.failed.saturating_add(1),
            _ => {}
        }
    }

    /// The jobs to start now: waiting jobs in queue order, as many as there are free workers, never one whose target
    /// is the target of a running job or of another job picked in the same round (they would share the temp file).
    /// A job that has to wait for its target does not stall the jobs behind it (PyCutRv bug 9).
    pub fn schedule(&self) -> Vec<JobId> {
        if self.paused || self.pause_after_current || self.hold_starts {
            return Vec::new();
        }
        let mut busy: Vec<&Path> = self
            .jobs
            .iter()
            .filter(|j| j.state.kind().is_active())
            .map(|j| j.target_path.as_path())
            .collect();
        let free = usize::from(self.workers).saturating_sub(busy.len());
        let mut picked = Vec::new();
        for job in self.jobs.iter().filter(|j| is_startable(j)) {
            if picked.len() >= free {
                break;
            }
            if busy.iter().any(|t| same_path(t, &job.target_path)) {
                continue;
            }
            busy.push(&job.target_path);
            picked.push(job.id);
        }
        picked
    }

    /// Nothing runs and no waiting job can start: the queue has done what it can. Returns the jobs that ended since the
    /// last time it was idle (once; empty when nothing ended).
    pub fn take_done(&mut self) -> Option<Tally> {
        let busy = self.jobs.iter().any(|j| j.state.kind().is_active() || is_startable(j));
        if busy || self.tally.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.tally))
    }

    /// Applies a move of `ids` and returns the new order, or `None` when nothing changed.
    pub fn reorder(&mut self, ids: &[JobId], to: MoveTarget) -> Option<Vec<JobId>> {
        let before = self.order();
        let after = moved(&before, ids, to);
        if after == before {
            return None;
        }
        let mut rows = std::mem::take(&mut self.jobs);
        for id in &after {
            if let Some(i) = rows.iter().position(|j| j.id == *id) {
                self.jobs.push(rows.swap_remove(i));
            }
        }
        Some(after)
    }
}

/// A waiting job whose spec this version can read.
fn is_startable(job: &JobRecord) -> bool {
    job.state == JobState::Waiting && matches!(job.spec, SpecSlot::Decoded(_))
}

/// The order after moving `ids` (in their current relative order) to `to`. Unknown ids are ignored.
pub fn moved(order: &[JobId], ids: &[JobId], to: MoveTarget) -> Vec<JobId> {
    let selected = |id: &JobId| ids.contains(id);
    let chosen: Vec<JobId> = order.iter().copied().filter(selected).collect();
    let rest: Vec<JobId> = order.iter().copied().filter(|id| !selected(id)).collect();
    match to {
        MoveTarget::Top => chosen.into_iter().chain(rest).collect(),
        MoveTarget::Bottom => rest.into_iter().chain(chosen).collect(),
        MoveTarget::Before(anchor) => match rest.iter().position(|id| *id == anchor) {
            Some(at) => {
                let (head, tail) = rest.split_at(at);
                head.iter().chain(&chosen).chain(tail).copied().collect()
            }
            None => rest.into_iter().chain(chosen).collect(),
        },
        MoveTarget::Up => step(order, &selected),
        MoveTarget::Down => {
            let reversed: Vec<JobId> = order.iter().rev().copied().collect();
            let mut out = step(&reversed, &selected);
            out.reverse();
            out
        }
    }
}

/// Every selected job moves one place towards the front, unless a selected job (or the front) is in its way: a block
/// of selected jobs moves as one.
fn step(order: &[JobId], selected: &impl Fn(&JobId) -> bool) -> Vec<JobId> {
    let mut out = order.to_vec();
    for i in 1..out.len() {
        let (Some(prev), Some(cur)) = (out.get(i - 1), out.get(i)) else {
            continue;
        };
        if selected(cur) && !selected(prev) {
            out.swap(i - 1, i);
        }
    }
    out
}
