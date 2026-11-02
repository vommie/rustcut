//! The queue as the panel knows it, without Slint: the rows the queue engine reported, the latest progress per running
//! job, the processing switches, the measured size of finished outputs and the overall progress of the current run.
//! The runtime task that follows the engine's events keeps it; the UI thread reads it when it refreshes.

use std::collections::HashMap;
use std::path::PathBuf;

use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::naming::QueuedTarget;
use rustcut_core::queue::{JobProgress, JobSummary, ProcessingState, QueueEvent, QueueSnapshot};

use super::totals::{Overall, Totals};

#[derive(Debug, Clone, Default)]
pub struct QueueView {
    pub rows: Vec<JobSummary>,
    pub progress: HashMap<JobId, JobProgress>,
    pub processing: Option<ProcessingState>,
    /// Sizes of finished outputs, measured on the runtime (with the path they were measured at; `None` while it is
    /// measured, or when the file is gone).
    pub sizes: HashMap<JobId, (PathBuf, Option<u64>)>,
    totals: Totals,
}

impl QueueView {
    pub fn replace(&mut self, snapshot: QueueSnapshot, now_ms: i64) {
        self.rows = snapshot.jobs;
        self.processing = Some(snapshot.processing);
        self.progress
            .retain(|id, _| self.rows.iter().any(|r| r.id == *id && r.state.kind().is_active()));
        self.sizes.retain(|id, _| self.rows.iter().any(|r| r.id == *id));
        self.totals.update(&self.rows, &self.progress, now_ms);
    }

    /// Applies an event; `true` when a job ended (its file may exist now).
    pub fn apply(&mut self, event: QueueEvent, now_ms: i64) -> bool {
        let ended = match event {
            QueueEvent::Added(row) => {
                if !self.rows.iter().any(|r| r.id == row.id) {
                    self.rows.push(*row);
                }
                false
            }
            QueueEvent::Updated(row) => {
                let ended = row.state.kind().is_done();
                if !row.state.kind().is_active() {
                    self.progress.remove(&row.id);
                }
                if row.state.kind() != JobStateKind::Finished {
                    self.sizes.remove(&row.id);
                }
                match self.rows.iter_mut().find(|r| r.id == row.id) {
                    Some(slot) => *slot = *row,
                    None => self.rows.push(*row),
                }
                ended
            }
            QueueEvent::Removed(ids) => {
                self.rows.retain(|r| !ids.contains(&r.id));
                self.progress.retain(|id, _| !ids.contains(id));
                self.sizes.retain(|id, _| !ids.contains(id));
                false
            }
            QueueEvent::Reordered(order) => {
                let mut rows = std::mem::take(&mut self.rows);
                for id in &order {
                    if let Some(i) = rows.iter().position(|r| r.id == *id) {
                        self.rows.push(rows.swap_remove(i));
                    }
                }
                self.rows.append(&mut rows);
                false
            }
            QueueEvent::Progress(p) => {
                if self.rows.iter().any(|r| r.id == p.id && r.state.kind().is_active()) {
                    self.progress.insert(p.id, p);
                }
                false
            }
            QueueEvent::Processing(p) => {
                self.processing = Some(p);
                false
            }
            // The status bar's move segment follows these itself (`ui::relocate`).
            QueueEvent::Done { .. } | QueueEvent::Relocation(_) | QueueEvent::Relocated(_) => false,
        };
        self.totals.update(&self.rows, &self.progress, now_ms);
        ended
    }

    pub fn paused(&self) -> bool {
        self.processing.as_ref().is_some_and(|p| p.paused)
    }

    pub fn pause_after_current(&self) -> bool {
        self.processing.as_ref().is_some_and(|p| p.pause_after_current)
    }

    pub fn row(&self, id: JobId) -> Option<&JobSummary> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn order(&self) -> Vec<JobId> {
        self.rows.iter().map(|r| r.id).collect()
    }

    /// The finished jobs whose output is not measured yet (or was measured at another path); they count as being
    /// measured from now on.
    pub fn take_unmeasured(&mut self) -> Vec<(JobId, PathBuf)> {
        let todo: Vec<(JobId, PathBuf)> = self
            .rows
            .iter()
            .filter(|r| r.state.kind() == JobStateKind::Finished)
            .filter(|r| self.sizes.get(&r.id).is_none_or(|(path, _)| *path != r.target))
            .map(|r| (r.id, r.target.clone()))
            .collect();
        for (id, path) in &todo {
            self.sizes.insert(*id, (path.clone(), None));
        }
        todo
    }

    pub fn size(&self, id: JobId) -> Option<u64> {
        self.sizes.get(&id).and_then(|(_, bytes)| *bytes)
    }

    /// The overall progress of the current run.
    pub fn overall(&self) -> Option<Overall> {
        let workers = self.processing.as_ref().map_or(1, |p| p.workers);
        self.totals.overall(&self.rows, &self.progress, workers)
    }

    /// Every job's target for the export panel's conflict check.
    pub fn targets(&self) -> Vec<QueuedTarget> {
        self.rows
            .iter()
            .map(|r| QueuedTarget {
                id: r.id,
                target: r.target.clone(),
                state: r.state.kind(),
            })
            .collect()
    }

    /// The totals line (review 2: a state symbol with a count each, only the states that occur): how many jobs wait,
    /// run (rendering or paused), are held, finished, failed and canceled, with the word of each.
    pub fn totals(&self) -> Vec<(JobStateKind, usize, &'static str)> {
        let mut counts = [0_usize; JobStateKind::ALL.len()];
        for row in &self.rows {
            if let Some(n) = JobStateKind::ALL
                .iter()
                .position(|k| *k == row.state.kind())
                .and_then(|i| counts.get_mut(i))
            {
                *n += 1;
            }
        }
        let count = |k: JobStateKind| {
            JobStateKind::ALL
                .iter()
                .position(|x| *x == k)
                .and_then(|i| counts.get(i))
                .copied()
                .unwrap_or_default()
        };
        [
            (JobStateKind::Waiting, count(JobStateKind::Waiting), "waiting"),
            (
                JobStateKind::Rendering,
                count(JobStateKind::Rendering) + count(JobStateKind::Paused),
                "running",
            ),
            (JobStateKind::Held, count(JobStateKind::Held), "held"),
            (JobStateKind::Finished, count(JobStateKind::Finished), "finished"),
            (JobStateKind::Failed, count(JobStateKind::Failed), "failed"),
            (JobStateKind::Canceled, count(JobStateKind::Canceled), "canceled"),
        ]
        .into_iter()
        .filter(|(_, n, _)| *n > 0)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::job::JobState;
    use rustcut_core::settings::WhenDone;
    use rustcut_core::time::Timestamp;

    use super::*;
    use crate::ui::queue::filter::tests::row;

    fn progress(id: i64) -> JobProgress {
        JobProgress {
            id: JobId(id),
            pass: 1,
            passes: 1,
            done: Timestamp::from_secs(5),
            total: Timestamp::from_secs(10),
            fps: 0.0,
            speed: 0.0,
            size_bytes: 0,
            eta_secs: Some(5),
        }
    }

    #[test]
    fn rows_follow_the_events() {
        let mut view = QueueView::default();
        view.replace(
            QueueSnapshot {
                jobs: vec![row(1, JobState::Waiting, "a"), row(2, JobState::Held, "b")],
                processing: ProcessingState {
                    paused: true,
                    pause_after_current: false,
                    workers: 1,
                    running: Vec::new(),
                    when_done: WhenDone::Nothing,
                },
            },
            0,
        );
        assert!(view.paused());
        view.apply(QueueEvent::Added(Box::new(row(3, JobState::Waiting, "c"))), 0);
        view.apply(QueueEvent::Reordered(vec![JobId(3), JobId(1), JobId(2)]), 0);
        assert_eq!(view.order(), vec![JobId(3), JobId(1), JobId(2)]);
        view.apply(
            QueueEvent::Updated(Box::new(row(3, JobState::Rendering { pass: 1, passes: 1 }, "c"))),
            0,
        );
        view.apply(QueueEvent::Progress(progress(3)), 0);
        view.apply(QueueEvent::Progress(progress(1)), 0);
        assert_eq!(
            view.progress.len(),
            1,
            "progress of a job that does not render is dropped"
        );
        let o = view.overall().unwrap();
        assert_eq!((o.done, o.total), (0, 2));
        assert!(
            view.apply(QueueEvent::Updated(Box::new(row(3, JobState::Finished, "c"))), 0),
            "an end is reported"
        );
        assert!(view.progress.is_empty());
        assert_eq!(
            view.take_unmeasured(),
            vec![(JobId(3), PathBuf::from("/out/clips/c.mkv"))]
        );
        assert!(view.take_unmeasured().is_empty(), "measured once");
        assert_eq!(view.size(JobId(3)), None);
        view.sizes
            .insert(JobId(3), (PathBuf::from("/out/clips/c.mkv"), Some(42)));
        assert_eq!(view.size(JobId(3)), Some(42));
        view.apply(QueueEvent::Removed(vec![JobId(2), JobId(3)]), 0);
        assert!(view.sizes.is_empty());
        assert_eq!(view.totals(), vec![(JobStateKind::Waiting, 1, "waiting")]);
        assert_eq!(view.targets().len(), 1);
    }
}
