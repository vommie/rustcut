//! The queue panel's refresh (UI thread): the shown rows (the filter's result), the table's columns, the filter menu's
//! choices and badge, the totals line, the toolbar, and the hooks of the other panels that follow the queue.

use std::fmt::Write as _;

use az::CheckedAs as _;
use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::queue::JobSummary;
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::{
    COMPACT_ROW, Column, Filter, FilterChoice, FilterKind, Overall, QueueAction, QueueBridge, ROW_PADDING, RowText,
    TABLE_ROW, actions, kind_number, layout, pause, row_text, short_clock, status_of,
};
use crate::util::model::{sync, sync_with};
use crate::{MultiChoiceItem, Queue, QueueColumnView, QueueRowView, QueueTotalView};

impl QueueBridge {
    /// The filter menu: its three choices with their texts, the rule and the badge's number.
    fn publish_filter(&self, q: &Queue<'_>, filter: &Filter, choices: &[(FilterKind, Vec<FilterChoice>); 3]) {
        *self.filter_values.borrow_mut() = choices
            .each_ref()
            .map(|(_, entries)| entries.iter().map(|c| c.value.clone()).collect());
        for ((kind, entries), model) in choices.iter().zip(&self.filter_models) {
            let text = Filter::summary(entries, *kind);
            sync(
                model,
                entries
                    .iter()
                    .map(|c| MultiChoiceItem {
                        label: c.label.as_str().into(),
                        detail: c.count.to_string().into(),
                        on: c.on,
                        tone: c.tone.map_or(-1, kind_number),
                    })
                    .collect(),
            );
            match kind {
                FilterKind::State => q.set_filter_states_text(text.into()),
                FilterKind::Folder => q.set_filter_folders_text(text.into()),
                FilterKind::Profile => q.set_filter_profiles_text(text.into()),
            }
        }
        q.set_filter_any(filter.any);
        q.set_filter_count(i32::try_from(filter.active_count()).unwrap_or(i32::MAX));
    }

    /// Shows the current view (UI thread).
    pub(super) fn refresh(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let mut ui = self.ui.borrow_mut();
        let layout = layout(
            (ui.width - ROW_PADDING)
                .max(0.0)
                .round()
                .checked_as::<u32>()
                .unwrap_or_default(),
            &ui.widths,
        );
        let shown: Vec<Column> = layout.slots.iter().map(|s| s.column).collect();
        let (texts, visible, total, choices, summary, overall, paused, pause_after, finished, selected, ended, start) = {
            let mut shared = self.shared.lock();
            let ended = std::mem::take(&mut shared.ended);
            let view = &shared.view;
            let rows: Vec<&JobSummary> = view.rows.iter().filter(|r| ui.filter.matches(r)).collect();
            let visible: Vec<JobId> = rows.iter().map(|r| r.id).collect();
            ui.selection
                .retain(|id| visible.contains(&id), |id| view.row(id).is_some());
            let texts: Vec<(RowText, bool)> = rows
                .iter()
                .map(|r| {
                    let text = row_text(r, view.progress.get(&r.id), view.size(r.id), &shown);
                    (text, QueueAction::Remove.applies_to(&r.state))
                })
                .collect();
            let selected: Vec<JobSummary> = ui
                .selection
                .in_order(&visible)
                .iter()
                .filter_map(|id| view.row(*id).cloned())
                .collect();
            (
                texts,
                visible,
                view.rows.len(),
                FilterKind::ALL.map(|kind| (kind, ui.filter.choices(kind, &view.rows))),
                view.totals(),
                view.overall(),
                view.paused(),
                view.pause_after_current(),
                view.rows
                    .iter()
                    .filter(|r| r.state.kind() == JobStateKind::Finished)
                    .count(),
                selected,
                ended,
                (view.processing.as_ref().map(|p| p.paused), pause::waiting(&view.rows)),
            )
        };
        ui.visible.clone_from(&visible);
        let current = ui.selection.current();
        let editing = self.editing.get();
        let rows: Vec<ShownRow> = texts
            .into_iter()
            .map(|(text, removable)| ShownRow {
                selected: ui.selection.contains(text.id),
                current: current == Some(text.id),
                editing: editing == Some(text.id),
                removable,
                text,
            })
            .collect();
        let row_count = rows.len();
        // Only the rows that changed are made again: a row's cells are a model, which Slint compares by pointer, so
        // comparing the Slint rows would rewrite every row on every refresh (10 Hz while rendering).
        sync_with(&self.rows, &mut self.shown_rows.borrow_mut(), &rows, ShownRow::view);
        self.drag_rows_changed(&visible);
        let column_views = layout
            .slots
            .iter()
            .map(|s| QueueColumnView {
                id: s.column.id().into(),
                title: s.column.title().into(),
                tooltip: s.column.tooltip().into(),
                x: az::Az::az::<f32>(s.x),
                width: az::Az::az::<f32>(s.width),
                right: s.column.right_aligned(),
                resizable: s.column.resizable(),
            })
            .collect();
        sync(&self.columns, column_views);
        let q = window.global::<Queue<'_>>();
        self.publish_filter(&q, &ui.filter, &choices);
        q.set_compact(layout.compact);
        q.set_row_height(if layout.compact { COMPACT_ROW } else { TABLE_ROW });
        q.set_filtered(ui.filter.is_active());
        q.set_filter_note(format!("{row_count} of {total} jobs shown").into());
        q.set_empty_text(
            if total > 0 && ui.filter.is_active() {
                "No job matches the filter."
            } else {
                self.empty_text.as_str()
            }
            .into(),
        );
        q.set_summary(if summary.is_empty() { "No jobs" } else { "" }.into());
        sync(
            &self.totals,
            summary
                .iter()
                .map(|(kind, count, word)| QueueTotalView {
                    kind: kind_number(*kind),
                    count: i32::try_from(*count).unwrap_or(i32::MAX),
                    // "3/4 finished": of every job in the queue (the user's wish, review 2).
                    tooltip: format!("{count}/{total} {word}").into(),
                })
                .collect(),
        );
        q.set_has_overall(overall.is_some());
        if let Some(o) = overall {
            q.set_overall(az::Az::az::<f32>(o.fraction));
            q.set_overall_text(overall_text(&o, paused).into());
        }
        q.set_paused(paused);
        q.set_pause_after(pause_after);
        q.set_can_pause_after(!paused && summary.iter().any(|(kind, _, _)| *kind == JobStateKind::Rendering));
        drop(ui);
        actions::publish_toolbar(&q, &selected, &visible, finished);
        self.pause_seen(start.0, start.1);
        if ended && let Some(hook) = self.hooks.borrow().job_ended.as_ref() {
            hook();
        }
        if let Some(hook) = self.hooks.borrow().refreshed.as_ref() {
            hook();
        }
        if let Some(hook) = self.hooks.borrow().jobs.as_ref() {
            let jobs = self
                .shared
                .lock()
                .view
                .rows
                .iter()
                .map(|row| (row.id.0, row.source.clone()))
                .collect();
            hook(jobs);
        }
        let (done, status, ended_jobs) = {
            let mut shared = self.shared.lock();
            (
                shared.done.take(),
                status_of(&shared.view),
                std::mem::take(&mut shared.ended_jobs),
            )
        };
        if let Some(hook) = self.hooks.borrow().job_done.as_ref() {
            for row in &ended_jobs {
                hook(row);
            }
        }
        for hook in &self.hooks.borrow().status {
            hook(status);
        }
        if let Some((finished, failed)) = done
            && let Some(hook) = self.hooks.borrow().done.as_ref()
        {
            hook(finished, failed);
        }
    }
}

/// "2 of 5 done · ~0:12:30 left" (the states' counts are symbols before it).
fn overall_text(o: &Overall, paused: bool) -> String {
    let mut text = format!("{} of {} done", o.done, o.total);
    match (o.eta_secs, paused) {
        (_, true) if o.done < o.total => text.push_str(" · paused"),
        (Some(eta), _) => {
            let eta = short_clock(rustcut_core::time::Timestamp::from_secs(
                i64::try_from(eta).unwrap_or(i64::MAX),
            ));
            let _ = if o.partial {
                write!(text, " · at least {eta} left")
            } else {
                write!(text, " · ~{eta} left")
            };
        }
        _ => {}
    }
    text
}

/// A row as last shown: its texts and the selection marks (compared to find the rows that changed).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ShownRow {
    text: RowText,
    removable: bool,
    selected: bool,
    current: bool,
    editing: bool,
}

impl ShownRow {
    fn view(&self) -> QueueRowView {
        let t = &self.text;
        QueueRowView {
            id: i32::try_from(t.id.0).unwrap_or(i32::MAX),
            removable: self.removable,
            percent: t.percent.as_str().into(),
            selected: self.selected,
            current: self.current,
            editing: self.editing,
            moving: t.moving,
            name: t.name.as_str().into(),
            state: t.state.as_str().into(),
            kind: t.kind,
            progress: t.progress,
            detail: t.detail.as_str().into(),
            tooltip: t.tooltip.as_str().into(),
            cells: ModelRc::new(VecModel::from(
                t.cells
                    .iter()
                    .map(|c| slint::SharedString::from(c.as_str()))
                    .collect::<Vec<_>>(),
            )),
        }
    }
}
