//! Dragging jobs in the queue list (P9.4, the user's review note): the selected jobs (the grabbed one is selected
//! first) move as one block; the other rows move aside to open the gap where they would land, a ghost with the grabbed
//! job's name follows the pointer, the list scrolls near its edges (`queue_panel.slint` asks `ui::drag::auto_scroll`)
//! and Escape cancels. The order changes only on the drop (`drop_rows`, a `QueueCommand::Move`); until the moved
//! order arrives the rows keep their places in the new layout, then they snap without animation, so nothing jumps.

use std::rc::Rc;
use std::time::Duration;

use rustcut_core::job::JobId;
use slint::{ComponentHandle as _, Model as _, ModelRc};

use super::QueueBridge;
use crate::ui::drag::{self, DragFrame, ListDrag};
use crate::{MainWindow, Queue};

/// How long the rows wait in their new places for the moved order before they go back (a refused move).
const SETTLE: Duration = Duration::from_millis(800);

/// A drag of the queue's rows.
pub(super) struct QueueDrag {
    model: ListDrag,
    /// The shown rows when it started: another set or order ends it.
    ids: Vec<JobId>,
    last: Option<DragFrame>,
    /// Dropped: the rows wait for the moved order.
    settling: bool,
}

impl QueueBridge {
    /// A drag starts at shown row `row` (already selected), the pointer at `pointer` (content coordinates).
    fn drag_start(self: &Rc<Self>, row: usize, pointer: f32, pitch: f32, height: f32) {
        let Some(window) = self.window.upgrade() else { return };
        let (ids, dragged) = {
            let ui = self.ui.borrow();
            let dragged: Vec<bool> = ui.visible.iter().map(|id| ui.selection.contains(*id)).collect();
            (ui.visible.clone(), dragged)
        };
        let count = dragged.iter().filter(|d| **d).count();
        let Some(model) = ListDrag::new(vec![height; ids.len()], pitch - height, dragged, row, pointer) else {
            return;
        };
        tracing::debug!(jobs = count, row, "[Queue] Drag started");
        let q = window.global::<Queue<'_>>();
        q.set_drag_title(self.rows.row_data(row).map(|r| r.name).unwrap_or_default());
        q.set_drag_detail(
            match count {
                1 => String::new(),
                n => format!("{n} jobs"),
            }
            .into(),
        );
        q.set_drag_count(i32::try_from(count).unwrap_or(i32::MAX));
        *self.drag.borrow_mut() = Some(QueueDrag {
            model,
            ids,
            last: None,
            settling: false,
        });
        let weak = Rc::downgrade(self);
        drag::begin(&window, move || {
            if let Some(bridge) = weak.upgrade() {
                bridge.drag_finish(false);
            }
        });
        self.drag_move(pointer);
        q.set_dragging(true);
    }

    /// The pointer moved (or the list scrolled under it).
    fn drag_move(&self, pointer: f32) {
        let frame = {
            let mut drag = self.drag.borrow_mut();
            let Some(drag) = drag.as_mut().filter(|d| !d.settling) else {
                return;
            };
            let frame = drag.model.frame(pointer);
            tracing::trace!(
                pointer,
                drop = frame.drop,
                ghost = frame.ghost_top,
                "[Queue] Drag moved"
            );
            drag.last = Some(frame.clone());
            frame
        };
        self.show_frame(&frame);
    }

    fn show_frame(&self, frame: &DragFrame) {
        let Some(window) = self.window.upgrade() else { return };
        let q = window.global::<Queue<'_>>();
        self.drag_shifts.set_vec(frame.shifts.clone());
        q.set_drag_gap_y(frame.gap_top);
        q.set_drag_gap_height(frame.gap_height);
        q.set_drag_ghost_y(frame.ghost_top);
    }

    /// The button was released (`drop`) or Escape pressed: a drop that changes the order moves the jobs.
    fn drag_finish(self: &Rc<Self>, drop: bool) {
        let Some(window) = self.window.upgrade() else { return };
        drag::end(&window);
        window.global::<Queue<'_>>().set_dragging(false);
        let target = {
            let mut slot = self.drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let target = drag.last.as_ref().filter(|f| drop && f.moves).map(|f| f.drop);
            drag.settling = target.is_some();
            target
        };
        let Some(index) = target else {
            tracing::debug!(dropped = drop, "[Queue] Drag ended without a move");
            self.drag.borrow_mut().take();
            self.drag_shifts.set_vec(Vec::new());
            return;
        };
        self.drop_rows(index);
        // A move the queue refuses never brings a new order: the rows go back after a while.
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(SETTLE, move || {
            if let Some(bridge) = weak.upgrade()
                && bridge.drag.borrow().as_ref().is_some_and(|d| d.settling)
            {
                bridge.drag.borrow_mut().take();
                bridge.drag_shifts.set_vec(Vec::new());
            }
        });
    }

    /// The shown rows were published (`refresh`): a dropped drag ends when its new order is there (the rows snap
    /// into it without animation); a running drag ends when the rows changed under it.
    pub(super) fn drag_rows_changed(&self, visible: &[JobId]) {
        let (settled, stale) = {
            let drag = self.drag.borrow();
            let Some(drag) = drag.as_ref() else { return };
            (
                drag.settling && drag.ids != visible,
                !drag.settling && drag.ids != visible,
            )
        };
        if !settled && !stale {
            return;
        }
        let Some(window) = self.window.upgrade() else { return };
        self.drag.borrow_mut().take();
        if stale {
            tracing::debug!("[Queue] Drag canceled: the shown jobs changed");
            drag::end(&window);
            window.global::<Queue<'_>>().set_dragging(false);
            self.drag_shifts.set_vec(Vec::new());
            return;
        }
        let q = window.global::<Queue<'_>>();
        q.set_drag_instant(true);
        self.drag_shifts.set_vec(Vec::new());
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::from_millis(50), move || {
            if let Some(window) = weak.upgrade() {
                window.global::<Queue<'_>>().set_drag_instant(false);
            }
        });
    }
}

pub(super) fn connect(bridge: &Rc<QueueBridge>, window: &MainWindow) {
    let q = window.global::<Queue<'_>>();
    q.set_drag_shifts(ModelRc::from(Rc::clone(&bridge.drag_shifts)));
    let weak = Rc::downgrade(bridge);
    q.on_drag_start(move |row, pointer, pitch, height| {
        if let (Some(b), Ok(row)) = (weak.upgrade(), usize::try_from(row)) {
            b.drag_start(row, pointer, pitch, height);
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_drag_move(move |pointer| {
        if let Some(b) = weak.upgrade() {
            b.drag_move(pointer);
        }
    });
    let weak = Rc::downgrade(bridge);
    q.on_drag_end(move || {
        if let Some(b) = weak.upgrade() {
            b.drag_finish(true);
        }
    });
}
