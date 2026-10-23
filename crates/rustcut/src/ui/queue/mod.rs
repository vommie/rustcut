//! The queue panel's bridge: it follows the queue engine's events on the runtime into a shared [`QueueView`] and asks
//! the UI thread to show it at most ten times a second (the latest view wins, `UiDispatcher::post_latest`); the
//! panel's buttons send commands to the engine without waiting.

mod view;

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use parking_lot::Mutex;
use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::naming::QueuedTarget;
use rustcut_core::queue::{MoveTarget, QueueCommand, QueueHandle, QueueService};
use slint::{ComponentHandle as _, ModelRc, VecModel};
use tokio::sync::broadcast::error::RecvError;

pub use view::{QueueView, RowText, clock, title};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::util::model::sync;
use crate::{MainWindow, Queue, QueueRowView};

const REFRESH: CoalesceKey = CoalesceKey::new("queue", 0);

thread_local! {
    /// The bridge, for the refreshes the dispatcher runs on the UI thread.
    static BRIDGE: RefCell<Weak<QueueBridge>> = const { RefCell::new(Weak::new()) };
}

/// The view as the runtime task keeps it, plus whether a job ended since the last refresh.
#[derive(Default)]
struct Shared {
    view: QueueView,
    ended: bool,
}

pub struct QueueBridge {
    window: slint::Weak<MainWindow>,
    handle: QueueHandle,
    shared: Arc<Mutex<Shared>>,
    rows: Rc<VecModel<QueueRowView>>,
    /// Called on the UI thread after a job ended (the export panel looks at the target folder again).
    on_job_ended: RefCell<Option<Box<dyn Fn()>>>,
}

impl QueueBridge {
    /// Follows `service` and handles the panel's buttons. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: &tokio::runtime::Handle,
        service: &QueueService,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            handle: service.handle.clone(),
            shared: Arc::new(Mutex::new(Shared::default())),
            rows: Rc::new(VecModel::default()),
            on_job_ended: RefCell::new(None),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        window
            .global::<Queue<'_>>()
            .set_rows(ModelRc::from(Rc::clone(&bridge.rows)));
        connect(&bridge, window);
        runtime.spawn(follow(service.clone(), Arc::clone(&bridge.shared), ui.clone()));
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

    pub fn set_on_job_ended(&self, hook: impl Fn() + 'static) {
        *self.on_job_ended.borrow_mut() = Some(Box::new(hook));
    }

    /// Shows the current view (UI thread).
    fn refresh(&self) {
        let (texts, summary, selected, paused, ended) = {
            let mut shared = self.shared.lock();
            let ended = std::mem::take(&mut shared.ended);
            let view = &shared.view;
            (
                view.row_texts(),
                view.summary(),
                view.selected_row().map(|r| (r.id, r.state.kind())),
                view.paused(),
                ended,
            )
        };
        let Some(window) = self.window.upgrade() else { return };
        let position = selected.and_then(|(id, _)| texts.iter().position(|t| t.id == id));
        let rows = texts
            .into_iter()
            .map(|t| QueueRowView {
                id: i32::try_from(t.id.0).unwrap_or(i32::MAX),
                name: t.name.into(),
                state: t.state.into(),
                kind: t.kind,
                progress: t.progress,
                detail: t.detail.into(),
                tooltip: t.tooltip.into(),
                selected: Some(t.id) == selected.map(|(id, _)| id),
            })
            .collect::<Vec<_>>();
        let count = rows.len();
        sync(&self.rows, rows);
        let q = window.global::<Queue<'_>>();
        q.set_summary(summary.into());
        q.set_paused(paused);
        let kind = selected.map(|(_, k)| k);
        q.set_has_selection(kind.is_some());
        q.set_can_cancel(kind.is_some_and(|k| !k.is_done()));
        q.set_can_remove(kind.is_some_and(|k| !k.is_active()));
        q.set_can_retry(kind.is_some_and(JobStateKind::is_done));
        q.set_can_hold(matches!(kind, Some(JobStateKind::Waiting | JobStateKind::Held)));
        q.set_held(kind == Some(JobStateKind::Held));
        q.set_can_move_up(position.is_some_and(|p| p > 0));
        q.set_can_move_down(position.is_some_and(|p| p + 1 < count));
        if ended && let Some(hook) = self.on_job_ended.borrow().as_ref() {
            hook();
        }
    }

    fn selected(&self) -> Option<(JobId, JobStateKind)> {
        let shared = self.shared.lock();
        shared.view.selected_row().map(|r| (r.id, r.state.kind()))
    }

    fn select(&self, id: i32) {
        {
            let mut shared = self.shared.lock();
            let id = JobId(i64::from(id));
            shared.view.selected = (shared.view.selected != Some(id)).then_some(id);
        }
        self.refresh();
    }

    fn send(&self, command: QueueCommand) {
        if self.handle.send(command).is_err() {
            tracing::warn!("[Queue] The queue engine has stopped; the command was dropped");
        }
    }

    fn act(&self, action: Act) {
        let Some((id, kind)) = self.selected() else { return };
        let ids = vec![id];
        match action {
            Act::Cancel => self.send(QueueCommand::Cancel { ids }),
            Act::Retry => self.send(QueueCommand::Restart { ids, profile: None }),
            Act::Hold if kind == JobStateKind::Held => self.send(QueueCommand::Release { ids }),
            Act::Hold => self.send(QueueCommand::Hold { ids }),
            Act::Up => self.send(QueueCommand::Move {
                ids,
                to: MoveTarget::Up,
            }),
            Act::Down => self.send(QueueCommand::Move {
                ids,
                to: MoveTarget::Down,
            }),
            Act::Remove => {
                let (reply, _answer) = tokio::sync::oneshot::channel();
                self.send(QueueCommand::Remove { ids, reply });
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Act {
    Cancel,
    Retry,
    Hold,
    Up,
    Down,
    Remove,
}

fn connect(bridge: &Rc<QueueBridge>, window: &MainWindow) {
    let q = window.global::<Queue<'_>>();
    let weak = Rc::downgrade(bridge);
    q.on_select(move |id| {
        if let Some(b) = weak.upgrade() {
            b.select(id);
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
    q.on_cancel(bind(bridge, Act::Cancel));
    q.on_retry(bind(bridge, Act::Retry));
    q.on_toggle_hold(bind(bridge, Act::Hold));
    q.on_move_up(bind(bridge, Act::Up));
    q.on_move_down(bind(bridge, Act::Down));
    q.on_remove(bind(bridge, Act::Remove));
}

/// A button's handler: `action` on the selected job.
fn bind(bridge: &Rc<QueueBridge>, action: Act) -> impl FnMut() + 'static {
    let weak = Rc::downgrade(bridge);
    move || {
        if let Some(b) = weak.upgrade() {
            b.act(action);
        }
    }
}

/// Follows the engine's events into the shared view; the UI shows it through coalesced refreshes. A subscriber that
/// fell behind takes a fresh snapshot.
async fn follow(service: QueueService, shared: Arc<Mutex<Shared>>, ui: UiDispatcher) {
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
    let mut resync = true;
    loop {
        if resync {
            match service.handle.request(|reply| QueueCommand::Snapshot { reply }).await {
                Ok(snapshot) => shared.lock().view.replace(snapshot),
                Err(_gone) => return,
            }
            refresh(&ui);
            resync = false;
        }
        match events.recv().await {
            Ok(event) => {
                let mut s = shared.lock();
                if s.view.apply(event) {
                    s.ended = true;
                }
                drop(s);
                refresh(&ui);
            }
            Err(RecvError::Lagged(missed)) => {
                tracing::debug!(missed, "[Queue] The panel fell behind; taking a snapshot");
                resync = true;
            }
            Err(RecvError::Closed) => return,
        }
    }
}
