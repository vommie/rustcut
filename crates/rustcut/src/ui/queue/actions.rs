//! The queue panel's callbacks: selection, filter, columns, drag and drop, double click, the context menu, the toolbar
//! and the queue keys. Every action reaches the selected jobs it applies to (`menu.rs`) and goes to the engine without
//! waiting for it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use az::CheckedAs as _;
use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::queue::{JobSummary, MoveTarget, QueueCommand};
use slint::ComponentHandle as _;

use super::QueueBridge;
use super::columns::{self, Column};
use super::menu::{QueueAction, queue_menu, targets};
use crate::ui::dock::PanelId;
use crate::{MainWindow, Queue};

pub(super) fn connect(bridge: &Rc<QueueBridge>, window: &MainWindow) {
    let q = window.global::<Queue<'_>>();
    let weak = Rc::downgrade(bridge);
    q.on_width_changed(move |width| {
        if let Some(b) = weak.upgrade() {
            let changed = (b.ui.borrow().width - width).abs() > 0.5;
            if changed {
                b.ui.borrow_mut().width = width;
                b.refresh();
            }
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_select(move |id, ctrl, shift| {
        if let Some(b) = weak.upgrade() {
            b.select(JobId(i64::from(id)), ctrl, shift);
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_select_all(move || {
        if let Some(b) = weak.upgrade() {
            b.select_all();
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_double_clicked(move |id| {
        if let Some(b) = weak.upgrade() {
            b.double_clicked(JobId(i64::from(id)));
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_context_menu(move |id, x, y| {
        if let Some(b) = weak.upgrade() {
            if id < 0 {
                b.ui.borrow_mut().selection.clear();
                b.refresh();
            }
            b.open_menu(x, y);
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_drop_rows(move |index| {
        if let Some(b) = weak.upgrade() {
            b.drop_rows(usize::try_from(index).unwrap_or_default());
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_column_resize_started(move |id| {
        if let Some(b) = weak.upgrade()
            && let Some(column) = Column::from_id(&id)
        {
            let mut ui = b.ui.borrow_mut();
            let width = columns::width_of(column, &ui.widths);
            ui.resizing = Some((column, width));
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_column_resized(move |id, delta| {
        let Some(b) = weak.upgrade() else { return };
        {
            let mut ui = b.ui.borrow_mut();
            let Some((column, start)) = ui.resizing.filter(|(c, _)| c.id() == id.as_str()) else {
                return;
            };
            let width = columns::dragged(start, delta.round().checked_as::<i32>().unwrap_or_default());
            ui.widths.insert(column.id().to_owned(), width);
        }
        b.refresh();
    });
    let weak = Rc::downgrade(bridge);
    q.on_toggle_chip(move |id| {
        if let Some(b) = weak.upgrade() {
            b.ui.borrow_mut().filter.toggle(&id);
            b.refresh();
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_search_edited(move |text| {
        if let Some(b) = weak.upgrade() {
            b.ui.borrow_mut().filter.search = text.to_string();
            b.refresh();
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_clear_filter(move || {
        if let Some(b) = weak.upgrade() {
            b.ui.borrow_mut().filter.clear();
            if let Some(window) = b.window.upgrade() {
                window.global::<Queue<'_>>().set_search("".into());
            }
            b.refresh();
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_toggle_paused(move || {
        if let Some(b) = weak.upgrade() {
            let paused = b.paused();
            tracing::info!(paused = !paused, "[Queue] Processing switched in the panel");
            b.send(QueueCommand::SetPaused(!paused));
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_toggle_pause_after(move || {
        if let Some(b) = weak.upgrade() {
            let on = !b.shared.lock().view.pause_after_current();
            tracing::info!(on, "[Queue] Pause after the running jobs switched");
            b.send(QueueCommand::SetPauseAfterCurrent(on));
        }
    });
    q.on_cancel(bind(bridge, QueueAction::Cancel));
    q.on_retry(bind(bridge, QueueAction::Retry));
    q.on_remove(bind(bridge, QueueAction::Remove));
    q.on_remove_finished(bind(bridge, QueueAction::RemoveFinished));
    q.on_edit_tags(bind(bridge, QueueAction::EditTags));
    q.on_edit(bind(bridge, QueueAction::Edit));
    let weak = Rc::downgrade(bridge);
    q.on_toggle_hold(move || {
        if let Some(b) = weak.upgrade() {
            let selected = b.selected_rows();
            let rows: Vec<&JobSummary> = selected.iter().collect();
            // Held jobs are released; otherwise the waiting ones are held.
            if targets(&QueueAction::Release, &rows).len() == rows.len() && !rows.is_empty() {
                b.run(&QueueAction::Release);
            } else {
                b.run(&QueueAction::Hold);
            }
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_move_up(move || {
        if let Some(b) = weak.upgrade() {
            b.move_selected(MoveTarget::Up);
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_move_down(move || {
        if let Some(b) = weak.upgrade() {
            b.move_selected(MoveTarget::Down);
        }
    });
}

/// A button's handler: `action` on the selected jobs.
fn bind(bridge: &Rc<QueueBridge>, action: QueueAction) -> impl FnMut() + 'static {
    let weak = Rc::downgrade(bridge);
    move || {
        if let Some(b) = weak.upgrade() {
            b.run(&action);
        }
    }
}

/// The toolbar's state for the selected jobs (in queue order) out of the shown ones.
pub(super) fn publish_toolbar(q: &Queue<'_>, selected: &[JobSummary], visible: &[JobId], finished: usize) {
    let rows: Vec<&JobSummary> = selected.iter().collect();
    let n = rows.len();
    let count = |a: &QueueAction| targets(a, &rows).len();
    let jobs = |k: usize| {
        if k == 1 {
            "the job".to_owned()
        } else {
            format!("{k} jobs")
        }
    };
    q.set_selected_count(i32::try_from(n).unwrap_or(i32::MAX));
    q.set_can_edit(count(&QueueAction::Edit) == 1);
    q.set_can_edit_tags(n == 1);
    let cancel = count(&QueueAction::Cancel);
    q.set_can_cancel(cancel > 0);
    q.set_cancel_tooltip(
        if cancel > 0 {
            format!(
                "Cancel {} (a running render stops, an existing target stays)",
                jobs(cancel)
            )
        } else {
            "Cancel the selected jobs (none is waiting or rendering)".to_owned()
        }
        .into(),
    );
    let remove = count(&QueueAction::Remove);
    q.set_can_remove(remove > 0);
    q.set_remove_tooltip(
        if remove > 0 {
            format!("Remove {} from the queue (output and source files stay)", jobs(remove))
        } else {
            "Remove the selected jobs (not while they render)".to_owned()
        }
        .into(),
    );
    let retry = count(&QueueAction::Retry);
    q.set_can_retry(retry > 0);
    q.set_retry_tooltip(
        if retry > 0 {
            format!("Render {} again", jobs(retry))
        } else {
            "Render again (finished, failed or canceled jobs)".to_owned()
        }
        .into(),
    );
    let (hold, release) = (count(&QueueAction::Hold), count(&QueueAction::Release));
    let all_held = release == n && n > 0;
    q.set_can_hold(hold > 0 || release > 0);
    q.set_held(all_held);
    q.set_hold_tooltip(
        if all_held {
            format!("Held: release {}", jobs(release))
        } else if hold > 0 {
            format!("Hold {} (waits until released)", jobs(hold))
        } else {
            "Hold waiting jobs (they wait until released)".to_owned()
        }
        .into(),
    );
    let first = visible.iter().position(|id| rows.first().is_some_and(|r| r.id == *id));
    let last = visible.iter().position(|id| rows.last().is_some_and(|r| r.id == *id));
    q.set_can_move_up(first.is_some_and(|p| p > 0));
    q.set_can_move_down(last.is_some_and(|p| p + 1 < visible.len()));
    let moving = |dir: &str| {
        if n > 1 {
            format!("Move the {n} jobs {dir} (drag them, or the keys)")
        } else {
            format!("Move the job {dir} (drag it, or the key)")
        }
    };
    q.set_move_up_tooltip(moving("up").into());
    q.set_move_down_tooltip(moving("down").into());
    q.set_can_remove_finished(finished > 0);
    q.set_remove_finished_tooltip(
        match finished {
            0 => "Remove the finished jobs (there are none)".to_owned(),
            1 => "Remove the finished job from the queue (its file stays)".to_owned(),
            n => format!("Remove the {n} finished jobs from the queue (their files stay)"),
        }
        .into(),
    );
}

impl QueueBridge {
    fn select(&self, id: JobId, ctrl: bool, shift: bool) {
        {
            let mut ui = self.ui.borrow_mut();
            let visible = ui.visible.clone();
            match (ctrl, shift) {
                (_, true) => ui.selection.extend(id, &visible, ctrl),
                (true, false) => ui.selection.toggle(id),
                (false, false) => ui.selection.click(id),
            }
        }
        self.refresh();
    }

    /// Ctrl+A: every shown job.
    pub fn select_all(&self) {
        {
            let mut ui = self.ui.borrow_mut();
            let visible = ui.visible.clone();
            ui.selection.select_all(&visible);
        }
        self.refresh();
    }

    /// The selected jobs in queue order.
    fn selected_rows(&self) -> Vec<JobSummary> {
        let ui = self.ui.borrow();
        let ids = ui.selection.in_order(&ui.visible);
        let shared = self.shared.lock();
        ids.iter().filter_map(|id| shared.view.row(*id).cloned()).collect()
    }

    /// Ctrl+Up/Down/Home/End and the toolbar: the selected jobs move together.
    pub fn move_selected(&self, to: MoveTarget) {
        let ids: Vec<JobId> = self.selected_rows().iter().map(|r| r.id).collect();
        if !ids.is_empty() {
            self.send(QueueCommand::Move { ids, to });
        }
    }

    /// The selected jobs were dropped in front of shown row `index`: they go before the first row from there on that
    /// is not one of them (else to the end).
    fn drop_rows(&self, index: usize) {
        let (ids, before) = {
            let ui = self.ui.borrow();
            let ids = ui.selection.in_order(&ui.visible);
            let before = ui
                .visible
                .iter()
                .skip(index)
                .find(|id| !ui.selection.contains(**id))
                .copied();
            (ids, before)
        };
        if ids.is_empty() {
            return;
        }
        let to = before.map_or(MoveTarget::Bottom, MoveTarget::Before);
        tracing::debug!(jobs = ids.len(), ?to, "[Queue] Jobs dropped");
        self.send(QueueCommand::Move { ids, to });
    }

    /// A double click: a finished job plays its output, a failed or canceled one shows its log, any other one is
    /// shown in the Job panel.
    fn double_clicked(&self, id: JobId) {
        let Some(row) = self.row(id) else { return };
        self.ui.borrow_mut().selection.click(id);
        self.refresh();
        match row.state.kind() {
            JobStateKind::Finished => open_file(&self.runtime, row.target, false),
            JobStateKind::Failed | JobStateKind::Canceled => self.reveal(PanelId::Logs),
            _ => self.reveal(PanelId::Job),
        }
    }

    /// Selects the job and shows its log (a notification's "Show log").
    pub fn show_log(&self, id: JobId) {
        if self.row(id).is_none() {
            return;
        }
        self.ui.borrow_mut().selection.click(id);
        self.refresh();
        self.reveal(PanelId::Logs);
    }

    fn reveal(&self, panel: PanelId) {
        if let Some(hook) = self.hooks.borrow().reveal.as_ref() {
            hook(panel);
        }
    }

    fn open_menu(self: &Rc<Self>, x: f32, y: f32) {
        let Some(menu) = self.menu.upgrade() else { return };
        let selected = self.selected_rows();
        let rows: Vec<&JobSummary> = selected.iter().collect();
        let finished = {
            let shared = self.shared.lock();
            shared
                .view
                .rows
                .iter()
                .filter(|r| r.state.kind() == JobStateKind::Finished)
                .count()
        };
        let profiles = self.hooks.borrow().profiles.as_ref().map(|p| p()).unwrap_or_default();
        let items = queue_menu(&rows, finished, &profiles, &self.keys);
        let weak = Rc::downgrade(self);
        menu.show(&items, x, y, move |id| {
            if let (Some(b), Some(action)) = (weak.upgrade(), QueueAction::from_id(id)) {
                b.run(&action);
            }
        });
    }

    /// Runs `action` on the selected jobs it applies to.
    pub(super) fn run(&self, action: &QueueAction) {
        self.run_rows(action, &self.selected_rows());
    }

    /// Runs `action` on job `id` alone (the Job panel's buttons).
    pub fn run_on(&self, action: &QueueAction, id: JobId) {
        if let Some(row) = self.row(id) {
            self.run_rows(action, &[row]);
        }
    }

    fn run_rows(&self, action: &QueueAction, selected: &[JobSummary]) {
        let rows: Vec<&JobSummary> = selected.iter().collect();
        let ids = targets(action, &rows);
        let first = ids.first().copied();
        match action {
            QueueAction::Edit | QueueAction::LoadAsNew => {
                if let (Some(id), Some(hook)) = (first, self.hooks.borrow().edit.as_ref()) {
                    hook(id);
                }
            }
            QueueAction::Details => self.reveal(PanelId::Job),
            QueueAction::ShowLog => self.reveal(PanelId::Logs),
            QueueAction::EditTags => {
                let name = first.and_then(|id| self.row(id)).map(|r| r.name);
                if let (Some(id), Some(name), Some(hook)) = (first, name, self.hooks.borrow().edit_tags.as_ref()) {
                    hook(id, name);
                }
            }
            QueueAction::Play => {
                if let Some(row) = first.and_then(|id| self.row(id)) {
                    open_file(&self.runtime, row.target, false);
                }
            }
            QueueAction::Reveal => {
                // One file manager window per folder.
                let mut seen = BTreeSet::new();
                for row in rows.iter().filter(|r| ids.contains(&r.id)) {
                    let folder = row.target.parent().map(Path::to_path_buf).unwrap_or_default();
                    if seen.insert(folder.clone()) {
                        if row.state.kind() == JobStateKind::Finished {
                            open_file(&self.runtime, row.target.clone(), true);
                        } else {
                            open_file(&self.runtime, folder, false);
                        }
                    }
                }
            }
            QueueAction::Hold if !ids.is_empty() => self.send(QueueCommand::Hold { ids }),
            QueueAction::Release if !ids.is_empty() => self.send(QueueCommand::Release { ids }),
            QueueAction::Retry if !ids.is_empty() => self.send(QueueCommand::Restart { ids, profile: None }),
            QueueAction::RetryWith(id) if !ids.is_empty() => {
                let profile = self.hooks.borrow().profile.as_ref().and_then(|p| p(id));
                if let Some(profile) = profile {
                    tracing::info!(jobs = ids.len(), profile = %profile.name, "[Queue] Jobs requeued with a profile");
                    self.send(QueueCommand::Restart {
                        ids,
                        profile: Some(Box::new(profile)),
                    });
                } else {
                    tracing::warn!(profile = %id, "[Queue] The profile is gone; nothing requeued");
                }
            }
            QueueAction::MoveTop if !ids.is_empty() => self.send(QueueCommand::Move {
                ids,
                to: MoveTarget::Top,
            }),
            QueueAction::MoveBottom if !ids.is_empty() => self.send(QueueCommand::Move {
                ids,
                to: MoveTarget::Bottom,
            }),
            QueueAction::Cancel if !ids.is_empty() => {
                tracing::info!(jobs = ids.len(), "[Queue] Jobs canceled in the panel");
                self.send(QueueCommand::Cancel { ids });
            }
            QueueAction::Remove if !ids.is_empty() => {
                tracing::info!(jobs = ids.len(), "[Queue] Jobs removed in the panel");
                let (reply, _answer) = tokio::sync::oneshot::channel();
                self.send(QueueCommand::Remove { ids, reply });
            }
            QueueAction::RemoveFinished => {
                let (reply, _answer) = tokio::sync::oneshot::channel();
                self.send(QueueCommand::RemoveFinished { reply });
            }
            _ => {}
        }
    }
}

/// Plays a file in the default player, or shows it (or a folder) in the file manager, off the UI thread.
pub fn open_file(runtime: &tokio::runtime::Handle, path: PathBuf, reveal: bool) {
    runtime.spawn_blocking(move || {
        let result = if reveal {
            opener::reveal(&path)
        } else {
            opener::open(&path)
        };
        if let Err(e) = result {
            tracing::warn!(file = %path.display(), error = %e, "[Queue] The file could not be opened");
        }
    });
}
