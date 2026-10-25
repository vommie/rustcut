//! Dragging sections in the Sections card (P9.5 with the drag & drop module of P9.4, the review's note 07#3): the
//! selected sections (the grabbed one is selected first) move as one block; the other rows move aside to open the gap
//! where they would land, a ghost with the grabbed section follows the pointer, the Export panel's card stack scrolls
//! near its edges (`DragState.area`) and Escape cancels. The order changes on the drop (`drop_selected`), at once: the
//! rows snap into it without animation.

use std::rc::Rc;
use std::time::Duration;

use rustcut_core::edit::SectionId;
use slint::{ComponentHandle as _, ModelRc};

use super::Editor;
use crate::ui::drag::{self, DragFrame, ListDrag};
use crate::{MainWindow, Sections};

/// The card stack the Sections card lies in (`dock_panels.slint`).
const AREA: &str = "export";

/// A drag of the section rows.
pub(super) struct SectionsDrag {
    model: ListDrag,
    /// The rows when it started: another set or order ends it.
    ids: Vec<SectionId>,
    last: Option<DragFrame>,
}

impl Editor {
    /// A drag starts at row `row`, the pointer at `pointer` (the list's coordinates).
    fn sections_drag_start(self: &Rc<Self>, row: usize, pointer: f32, pitch: f32, height: f32) {
        let Some(window) = self.window.upgrade() else { return };
        if self.probe_blocks() {
            return;
        }
        let (model, ids, title, count) = {
            let mut st = self.state.borrow_mut();
            let s = &mut st.sections;
            let Some(grabbed) = s.id_at(row) else { return };
            if !s.is_selected(grabbed) {
                s.pick(row, false, false);
            }
            let ids: Vec<SectionId> = s.list().ids().collect();
            let dragged: Vec<bool> = ids.iter().map(|id| s.is_selected(*id)).collect();
            let count = dragged.iter().filter(|d| **d).count();
            let Some(model) = ListDrag::new(vec![height; ids.len()], pitch - height, dragged, row, pointer) else {
                return;
            };
            (model, ids, format!("Section {}", row + 1), count)
        };
        tracing::debug!(sections = count, row, "[Editor] Section drag started");
        let sections = window.global::<Sections<'_>>();
        sections.set_drag_title(title.into());
        sections.set_drag_detail("".into());
        sections.set_drag_count(i32::try_from(count).unwrap_or(i32::MAX));
        *self.sections_drag.borrow_mut() = Some(SectionsDrag { model, ids, last: None });
        let weak = Rc::downgrade(self);
        drag::begin(&window, AREA, move || {
            if let Some(editor) = weak.upgrade() {
                editor.sections_drag_finish(false);
            }
        });
        self.publish();
        self.sections_drag_move(pointer);
        window.global::<Sections<'_>>().set_dragging(true);
    }

    /// The pointer moved (or the card stack scrolled under it).
    fn sections_drag_move(&self, pointer: f32) {
        let frame = {
            let mut slot = self.sections_drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let frame = drag.model.frame(pointer);
            tracing::trace!(pointer, drop = frame.drop, "[Editor] Section drag moved");
            drag.last = Some(frame.clone());
            frame
        };
        let Some(window) = self.window.upgrade() else { return };
        let sections = window.global::<Sections<'_>>();
        self.drag_shifts.set_vec(frame.shifts.clone());
        sections.set_drag_gap_y(frame.gap_top);
        sections.set_drag_gap_height(frame.gap_height);
        sections.set_drag_ghost_y(frame.ghost_top);
    }

    /// The button was released (`drop`) or Escape pressed: a drop that changes the order moves the sections.
    fn sections_drag_finish(&self, drop: bool) {
        let Some(window) = self.window.upgrade() else { return };
        let Some(drag) = self.sections_drag.borrow_mut().take() else {
            return;
        };
        drag::end(&window);
        let target = drag.last.filter(|f| drop && f.moves).map(|f| f.drop);
        let moved = target.is_some_and(|before| self.state.borrow_mut().sections.drop_selected(before));
        tracing::debug!(dropped = drop, moved, "[Editor] Section drag ended");
        let sections = window.global::<Sections<'_>>();
        sections.set_drag_instant(moved);
        sections.set_dragging(false);
        self.drag_shifts.set_vec(Vec::new());
        if moved {
            self.edit_accepted();
            self.publish();
            let weak = window.as_weak();
            slint::Timer::single_shot(Duration::from_millis(50), move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<Sections<'_>>().set_drag_instant(false);
                }
            });
        }
    }

    /// The rows were published: a drag ends when the rows changed under it (a key deleted one).
    pub(super) fn sections_drag_rows_changed(&self, ids: &[SectionId]) {
        let stale = self.sections_drag.borrow().as_ref().is_some_and(|d| d.ids != ids);
        if !stale {
            return;
        }
        let Some(window) = self.window.upgrade() else { return };
        self.sections_drag.borrow_mut().take();
        tracing::debug!("[Editor] Section drag canceled: the rows changed");
        drag::end(&window);
        window.global::<Sections<'_>>().set_dragging(false);
        self.drag_shifts.set_vec(Vec::new());
    }
}

pub(super) fn connect(editor: &Rc<Editor>, window: &MainWindow) {
    let sections = window.global::<Sections<'_>>();
    sections.set_drag_shifts(ModelRc::from(Rc::clone(&editor.drag_shifts)));
    let weak = Rc::downgrade(editor);
    sections.on_drag_start(move |row, pointer, pitch, height| {
        if let (Some(e), Ok(row)) = (weak.upgrade(), usize::try_from(row)) {
            e.sections_drag_start(row, pointer, pitch, height);
        }
    });
    let weak = Rc::downgrade(editor);
    sections.on_drag_move(move |pointer| {
        if let Some(e) = weak.upgrade() {
            e.sections_drag_move(pointer);
        }
    });
    let weak = Rc::downgrade(editor);
    sections.on_drag_end(move || {
        if let Some(e) = weak.upgrade() {
            e.sections_drag_finish(true);
        }
    });
}
