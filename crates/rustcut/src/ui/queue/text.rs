//! The texts of a queue row: the compact two-line form for a narrow sidebar (name and state; folder, profile and length,
//! or ETA, size estimate and speed while rendering with the percentage on the right, or the failure) and the cells of
//! the table form (`columns.rs`).

use std::path::Path;

use humansize::{BINARY, format_size};
use rustcut_core::job::{JobId, JobState, JobStateKind};
use rustcut_core::queue::{JobProgress, JobSummary};
use rustcut_core::render::estimate_size;
use rustcut_core::time::Timestamp;

use super::columns::Column;

/// One row as Slint shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct RowText {
    pub id: JobId,
    pub name: String,
    /// The state without the percentage (the compact form shows it on its second row, P9.6).
    pub state: String,
    /// "45 %" while rendering, else empty.
    pub percent: String,
    /// The state as a number for the color (the order of `JobStateKind::ALL`).
    pub kind: i32,
    /// 0 to 1 while rendering or when finished, negative otherwise.
    pub progress: f32,
    pub detail: String,
    pub tooltip: String,
    /// The table's cells, one per shown column.
    pub cells: Vec<String>,
}

/// The texts of `row`; `size` is the measured size of a finished output.
pub fn row_text(row: &JobSummary, progress: Option<&JobProgress>, size: Option<u64>, columns: &[Column]) -> RowText {
    let fraction = progress.map(JobProgress::fraction);
    let progress_value = match row.state.kind() {
        JobStateKind::Rendering | JobStateKind::Paused => az::Az::az::<f32>(fraction.unwrap_or(0.0)),
        JobStateKind::Finished => 1.0,
        _ => -1.0,
    };
    let detail = match (&row.state, progress) {
        (JobState::Failed { reason }, _) => reason.clone(),
        (state, Some(p)) if state.kind().is_active() => running_detail(p),
        _ => {
            let mut parts = vec![folder(row), row.profile.clone(), clock(row.duration)];
            // A finished job shows its output's size (review 2).
            if row.state.kind() == JobStateKind::Finished
                && let Some(bytes) = size
            {
                parts.push(size_text(bytes));
            }
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
    let state = state_text(&row.state, fraction);
    let percent = match row.state.kind() {
        JobStateKind::Rendering | JobStateKind::Paused if !matches!(row.state, JobState::Rendering { pass: 0, .. }) => {
            percent_text(fraction.unwrap_or(0.0))
        }
        _ => String::new(),
    };
    let cells = columns
        .iter()
        .map(|c| match c {
            Column::Id => row.id.0.to_string(),
            Column::Name => file_label(row),
            Column::State => state.clone(),
            Column::Eta => time_cell(row, progress),
            Column::Length => match row.sections {
                0 | 1 => clock(row.duration),
                n => format!("{} ({n})", clock(row.duration)),
            },
            Column::Size => match (row.state.kind(), progress.and_then(estimate_size), size) {
                (JobStateKind::Rendering | JobStateKind::Paused, Some(bytes), _) => {
                    format!("~{}", size_text(bytes))
                }
                (JobStateKind::Finished, _, Some(bytes)) => size_text(bytes),
                _ => String::new(),
            },
            Column::Profile => row.profile.clone(),
            Column::Folder => folder(row),
            Column::Remove => String::new(),
        })
        .collect();
    RowText {
        id: row.id,
        name: file_label(row),
        state: state_title(&row.state),
        percent,
        kind: kind_number(row.state.kind()),
        progress: progress_value,
        detail,
        tooltip,
        cells,
    }
}

/// The table's state cell: a running render ends with its percentage.
fn state_text(state: &JobState, fraction: Option<f64>) -> String {
    match state {
        JobState::Rendering { pass: 0, .. } => state_title(state),
        JobState::Rendering { .. } | JobState::Paused { .. } => {
            format!("{} {}", state_title(state), percent_text(fraction.unwrap_or(0.0)))
        }
        _ => state_title(state),
    }
}

/// The state without the percentage (the compact form shows it on its second row).
fn state_title(state: &JobState) -> String {
    match state {
        JobState::Rendering { pass: 0, .. } => "Starting".to_owned(),
        JobState::Rendering { pass, passes } if *passes > 1 => format!("Pass {pass}/{passes}"),
        JobState::Rendering { .. } => "Rendering".to_owned(),
        JobState::Paused { .. } => "Paused".to_owned(),
        state => title(state.kind()).to_owned(),
    }
}

fn percent_text(fraction: f64) -> String {
    format!("{:.0} %", (fraction * 100.0).floor())
}

/// The "Time" column (P9.6, the UI review's note 18#2): the time left while the job renders, afterwards how long its
/// render took (until it finished, failed or was canceled).
fn time_cell(row: &JobSummary, progress: Option<&JobProgress>) -> String {
    let kind = row.state.kind();
    if kind.is_active() {
        return progress
            .and_then(|p| p.eta_secs)
            .map(|eta| short_clock(Timestamp::from_secs(i64::from(eta))))
            .unwrap_or_default();
    }
    match (kind.is_done(), row.started_at, row.finished_at) {
        (true, Some(start), Some(end)) if end >= start => short_clock(Timestamp::from_millis(end - start)),
        _ => String::new(),
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
        parts.push(format!("~{}", size_text(bytes)));
    }
    if p.speed > 0.0 {
        parts.push(format!("{:.1}x", p.speed));
    }
    if p.fps > 0.0 {
        parts.push(format!("{:.0} fps", p.fps));
    }
    parts.join(" · ")
}

/// A file's size in binary units with at most one decimal ("324 KiB", "2.1 MiB", "2.3 GiB"; review 2).
pub fn size_text(bytes: u64) -> String {
    format_size(bytes, BINARY.decimal_places(1).decimal_zeroes(0))
}

/// `H:MM:SS`, whole seconds.
pub fn clock(t: Timestamp) -> String {
    let secs = t.as_micros().max(0) / 1_000_000;
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// `M:SS` below an hour, else `H:MM:SS`.
pub fn short_clock(t: Timestamp) -> String {
    let secs = t.as_micros().max(0) / 1_000_000;
    if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        clock(t)
    }
}

/// The target's file name.
pub fn file_label(row: &JobSummary) -> String {
    row.target
        .file_name()
        .map_or_else(|| row.name.clone(), |n| n.to_string_lossy().into_owned())
}

/// The name of the target folder.
fn folder(row: &JobSummary) -> String {
    row.target
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
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

pub fn kind_number(kind: JobStateKind) -> i32 {
    JobStateKind::ALL
        .iter()
        .position(|k| *k == kind)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::queue::columns::ORDER;
    use crate::ui::queue::filter::tests::row;

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
            eta_secs: Some(75),
        }
    }

    #[test]
    fn compact_and_table_texts() {
        let mut running = row(3, JobState::Rendering { pass: 2, passes: 2 }, "Clip 3");
        running.sections = 3;
        let p = progress(3);
        let t = row_text(&running, Some(&p), None, &ORDER);
        assert_eq!((t.state.as_str(), t.percent.as_str()), ("Pass 2/2", "75 %"));
        assert!((t.progress - 0.75).abs() < 1e-6);
        assert_eq!(t.detail, "ETA 0:01:15 · ~9.5 MiB · 2.0x · 48 fps");
        assert_eq!(
            t.cells,
            [
                "3",
                "Clip 3.mkv",
                "Pass 2/2 75 %",
                "1:15",
                "0:00:10 (3)",
                "~9.5 MiB",
                "AV1 balanced",
                "clips",
                ""
            ]
        );
        let waiting = row_text(
            &row(1, JobState::Waiting, "a"),
            None,
            None,
            &[Column::Eta, Column::Size],
        );
        assert_eq!(waiting.detail, "clips · AV1 balanced · 0:00:10");
        assert_eq!(waiting.cells, ["", ""]);
        let finished = row_text(&row(2, JobState::Finished, "b"), None, Some(1_234_000), &[Column::Size]);
        assert_eq!(finished.cells, ["1.2 MiB"]);
        assert_eq!(
            finished.detail, "clips · AV1 balanced · 0:00:10 · 1.2 MiB",
            "the output's size"
        );
        assert_eq!(size_text(331_776), "324 KiB");
        assert_eq!(size_text(2_469_606_195), "2.3 GiB");
        assert!((finished.progress - 1.0).abs() < 1e-6);
        let failed = row(
            4,
            JobState::Failed {
                reason: "ffmpeg failed".into(),
            },
            "c",
        );
        assert_eq!(row_text(&failed, None, None, &[]).detail, "ffmpeg failed");
        assert_eq!(short_clock(Timestamp::from_secs(3_725)), "1:02:05");
        // The time column of a done job: how long its render took.
        let mut done = row(5, JobState::Finished, "d");
        (done.started_at, done.finished_at) = (Some(1_000), Some(96_000));
        let t = row_text(&done, None, None, &[Column::Eta]);
        assert_eq!((t.cells, t.percent), (vec!["1:35".to_owned()], String::new()));
    }
}
