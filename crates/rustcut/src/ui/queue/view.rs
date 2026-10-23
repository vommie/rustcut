//! The queue panel's state without Slint: the rows the queue engine reported, the latest progress per running job, the
//! selection, and the texts of a row (user decision Q-X2: a compact two-line list with controls; the full panel is M4.1).

use std::collections::HashMap;
use std::path::Path;

use humansize::{DECIMAL, format_size};
use rustcut_core::job::{JobId, JobState, JobStateKind};
use rustcut_core::naming::QueuedTarget;
use rustcut_core::queue::{JobProgress, JobSummary, ProcessingState, QueueEvent, QueueSnapshot};
use rustcut_core::render::estimate_size;
use rustcut_core::time::Timestamp;

/// What the panel knows.
#[derive(Debug, Clone, Default)]
pub struct QueueView {
    pub rows: Vec<JobSummary>,
    pub progress: HashMap<JobId, JobProgress>,
    pub processing: Option<ProcessingState>,
    pub selected: Option<JobId>,
}

/// One row as Slint shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct RowText {
    pub id: JobId,
    pub name: String,
    pub state: String,
    /// The state as a number for the color (the order of `JobStateKind::ALL`).
    pub kind: i32,
    /// 0 to 1 while rendering or when finished, negative otherwise.
    pub progress: f32,
    pub detail: String,
    pub tooltip: String,
}

impl QueueView {
    pub fn replace(&mut self, snapshot: QueueSnapshot) {
        self.rows = snapshot.jobs;
        self.processing = Some(snapshot.processing);
        self.progress
            .retain(|id, _| self.rows.iter().any(|r| r.id == *id && r.state.kind().is_active()));
        if self.selected.is_some_and(|id| !self.rows.iter().any(|r| r.id == id)) {
            self.selected = None;
        }
    }

    /// Applies an event; `true` when a job ended (its file may exist now).
    pub fn apply(&mut self, event: QueueEvent) -> bool {
        match event {
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
                match self.rows.iter_mut().find(|r| r.id == row.id) {
                    Some(slot) => *slot = *row,
                    None => self.rows.push(*row),
                }
                ended
            }
            QueueEvent::Removed(ids) => {
                self.rows.retain(|r| !ids.contains(&r.id));
                self.progress.retain(|id, _| !ids.contains(id));
                if self.selected.is_some_and(|id| ids.contains(&id)) {
                    self.selected = None;
                }
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
            QueueEvent::Done { .. } => false,
        }
    }

    pub fn paused(&self) -> bool {
        self.processing.as_ref().is_some_and(|p| p.paused)
    }

    pub fn selected_row(&self) -> Option<&JobSummary> {
        let id = self.selected?;
        self.rows.iter().find(|r| r.id == id)
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

    pub fn row_texts(&self) -> Vec<RowText> {
        self.rows.iter().map(|r| self.row_text(r)).collect()
    }

    fn row_text(&self, row: &JobSummary) -> RowText {
        let progress = self.progress.get(&row.id);
        let fraction = progress.map(JobProgress::fraction);
        let percent = |f: f64| format!("{:.0} %", (f * 100.0).floor());
        let state = match (&row.state, fraction) {
            (JobState::Rendering { pass: 0, .. }, _) => "Starting".to_owned(),
            (JobState::Rendering { pass, passes }, f) if *passes > 1 => {
                format!("Pass {pass}/{passes} {}", percent(f.unwrap_or(0.0)))
            }
            (JobState::Rendering { .. }, f) => format!("Rendering {}", percent(f.unwrap_or(0.0))),
            (JobState::Paused { .. }, f) => format!("Paused {}", percent(f.unwrap_or(0.0))),
            (state, _) => title(state.kind()).to_owned(),
        };
        let progress_value = match row.state.kind() {
            JobStateKind::Rendering | JobStateKind::Paused => az::Az::az::<f32>(fraction.unwrap_or(0.0)),
            JobStateKind::Finished => 1.0,
            _ => -1.0,
        };
        let folder = row
            .target
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let detail = match (&row.state, progress) {
            (JobState::Failed { reason }, _) => reason.clone(),
            (state, Some(p)) if state.kind().is_active() => running_detail(p),
            _ => {
                let mut parts = vec![folder, row.profile.clone(), clock(row.duration)];
                parts.retain(|p| !p.is_empty());
                parts.join(" · ")
            }
        };
        let mut tooltip = format!(
            "Job {} (attempt {})\n{}\nfrom {}",
            row.id,
            row.attempt,
            row.target.display(),
            row.source.display()
        );
        if let Some(note) = &row.note {
            tooltip.push('\n');
            tooltip.push_str(note);
        }
        RowText {
            id: row.id,
            name: file_label(row),
            state,
            kind: kind_number(row.state.kind()),
            progress: progress_value,
            detail,
            tooltip,
        }
    }

    /// One line for the panel's header: how many jobs wait, run and are done.
    pub fn summary(&self) -> String {
        let count = |k: JobStateKind| self.rows.iter().filter(|r| r.state.kind() == k).count();
        let running = count(JobStateKind::Rendering) + count(JobStateKind::Paused);
        let mut parts = Vec::new();
        for (n, word) in [
            (count(JobStateKind::Waiting), "waiting"),
            (running, "running"),
            (count(JobStateKind::Held), "held"),
            (count(JobStateKind::Finished), "finished"),
            (count(JobStateKind::Failed), "failed"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        if parts.is_empty() {
            "No jobs".to_owned()
        } else {
            parts.join(" · ")
        }
    }
}

/// "ETA 1:23 · ~245 MB · 2.1x" for a running job.
fn running_detail(p: &JobProgress) -> String {
    let mut parts = Vec::new();
    if let Some(eta) = p.eta_secs {
        parts.push(format!("ETA {}", clock(Timestamp::from_secs(i64::from(eta)))));
    }
    // The bytes written so far say little before the estimate (header, lookahead): only the estimate is shown.
    if let Some(bytes) = estimate_size(p) {
        parts.push(format!("~{}", format_size(bytes, DECIMAL)));
    }
    if p.speed > 0.0 {
        parts.push(format!("{:.1}x", p.speed));
    }
    if p.fps > 0.0 {
        parts.push(format!("{:.0} fps", p.fps));
    }
    parts.join(" · ")
}

/// `H:MM:SS`, whole seconds.
pub fn clock(t: Timestamp) -> String {
    let secs = t.as_micros().max(0) / 1_000_000;
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn file_label(row: &JobSummary) -> String {
    row.target
        .file_name()
        .map_or_else(|| row.name.clone(), |n| n.to_string_lossy().into_owned())
}

pub const fn title(kind: JobStateKind) -> &'static str {
    match kind {
        JobStateKind::Waiting => "Waiting",
        JobStateKind::Held => "Held",
        JobStateKind::Rendering => "Rendering",
        JobStateKind::Paused => "Paused",
        JobStateKind::Finished => "Finished",
        JobStateKind::Failed => "Failed",
        JobStateKind::Canceled => "Canceled",
    }
}

fn kind_number(kind: JobStateKind) -> i32 {
    JobStateKind::ALL
        .iter()
        .position(|k| *k == kind)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::rustview::SyncState;
    use rustcut_core::settings::WhenDone;

    use super::*;

    fn row(id: i64, state: JobState) -> JobSummary {
        JobSummary {
            id: JobId(id),
            state,
            attempt: 1,
            name: format!("Clip {id}"),
            target: PathBuf::from(format!("/out/clips/Clip {id}.mkv")),
            source: PathBuf::from("/in/a.mp4"),
            profile: "AV1 balanced".into(),
            sections: 1,
            duration: Timestamp::from_secs(75),
            note: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            sync: SyncState::NotSynced,
        }
    }

    fn progress(id: i64) -> JobProgress {
        JobProgress {
            id: JobId(id),
            pass: 2,
            passes: 2,
            done: Timestamp::from_secs(30),
            total: Timestamp::from_secs(60),
            fps: 48.0,
            speed: 2.0,
            size_bytes: 5_000_000,
            eta_secs: Some(15),
        }
    }

    #[test]
    fn rows_follow_the_events() {
        let mut view = QueueView::default();
        view.replace(QueueSnapshot {
            jobs: vec![row(1, JobState::Waiting), row(2, JobState::Held)],
            processing: ProcessingState {
                paused: true,
                pause_after_current: false,
                workers: 1,
                running: Vec::new(),
                when_done: WhenDone::Nothing,
            },
        });
        assert!(view.paused());
        view.selected = Some(JobId(2));
        view.apply(QueueEvent::Added(Box::new(row(3, JobState::Waiting))));
        view.apply(QueueEvent::Reordered(vec![JobId(3), JobId(1), JobId(2)]));
        assert_eq!(view.rows.iter().map(|r| r.id.0).collect::<Vec<_>>(), vec![3, 1, 2]);
        view.apply(QueueEvent::Updated(Box::new(row(
            3,
            JobState::Rendering { pass: 2, passes: 2 },
        ))));
        view.apply(QueueEvent::Progress(progress(3)));
        view.apply(QueueEvent::Progress(progress(1)));
        assert_eq!(
            view.progress.len(),
            1,
            "progress of a job that does not render is dropped"
        );
        let text = &view.row_texts()[0];
        assert_eq!(text.state, "Pass 2/2 75 %");
        assert!((text.progress - 0.75).abs() < 1e-6);
        assert_eq!(text.detail, "ETA 0:00:15 · ~10 MB · 2.0x · 48 fps");
        assert_eq!(text.name, "Clip 3.mkv");
        let waiting = &view.row_texts()[1];
        assert_eq!(
            (waiting.state.as_str(), waiting.detail.as_str()),
            ("Waiting", "clips · AV1 balanced · 0:01:15")
        );
        assert!(
            view.apply(QueueEvent::Updated(Box::new(row(3, JobState::Finished)))),
            "an end is reported"
        );
        assert!(view.progress.is_empty());
        view.apply(QueueEvent::Removed(vec![JobId(2)]));
        assert_eq!(view.selected, None);
        assert_eq!(view.summary(), "1 waiting · 1 finished");
        let failed = row(
            4,
            JobState::Failed {
                reason: "ffmpeg failed".into(),
            },
        );
        view.apply(QueueEvent::Added(Box::new(failed)));
        assert_eq!(view.row_texts()[2].detail, "ffmpeg failed");
        assert_eq!(view.targets().len(), 3);
    }
}
