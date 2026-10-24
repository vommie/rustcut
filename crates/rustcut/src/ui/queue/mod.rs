//! The queue panel's bridge (M4.1): it follows the queue engine's events on the runtime into a shared [`QueueView`] and
//! asks the UI thread to show it at most ten times a second (the latest view wins, `UiDispatcher::post_latest`). The
//! panel's own state lives on the UI thread: the selection, the filter, the column widths and the panel's width. The
//! actions (`actions.rs`) send commands to the engine without waiting; the Job and Logs panels and the editor follow
//! the current job through hooks.

mod actions;
mod columns;
mod filter;
mod menu;
mod selection;
mod text;
mod totals;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use az::CheckedAs as _;
use parking_lot::Mutex;
use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::naming::QueuedTarget;
use rustcut_core::queue::{JobProgress, JobSummary, QueueCommand, QueueEvent, QueueHandle, QueueService};
use rustcut_core::render::RenderProfile;
use rustcut_core::settings::QueuePanelState;
use slint::{ComponentHandle as _, ModelRc, VecModel};
use tokio::sync::broadcast::error::RecvError;

pub use self::columns::{Column, Layout, layout};
pub use self::filter::{CHIPS, Chip, Filter};
pub use self::menu::{MenuKeys, QueueAction, queue_menu, targets};
pub use self::selection::Selection;
pub use self::text::{RowText, clock, file_label, kind_number, row_text, short_clock, title};
pub use self::totals::Overall;
pub use self::view::QueueView;

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::ui::dock::PanelId;
use crate::ui::menu::ContextMenuBridge;
use crate::util::model::sync;
use crate::{MainWindow, Queue, QueueChipView, QueueColumnView, QueueRowView};

const REFRESH: CoalesceKey = CoalesceKey::new("queue", 0);
/// Row heights of the two forms.
const COMPACT_ROW: f32 = 44.0;
const TABLE_ROW: f32 = 26.0;
/// The rows' padding left and right (`queue_row.slint`).
const ROW_PADDING: f32 = 16.0;

thread_local! {
    /// The bridge, for the refreshes the dispatcher runs on the UI thread.
    static BRIDGE: RefCell<Weak<QueueBridge>> = const { RefCell::new(Weak::new()) };
}

/// The view as the runtime task keeps it, plus whether a job ended since the last refresh.
#[derive(Default)]
struct Shared {
    view: QueueView,
    ended: bool,
    /// The queue reported that it is done (finished, failed) since the last refresh.
    done: Option<(u32, u32)>,
    /// Jobs whose render ended (finished, failed or canceled) since the last refresh.
    ended_jobs: Vec<JobSummary>,
}

/// How busy the queue is (the desktop integration: sleep lock, taskbar, close warning).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueueStatus {
    /// Jobs rendering now (not paused).
    pub rendering: usize,
    /// Jobs rendering or paused in the middle of a render.
    pub active: usize,
    /// Processing is paused (nothing new starts).
    pub processing_paused: bool,
    /// Jobs waiting, rendering or paused.
    pub open: usize,
    /// Jobs put on hold.
    pub held: usize,
    pub overall: Option<Overall>,
}

/// What the panel arranged (UI thread).
#[derive(Debug, Default)]
struct PanelUi {
    selection: Selection,
    filter: Filter,
    widths: BTreeMap<String, u32>,
    /// The list's inner width in logical pixels.
    width: f32,
    /// A column whose edge is dragged, and its width when the drag started.
    resizing: Option<(Column, u32)>,
    /// The rows as shown (the filter's result), in queue order.
    visible: Vec<JobId>,
}

/// The render profiles (id, name) for "Render again with".
/// See [`QueueHooks::job_done`].
pub type JobDoneHook = Box<dyn Fn(&JobSummary)>;
pub type ProfilesHook = Box<dyn Fn() -> Vec<(String, String)>>;
/// The profile of an id ("" = the Codecs panel's current settings).
pub type ProfileHook = Box<dyn Fn(&str) -> Option<RenderProfile>>;

/// What other parts of the app do for the queue panel.
#[derive(Default)]
pub struct QueueHooks {
    /// A job ended (the export panel looks at the target folder again).
    pub job_ended: Option<Box<dyn Fn()>>,
    /// "Edit tags and rating" (id, name): the Tag & Rate panel takes it.
    pub edit_tags: Option<Box<dyn Fn(JobId, String)>>,
    /// "Edit in the editor" / "Load as a new job".
    pub edit: Option<Box<dyn Fn(JobId)>>,
    /// Shows a dock panel (the Job or the Logs panel).
    pub reveal: Option<Box<dyn Fn(PanelId)>>,
    /// After every refresh (the Job and Logs panels follow the current job).
    pub refreshed: Option<Box<dyn Fn()>>,
    /// After every refresh, how busy the queue is (the power bridge and the tray).
    pub status: Vec<Box<dyn Fn(QueueStatus)>>,
    /// The last waiting job ended (finished, failed): the "when done" action follows.
    pub done: Option<Box<dyn Fn(u32, u32)>>,
    /// A job's render ended (finished, failed or canceled; not a later edit of a done job): its notification.
    pub job_done: Option<JobDoneHook>,
    pub profiles: Option<ProfilesHook>,
    pub profile: Option<ProfileHook>,
}

pub struct QueueBridge {
    window: slint::Weak<MainWindow>,
    handle: QueueHandle,
    runtime: tokio::runtime::Handle,
    shared: Arc<Mutex<Shared>>,
    ui: RefCell<PanelUi>,
    rows: Rc<VecModel<QueueRowView>>,
    columns: Rc<VecModel<QueueColumnView>>,
    chips: Rc<VecModel<QueueChipView>>,
    menu: Weak<ContextMenuBridge>,
    hooks: RefCell<QueueHooks>,
    /// The job loaded into the editor (its row is marked).
    editing: Cell<Option<JobId>>,
    keys: MenuKeys,
    empty_text: String,
}

impl QueueBridge {
    /// Follows `service` and handles the panel. The app owns the only strong reference.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts the panel connects; called once at startup"
    )]
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: &tokio::runtime::Handle,
        service: &QueueService,
        saved: &QueuePanelState,
        menu: &Rc<ContextMenuBridge>,
        keys: MenuKeys,
        save_key: &str,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            handle: service.handle.clone(),
            runtime: runtime.clone(),
            shared: Arc::new(Mutex::new(Shared::default())),
            ui: RefCell::new(PanelUi {
                selection: Selection::restore(&saved.selected, saved.current),
                filter: Filter::new(&saved.state_filter, &saved.search),
                widths: saved.columns.clone(),
                ..PanelUi::default()
            }),
            rows: Rc::new(VecModel::default()),
            columns: Rc::new(VecModel::default()),
            chips: Rc::new(VecModel::default()),
            menu: Rc::downgrade(menu),
            hooks: RefCell::new(QueueHooks::default()),
            editing: Cell::new(None),
            keys,
            empty_text: if save_key.is_empty() {
                "Saved jobs appear here.".to_owned()
            } else {
                format!("Saved jobs appear here ({save_key}).")
            },
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        let q = window.global::<Queue<'_>>();
        q.set_rows(ModelRc::from(Rc::clone(&bridge.rows)));
        q.set_columns(ModelRc::from(Rc::clone(&bridge.columns)));
        q.set_chips(ModelRc::from(Rc::clone(&bridge.chips)));
        q.set_search(saved.search.as_str().into());
        q.set_key_move_up(bridge.keys.move_up.as_str().into());
        q.set_key_move_down(bridge.keys.move_down.as_str().into());
        actions::connect(&bridge, window);
        runtime.spawn(follow(
            service.clone(),
            Arc::clone(&bridge.shared),
            ui.clone(),
            runtime.clone(),
        ));
        bridge
    }

    pub fn handle(&self) -> &QueueHandle {
        &self.handle
    }

    /// Every job's target (the export panel's conflict check).
    pub fn targets(&self) -> Vec<QueuedTarget> {
        self.shared.lock().view.targets()
    }

    /// Processing is paused (saved in `ui-state.toml`).
    pub fn paused(&self) -> bool {
        self.shared.lock().view.paused()
    }

    /// The other parts' hooks (set once after they are installed).
    pub fn hooks(&self) -> std::cell::RefMut<'_, QueueHooks> {
        self.hooks.borrow_mut()
    }

    /// What `ui-state.toml` keeps of the panel.
    pub fn save_into(&self, state: &mut QueuePanelState) {
        state.paused = self.paused();
        let ui = self.ui.borrow();
        state.state_filter.clone_from(&ui.filter.states);
        state.search.clone_from(&ui.filter.search);
        state.columns.clone_from(&ui.widths);
        state.selected = ui.selection.ids().to_vec();
        state.current = ui.selection.current();
    }

    /// The job the Job and Logs panels show, with its progress and measured size.
    pub fn current(&self) -> Option<(JobSummary, Option<JobProgress>, Option<u64>)> {
        let id = self.ui.borrow().selection.current()?;
        let shared = self.shared.lock();
        let row = shared.view.row(id)?.clone();
        Some((row, shared.view.progress.get(&id).copied(), shared.view.size(id)))
    }

    /// Makes `id` the current job (the Job and Logs panels show it).
    pub fn set_current(&self, id: JobId) {
        self.ui.borrow_mut().selection.set_current(Some(id));
        self.refresh();
    }

    /// A jump mark leads to job `id` (a failed job's toast): it becomes the only selected and the current job; a
    /// filter that hides it is cleared so its row can show.
    pub fn show_job(&self, id: JobId) {
        {
            let mut ui = self.ui.borrow_mut();
            ui.selection.click(id);
        }
        let hidden = {
            let ui = self.ui.borrow();
            ui.filter.is_active()
                && self
                    .shared
                    .lock()
                    .view
                    .row(id)
                    .is_some_and(|row| !ui.filter.matches(row))
        };
        if hidden {
            tracing::info!(job = id.0, "[Queue] The filter hid the job a message leads to; cleared");
            self.ui.borrow_mut().filter.clear();
        }
        self.refresh();
    }

    /// The job loaded into the editor (its row is marked), or none.
    pub fn set_editing(&self, id: Option<JobId>) {
        self.editing.set(id);
        self.refresh();
    }

    /// One row of the queue as it is known now.
    pub fn row(&self, id: JobId) -> Option<JobSummary> {
        self.shared.lock().view.row(id).cloned()
    }

    /// Shows the current view (UI thread).
    fn refresh(&self) {
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
        let (texts, visible, counts, summary, overall, paused, pause_after, finished, selected, ended) = {
            let mut shared = self.shared.lock();
            let ended = std::mem::take(&mut shared.ended);
            let view = &shared.view;
            let rows: Vec<&JobSummary> = view.rows.iter().filter(|r| ui.filter.matches(r)).collect();
            let visible: Vec<JobId> = rows.iter().map(|r| r.id).collect();
            ui.selection
                .retain(|id| visible.contains(&id), |id| view.row(id).is_some());
            let texts: Vec<RowText> = rows
                .iter()
                .map(|r| row_text(r, view.progress.get(&r.id), view.size(r.id), &shown))
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
                Filter::counts(&view.rows),
                view.summary(),
                view.overall(),
                view.paused(),
                view.pause_after_current(),
                view.rows
                    .iter()
                    .filter(|r| r.state.kind() == JobStateKind::Finished)
                    .count(),
                selected,
                ended,
            )
        };
        ui.visible.clone_from(&visible);
        let current = ui.selection.current();
        let editing = self.editing.get();
        let rows: Vec<QueueRowView> = texts
            .into_iter()
            .map(|t| QueueRowView {
                id: i32::try_from(t.id.0).unwrap_or(i32::MAX),
                selected: ui.selection.contains(t.id),
                current: current == Some(t.id),
                editing: editing == Some(t.id),
                name: t.name.into(),
                state: t.state.into(),
                kind: t.kind,
                progress: t.progress,
                detail: t.detail.into(),
                tooltip: t.tooltip.into(),
                cells: ModelRc::new(VecModel::from(
                    t.cells
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<slint::SharedString>>(),
                )),
            })
            .collect();
        let row_count = rows.len();
        sync(&self.rows, rows);
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
                resizable: s.column != Column::Name,
            })
            .collect();
        sync(&self.columns, column_views);
        let chips = CHIPS
            .iter()
            .zip(counts)
            .map(|(chip, count)| QueueChipView {
                id: chip.id.into(),
                title: chip.title.into(),
                count: i32::try_from(count).unwrap_or(i32::MAX),
                on: ui.filter.is_on(chip),
            })
            .collect();
        sync(&self.chips, chips);
        let q = window.global::<Queue<'_>>();
        q.set_compact(layout.compact);
        q.set_row_height(if layout.compact { COMPACT_ROW } else { TABLE_ROW });
        q.set_filtered(ui.filter.is_active());
        let total: usize = counts.iter().sum();
        q.set_filter_note(format!("{row_count} of {total} jobs shown").into());
        q.set_empty_text(
            if total > 0 && ui.filter.is_active() {
                "No job matches the filter."
            } else {
                self.empty_text.as_str()
            }
            .into(),
        );
        q.set_summary(summary.as_str().into());
        q.set_has_overall(overall.is_some());
        if let Some(o) = overall {
            q.set_overall(az::Az::az::<f32>(o.fraction));
            q.set_overall_text(overall_text(&summary, &o, paused).into());
        }
        q.set_paused(paused);
        q.set_pause_after(pause_after);
        drop(ui);
        actions::publish_toolbar(&q, &selected, &visible, finished);
        if ended && let Some(hook) = self.hooks.borrow().job_ended.as_ref() {
            hook();
        }
        if let Some(hook) = self.hooks.borrow().refreshed.as_ref() {
            hook();
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

    /// How busy the queue is now.
    pub fn status(&self) -> QueueStatus {
        status_of(&self.shared.lock().view)
    }

    fn send(&self, command: QueueCommand) {
        if self.handle.send(command).is_err() {
            tracing::warn!("[Queue] The queue engine has stopped; the command was dropped");
        }
    }
}

/// "1 running · 2 waiting — 2 of 5 done · about 0:12:30 left".
fn overall_text(summary: &str, o: &Overall, paused: bool) -> String {
    let mut text = format!("{summary} — {} of {} done", o.done, o.total);
    match (o.eta_secs, paused) {
        (_, true) if o.done < o.total => text.push_str(" · paused"),
        (Some(eta), _) => {
            let eta = short_clock(rustcut_core::time::Timestamp::from_secs(
                i64::try_from(eta).unwrap_or(i64::MAX),
            ));
            let _ = if o.partial {
                write!(text, " · at least {eta} left")
            } else {
                write!(text, " · about {eta} left")
            };
        }
        _ => {}
    }
    text
}

/// Follows the engine's events into the shared view; the UI shows it through coalesced refreshes. A subscriber that
/// fell behind takes a fresh snapshot. Finished outputs are measured on the runtime.
async fn follow(service: QueueService, shared: Arc<Mutex<Shared>>, ui: UiDispatcher, runtime: tokio::runtime::Handle) {
    let mut events = service.events.subscribe();
    let refresh = |ui: &UiDispatcher| {
        ui.post_latest(REFRESH, rate::PROGRESS, || {
            BRIDGE.with(|b| {
                if let Some(bridge) = b.borrow().upgrade() {
                    bridge.refresh();
                }
            });
        });
    };
    let now = || chrono::Utc::now().timestamp_millis();
    let mut resync = true;
    loop {
        if resync {
            match service.handle.request(|reply| QueueCommand::Snapshot { reply }).await {
                Ok(snapshot) => shared.lock().view.replace(snapshot, now()),
                Err(_gone) => return,
            }
            refresh(&ui);
            resync = false;
        } else {
            match events.recv().await {
                Ok(event) => {
                    let mut s = shared.lock();
                    if let QueueEvent::Done { finished, failed } = &event {
                        s.done = Some((*finished, *failed));
                    }
                    if let QueueEvent::Updated(row) = &event
                        && row.state.kind().is_done()
                        && s.view.row(row.id).is_some_and(|before| before.state.kind().is_active())
                    {
                        s.ended_jobs.push((**row).clone());
                    }
                    if s.view.apply(event, now()) {
                        s.ended = true;
                    }
                    drop(s);
                    refresh(&ui);
                }
                Err(RecvError::Lagged(missed)) => {
                    tracing::debug!(missed, "[Queue] The panel fell behind; taking a snapshot");
                    resync = true;
                    continue;
                }
                Err(RecvError::Closed) => return,
            }
        }
        let unmeasured = shared.lock().view.take_unmeasured();
        for (id, path) in unmeasured {
            let shared = Arc::clone(&shared);
            let ui = ui.clone();
            runtime.spawn(async move {
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    return;
                };
                shared.lock().view.sizes.insert(id, (path, Some(meta.len())));
                refresh(&ui);
            });
        }
    }
}

fn status_of(view: &QueueView) -> QueueStatus {
    let kinds = || view.rows.iter().map(|r| r.state.kind());
    QueueStatus {
        rendering: kinds().filter(|k| *k == JobStateKind::Rendering).count(),
        active: kinds().filter(|k| k.is_active()).count(),
        processing_paused: view.paused(),
        open: kinds()
            .filter(|k| {
                matches!(
                    k,
                    JobStateKind::Waiting | JobStateKind::Rendering | JobStateKind::Paused
                )
            })
            .count(),
        held: kinds().filter(|k| *k == JobStateKind::Held).count(),
        overall: view.overall(),
    }
}
