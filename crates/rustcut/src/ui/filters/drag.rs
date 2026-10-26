//! Dragging a filter to another place in the chain (P9.13 with the drag & drop module of P9.4, the review's notes 12 and
//! 12#1): the other rows move aside to open the gap where it would land, a ghost with its name follows the pointer, the
//! Export panel's card stack scrolls near its edges (`DragState.area`) and Escape cancels. The order changes on the
//! drop (`FiltersState::drop_filter`), at once: the rows snap into it without animation. Crop's row is two lines high.

use std::rc::{Rc, Weak};
use std::time::Duration;

use rustcut_core::edit::FilterKind;
use slint::{ComponentHandle as _, ModelRc};

use super::{FiltersBridge, view};
use crate::ui::drag::{self, DragFrame, ListDrag};
use crate::{Filters, MainWindow};

/// The card stack the Filters card lies in (`dock_panels.slint`).
const AREA: &str = "export";

/// A drag of one filter row.
pub(super) struct FiltersDrag {
    model: ListDrag,
    kind: FilterKind,
    /// The order when it started: another order ends it.
    order: Vec<FilterKind>,
    last: Option<DragFrame>,
}

impl FiltersBridge {
    /// A drag starts at row `row`, the pointer at `pointer` (the list's coordinates).
    fn drag_start(&self, row: usize, pointer: f32) {
        let Some(window) = self.window.upgrade() else { return };
        let rows = view::card(&self.state.borrow()).rows;
        let Some(kind) = rows.get(row).map(|r| r.kind) else {
            return;
        };
        let f = window.global::<Filters<'_>>();
        let line = f.get_line_height();
        let heights: Vec<f32> = rows.iter().map(|r| line * lines(r)).collect();
        let dragged: Vec<bool> = (0..rows.len()).map(|i| i == row).collect();
        let ghost = heights.get(row).copied().unwrap_or(line);
        let Some(model) = ListDrag::new(heights, f.get_row_spacing(), dragged, row, pointer) else {
            return;
        };
        tracing::debug!(filter = kind.label(), row, "[Filters] Drag started");
        self.close_menu();
        f.set_drag_kind(super::kind_id(kind).into());
        f.set_drag_title(kind.label().into());
        f.set_drag_ghost_height(ghost);
        *self.drag.borrow_mut() = Some(FiltersDrag {
            model,
            kind,
            order: self.state.borrow().chain().order().collect(),
            last: None,
        });
        let me = Weak::clone(&self.me);
        drag::begin(&window, AREA, move || {
            if let Some(bridge) = me.upgrade() {
                bridge.drag_finish(false);
            }
        });
        self.drag_move(pointer);
        f.set_dragging(true);
    }

    /// The pointer moved (or the card stack scrolled under it).
    fn drag_move(&self, pointer: f32) {
        let frame = {
            let mut slot = self.drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let frame = drag.model.frame(pointer);
            tracing::trace!(pointer, drop = frame.drop, "[Filters] Drag moved");
            drag.last = Some(frame.clone());
            frame
        };
        let Some(window) = self.window.upgrade() else { return };
        let f = window.global::<Filters<'_>>();
        self.drag_shifts.set_vec(frame.shifts.clone());
        f.set_drag_gap_y(frame.gap_top);
        f.set_drag_gap_height(frame.gap_height);
        f.set_drag_ghost_y(frame.ghost_top);
    }

    /// The button was released (`drop`) or Escape pressed: a drop that changes the order moves the filter.
    pub(super) fn drag_finish(&self, drop: bool) {
        let Some(window) = self.window.upgrade() else { return };
        let Some(drag) = self.drag.borrow_mut().take() else {
            return;
        };
        drag::end(&window);
        let stale = self.state.borrow().chain().order().ne(drag.order.iter().copied());
        let target = drag.last.filter(|f| drop && f.moves && !stale).map(|f| f.drop);
        let moved = target.is_some_and(|before| self.state.borrow_mut().drop_filter(drag.kind, before));
        tracing::debug!(dropped = drop, moved, "[Filters] Drag ended");
        let f = window.global::<Filters<'_>>();
        f.set_drag_instant(moved);
        f.set_dragging(false);
        self.drag_shifts.set_vec(Vec::new());
        if moved {
            self.publish();
            let weak = window.as_weak();
            slint::Timer::single_shot(Duration::from_millis(50), move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<Filters<'_>>().set_drag_instant(false);
                }
            });
        }
    }
}

/// A row's height in lines, as a length factor.
fn lines(row: &view::RowView) -> f32 {
    if row.lines() > 1 { 2.0 } else { 1.0 }
}

pub(super) fn connect(bridge: &Rc<FiltersBridge>, window: &MainWindow) {
    let f = window.global::<Filters<'_>>();
    f.set_drag_shifts(ModelRc::from(Rc::clone(&bridge.drag_shifts)));
    let weak = Rc::downgrade(bridge);
    f.on_drag_start(move |row, pointer| {
        if let (Some(b), Ok(row)) = (weak.upgrade(), usize::try_from(row)) {
            b.drag_start(row, pointer);
        }
    });
    let weak = Rc::downgrade(bridge);
    f.on_drag_move(move |pointer| {
        if let Some(b) = weak.upgrade() {
            b.drag_move(pointer);
        }
    });
    let weak = Rc::downgrade(bridge);
    f.on_drag_end(move || {
        if let Some(b) = weak.upgrade() {
            b.drag_finish(true);
        }
    });
}
