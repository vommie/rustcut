//! The queue's overall progress and total ETA. The progress covers the current run: the jobs that waited or rendered
//! since the queue was last idle (finished, failed and canceled ones count as done, held ones are left out), weighted
//! by their output length. The ETA adds the running renders' own estimates to the waiting jobs' length divided by the
//! speed RustCut measured on the renders of this session (output seconds per second, passes included).

use std::collections::{HashMap, HashSet};

use az::CheckedAs as _;

use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::queue::{JobProgress, JobSummary};

#[derive(Debug, Clone, Default)]
pub struct Totals {
    /// The jobs of the current run.
    batch: HashSet<JobId>,
    /// Nothing waits or renders: the run is over (its result stays shown until the next job starts or is added).
    closed: bool,
    /// Output seconds rendered per second of work, measured on the renders of this session.
    rate: Option<f64>,
}

/// What the panel's header shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overall {
    /// 0 to 1 over the current run.
    pub fraction: f64,
    pub done: usize,
    pub total: usize,
    /// Seconds until the last waiting job is done; `None` while nothing is known.
    pub eta_secs: Option<u64>,
    /// Waiting jobs could not be estimated yet (no render measured): the ETA covers only the running ones.
    pub partial: bool,
}

const fn is_open(kind: JobStateKind) -> bool {
    matches!(
        kind,
        JobStateKind::Waiting | JobStateKind::Rendering | JobStateKind::Paused
    )
}

impl Totals {
    /// Follows the rows after a change; `now_ms` is the wall clock in Unix milliseconds (for the speed measurement).
    pub fn update(&mut self, rows: &[JobSummary], progress: &HashMap<JobId, JobProgress>, now_ms: i64) {
        let open: Vec<JobId> = rows.iter().filter(|r| is_open(r.state.kind())).map(|r| r.id).collect();
        self.batch
            .retain(|id| rows.iter().any(|r| r.id == *id && r.state.kind() != JobStateKind::Held));
        if open.is_empty() {
            self.closed = true;
        } else {
            if self.closed {
                self.batch.clear();
                self.closed = false;
            }
            self.batch.extend(open);
        }
        let rates: Vec<f64> = rows
            .iter()
            .filter_map(|r| {
                let p = progress.get(&r.id)?;
                let eta = f64::from(p.eta_secs?);
                let elapsed = (now_ms - r.started_at?).max(0) as f64 / 1000.0;
                let length = r.duration.as_micros().max(0) as f64 / 1e6;
                // Too early to tell: the first seconds hold the probe and the encoder's start.
                (elapsed >= 3.0 && length > 0.0).then(|| length / (elapsed + eta))
            })
            .collect();
        if !rates.is_empty() {
            self.rate = Some(rates.iter().sum::<f64>() / rates.len() as f64);
        }
    }

    /// The overall progress and ETA of the current run with `workers` renders at a time; `None` without a run.
    pub fn overall(&self, rows: &[JobSummary], progress: &HashMap<JobId, JobProgress>, workers: u8) -> Option<Overall> {
        let jobs: Vec<&JobSummary> = rows.iter().filter(|r| self.batch.contains(&r.id)).collect();
        if jobs.is_empty() {
            return None;
        }
        let length = |r: &JobSummary| (r.duration.as_micros().max(0) as f64 / 1e6).max(0.001);
        let (mut done_secs, mut all_secs, mut done, mut running_eta, mut waiting_secs) = (0.0, 0.0, 0, 0.0, 0.0);
        let mut unknown_running = false;
        for r in &jobs {
            let secs = length(r);
            all_secs += secs;
            let kind = r.state.kind();
            match kind {
                JobStateKind::Rendering | JobStateKind::Paused => {
                    let p = progress.get(&r.id);
                    done_secs += secs * p.map_or(0.0, JobProgress::fraction);
                    match p.and_then(|p| p.eta_secs) {
                        Some(eta) => running_eta += f64::from(eta),
                        None => unknown_running = true,
                    }
                }
                JobStateKind::Waiting => waiting_secs += secs,
                _ if kind.is_done() => {
                    done_secs += secs;
                    done += 1;
                }
                _ => {}
            }
        }
        let workers = f64::from(workers.max(1));
        let waiting_eta = (waiting_secs > 0.0).then(|| self.rate.map(|rate| waiting_secs / rate.max(1e-3)));
        let partial = matches!(waiting_eta, Some(None)) || unknown_running;
        let eta = running_eta + waiting_eta.flatten().unwrap_or(0.0);
        let any_open = jobs.iter().any(|r| is_open(r.state.kind()));
        let eta_secs = (any_open && eta > 0.0)
            .then(|| (eta / workers).round().checked_as::<u64>())
            .flatten();
        Some(Overall {
            fraction: (done_secs / all_secs.max(0.001)).clamp(0.0, 1.0),
            done,
            total: jobs.len(),
            eta_secs,
            partial,
        })
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::job::JobState;
    use rustcut_core::time::Timestamp;

    use super::*;
    use crate::ui::queue::filter::tests::row;

    fn progress(id: i64, done: i64, eta: u32) -> JobProgress {
        JobProgress {
            id: JobId(id),
            pass: 1,
            passes: 1,
            done: Timestamp::from_secs(done),
            total: Timestamp::from_secs(10),
            fps: 0.0,
            speed: 0.0,
            size_bytes: 0,
            eta_secs: Some(eta),
        }
    }

    #[test]
    fn a_run_from_start_to_end() {
        let mut totals = Totals::default();
        let mut rows = vec![
            row(1, JobState::Rendering { pass: 1, passes: 1 }, "a"),
            row(2, JobState::Waiting, "b"),
            row(3, JobState::Held, "c"),
            row(4, JobState::Finished, "old"),
        ];
        rows[0].started_at = Some(0);
        let mut p = HashMap::new();
        p.insert(JobId(1), progress(1, 5, 10));
        totals.update(&rows, &p, 10_000);
        let o = totals.overall(&rows, &p, 1).unwrap();
        assert_eq!(
            (o.done, o.total),
            (0, 2),
            "held and earlier jobs are not part of the run"
        );
        assert!((o.fraction - 0.25).abs() < 1e-9);
        // 10 s of output in 10 s elapsed + 10 s to go: 0.5 output seconds per second; b needs 20 s.
        assert_eq!((o.eta_secs, o.partial), (Some(30), false));

        rows[0].state = JobState::Finished;
        rows[1].state = JobState::Rendering { pass: 1, passes: 1 };
        p.clear();
        totals.update(&rows, &p, 21_000);
        let o = totals.overall(&rows, &p, 1).unwrap();
        assert_eq!((o.done, o.total, o.eta_secs, o.partial), (1, 2, None, true));
        assert!((o.fraction - 0.5).abs() < 1e-9);

        rows[1].state = JobState::Failed { reason: "x".into() };
        totals.update(&rows, &p, 40_000);
        let o = totals.overall(&rows, &p, 1).unwrap();
        assert_eq!(
            (o.done, o.total, o.eta_secs),
            (2, 2, None),
            "the finished run stays shown"
        );
        assert!((o.fraction - 1.0).abs() < 1e-9);

        // A new job starts a new run.
        rows.push(row(5, JobState::Waiting, "d"));
        totals.update(&rows, &p, 50_000);
        let o = totals.overall(&rows, &p, 2).unwrap();
        assert_eq!(
            (o.done, o.total, o.eta_secs),
            (0, 1, Some(10)),
            "measured rate, two workers"
        );
    }

    #[test]
    fn no_run_no_totals_and_unknown_rates() {
        let mut totals = Totals::default();
        let rows = vec![row(1, JobState::Finished, "a")];
        totals.update(&rows, &HashMap::new(), 0);
        assert_eq!(totals.overall(&rows, &HashMap::new(), 1), None);
        let rows = vec![row(2, JobState::Waiting, "b")];
        totals.update(&rows, &HashMap::new(), 0);
        let o = totals.overall(&rows, &HashMap::new(), 1).unwrap();
        assert_eq!((o.eta_secs, o.partial), (None, true), "nothing measured yet");
    }
}
