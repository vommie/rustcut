//! What the job card says (P12 block 10, Q-P12h), without Slint: the head (name, state, the job's picture), the rating
//! and tags, the render's progress or the failure, then the rows of place, source, cut, output, time and RustView, and a
//! quiet footer with the number, the creation time and the attempt. Empty facts fall away (no "Tags: -").
//! The card adapts to the state: a render shows its bar, speed and time left, a failure its reason, a finished job its
//! size and render time, a file that waits to move its new place.

use std::path::Path;

use rustcut_core::edit::FilterKind;
use rustcut_core::job::{JobSpec, JobState, JobStateKind};
use rustcut_core::queue::{JobProgress, JobSummary};
use rustcut_core::render::estimate_size;
use rustcut_core::rustview::SyncState;
use rustcut_core::time::Timestamp;

use super::super::text::{file_label, short_clock, size_text, state_text};
use crate::icons::Icon;

/// How a text stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Error,
    /// The primary color (a file that waits to move).
    Accent,
}

/// One row under the line: a symbol, a text (a path is cut in the middle) and a quieter text on the right.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FactRow {
    pub icon: Option<Icon>,
    pub text: String,
    pub right: String,
    pub tone: Tone,
    pub path: bool,
    /// The active filters' symbols follow the right text (the cut row).
    pub filters: bool,
}

/// The render's progress under the head.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// 0 to 1.
    pub fraction: f32,
    /// "Pass 1/2 · 48 fps · 1.9×".
    pub left: String,
    /// "~2:10 left".
    pub right: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub name: String,
    /// "Rendering 63 %", "Finished".
    pub state: String,
    pub kind: JobStateKind,
    pub rating: Option<u8>,
    pub tags: Vec<String>,
    /// "No rating, no tags" while the spec says so (empty while the spec is read).
    pub hint: String,
    pub progress: Option<Progress>,
    pub error: Option<String>,
    pub rows: Vec<FactRow>,
    pub filters: Vec<FilterKind>,
    /// "#12 · created 14:03 · attempt 2".
    pub footer: String,
    pub note: Option<String>,
}

/// What the card is made from.
pub struct Input<'a> {
    pub row: &'a JobSummary,
    pub progress: Option<&'a JobProgress>,
    /// The measured size of a finished output.
    pub size: Option<u64>,
    /// `None` while it is read, or when this version cannot read it.
    pub spec: Option<&'a JobSpec>,
    pub tag_name: &'a dyn Fn(u32) -> Option<String>,
    /// A waiting job's place among the waiting jobs (1 = next).
    pub in_line: Option<usize>,
    /// Unix milliseconds.
    pub now_ms: i64,
}

pub fn facts(input: &Input<'_>) -> Facts {
    let row = input.row;
    let kind = row.state.kind();
    let fraction = input.progress.map(JobProgress::fraction);
    let metadata = input.spec.map(|s| &s.metadata);
    let rating = metadata.and_then(|m| m.rating);
    let mut tags: Vec<String> = metadata
        .map(|m| m.tags.iter().filter_map(|id| (input.tag_name)(*id)).collect())
        .unwrap_or_default();
    tags.sort_by_key(|t| t.to_lowercase());
    // Without RustView's tag tree (off, not reached) no id has a name: the card counts them instead of "No tags".
    let unnamed = metadata.map_or(0, |m| m.tags.len()).saturating_sub(tags.len());
    let count = match unnamed {
        1 => "1 tag".to_owned(),
        n => format!("{n} tags"),
    };
    let hint = match (input.spec, rating.is_some(), tags.is_empty(), unnamed > 0) {
        (None, ..) | (Some(_), true, false, _) => String::new(),
        (Some(_), false, true, true) => format!("No rating · {count}"),
        (Some(_), true, true, true) => count,
        (Some(_), false, true, false) => "No rating, no tags".to_owned(),
        (Some(_), false, false, _) => "No rating".to_owned(),
        (Some(_), true, true, false) => "No tags".to_owned(),
    };
    let progress = kind
        .is_active()
        .then(|| progress_of(&row.state, input.progress, fraction.unwrap_or(0.0)));
    let error = match &row.state {
        JobState::Failed { reason } => Some(reason.clone()),
        _ => None,
    };
    Facts {
        name: file_label(row),
        state: state_text(&row.state, fraction),
        kind,
        rating,
        tags,
        hint,
        progress,
        error,
        rows: rows(input),
        filters: input
            .spec
            .map(|s| s.filters.active().map(rustcut_core::edit::Filter::kind).collect())
            .unwrap_or_default(),
        footer: footer(row, input.now_ms),
        note: row.note.clone(),
    }
}

fn progress_of(state: &JobState, progress: Option<&JobProgress>, fraction: f64) -> Progress {
    let mut left = Vec::new();
    match state {
        JobState::Rendering { pass: 0, .. } => left.push("Starting".to_owned()),
        JobState::Rendering { pass, passes } | JobState::Paused { pass, passes } if *passes > 1 => {
            left.push(format!("Pass {pass}/{passes}"));
        }
        _ => {}
    }
    let mut right = Vec::new();
    if let Some(p) = progress {
        if p.fps >= 1.0 {
            left.push(format!("{:.0} fps", p.fps));
        } else if p.fps > 0.0 {
            left.push(format!("{:.1} fps", p.fps));
        }
        if p.speed > 0.0 {
            left.push(format!("{:.1}×", p.speed));
        }
        // The size estimate stands in the output row.
        if let Some(eta) = p.eta_secs {
            right.push(format!("~{} left", short_clock(Timestamp::from_secs(i64::from(eta)))));
        }
    }
    Progress {
        fraction: az::Az::az::<f32>(fraction.clamp(0.0, 1.0)),
        left: left.join(" · "),
        right: right.join(" · "),
    }
}

fn rows(input: &Input<'_>) -> Vec<FactRow> {
    let row = input.row;
    let kind = row.state.kind();
    let mut rows = vec![FactRow {
        icon: Some(Icon::TargetFolder),
        text: path_text(&row.target),
        path: true,
        ..FactRow::default()
    }];
    if let Some(to) = &row.relocation {
        rows.push(FactRow {
            icon: Some(Icon::JobMove),
            text: path_text(to),
            right: if kind.is_active() { "after the render" } else { "" }.to_owned(),
            tone: Tone::Accent,
            path: true,
            ..FactRow::default()
        });
    }
    rows.push(FactRow {
        icon: Some(Icon::CardSource),
        text: path_text(&row.source),
        path: true,
        ..FactRow::default()
    });
    let sections = match row.sections {
        1 => "1 section".to_owned(),
        n => format!("{n} sections"),
    };
    rows.push(FactRow {
        icon: Some(Icon::CardCut),
        text: format!("{} · {sections}", short_clock(row.duration)),
        right: input.spec.and_then(output_size).unwrap_or_default(),
        filters: true,
        ..FactRow::default()
    });
    let bytes = match (kind, input.progress.and_then(estimate_size), input.size) {
        (JobStateKind::Finished, _, Some(bytes)) => size_text(bytes),
        (JobStateKind::Rendering | JobStateKind::Paused, Some(bytes), _) => format!("~{}", size_text(bytes)),
        _ => String::new(),
    };
    if !row.profile.is_empty() || !bytes.is_empty() {
        rows.push(FactRow {
            icon: Some(Icon::CardOutput),
            text: row.profile.clone(),
            right: bytes,
            ..FactRow::default()
        });
    }
    if let Some(time) = time_row(input) {
        rows.push(time);
    }
    rows.push(rustview(row, input.spec));
    rows
}

/// The size the render writes ("1920×800"), when the spec knows the source's.
fn output_size(spec: &JobSpec) -> Option<String> {
    let source = spec.source_info?.size;
    let size = spec.filters.final_size(source, spec.final_multiple).ok()?;
    Some(format!("{}×{}", size.width, size.height))
}

/// When it renders, or how long it took (nothing while it renders).
fn time_row(input: &Input<'_>) -> Option<FactRow> {
    let row = input.row;
    let took = |end: i64| {
        row.started_at
            .filter(|start| end >= *start)
            .map(|start| short_clock(Timestamp::from_millis(end - start)))
    };
    let ended = |at: i64| format!("ended {}", local_time(at, input.now_ms));
    let (text, right) = match row.state.kind() {
        JobStateKind::Waiting => (
            input
                .in_line
                .map_or_else(|| "Waiting".to_owned(), |n| format!("Waiting · #{n} in line")),
            String::new(),
        ),
        JobStateKind::Held => ("Held until released".to_owned(), String::new()),
        // The bar's line has the time left; `started_at` is the first start, also of a render that was interrupted.
        JobStateKind::Rendering | JobStateKind::Paused => return None,
        kind @ (JobStateKind::Finished | JobStateKind::Failed | JobStateKind::Canceled) => {
            let end = row.finished_at?;
            let verb = match kind {
                JobStateKind::Finished => "Rendered in",
                JobStateKind::Failed => "Failed after",
                _ => "Canceled after",
            };
            match took(end) {
                Some(t) => (format!("{verb} {t}"), ended(end)),
                None => (ended(end), String::new()),
            }
        }
    };
    Some(FactRow {
        icon: Some(Icon::CardTime),
        text,
        right,
        ..FactRow::default()
    })
}

fn footer(row: &JobSummary, now_ms: i64) -> String {
    let mut parts = vec![
        row.id.to_string(),
        format!("created {}", local_time(row.created_at, now_ms)),
    ];
    if row.attempt > 1 {
        parts.push(format!("attempt {}", row.attempt));
    }
    parts.join(" · ")
}

fn rustview(row: &JobSummary, spec: Option<&JobSpec>) -> FactRow {
    let (text, icon, tone) = if spec.is_some_and(|s| s.metadata.without_rustview) {
        ("Saved without RustView".to_owned(), Icon::CardRustviewOff, Tone::Muted)
    } else {
        match &row.sync {
            SyncState::NotSynced => ("Not in RustView".to_owned(), Icon::CardRustviewOff, Tone::Muted),
            SyncState::Pending => ("Waits for RustView".to_owned(), Icon::CardRustviewWaiting, Tone::Muted),
            SyncState::Synced => ("In RustView".to_owned(), Icon::CardRustviewWritten, Tone::Normal),
            SyncState::Failed { error } => (
                format!("RustView failed: {error}"),
                Icon::CardRustviewFailed,
                Tone::Error,
            ),
            SyncState::CleanedUp => (
                "Removed from RustView again".to_owned(),
                Icon::CardRustviewRemoved,
                Tone::Muted,
            ),
        }
    };
    FactRow {
        icon: Some(icon),
        text,
        tone,
        ..FactRow::default()
    }
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

/// "14:03" today, else "2026-10-30 14:03" (local time).
pub fn local_time(ms: i64, now_ms: i64) -> String {
    let local = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).map(|t| t.with_timezone(&chrono::Local));
    match (local(ms), local(now_ms)) {
        (Some(t), Some(now)) if t.date_naive() == now.date_naive() => t.format("%H:%M").to_string(),
        (Some(t), _) => t.format("%Y-%m-%d %H:%M").to_string(),
        (None, _) => String::new(),
    }
}

/// A waiting job's place among the waiting jobs, in queue order (1 = next).
pub fn in_line(rows: &[JobSummary], id: rustcut_core::job::JobId) -> Option<usize> {
    rows.iter()
        .filter(|r| r.state.kind() == JobStateKind::Waiting)
        .position(|r| r.id == id)
        .map(|i| i + 1)
}

#[cfg(test)]
#[path = "facts_tests.rs"]
mod tests;
