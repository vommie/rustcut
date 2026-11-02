//! Drag & drop of a plugin table's rows and of the plugin list, with the program's scheme (`ui::drag`, P9.4): the
//! other rows move aside to open the gap, a ghost follows the pointer, the Settings page scrolls near its edges
//! (`DragState.area` "settings") and Escape cancels. The plugin only declares `reorder_drag`; RustCut does the rest and
//! hands the plugin the rows in their new order.

use std::rc::Weak;
use std::time::Duration;

use slint::ComponentHandle as _;

use super::{PluginKey, SettingsBridge};
use crate::PluginsUi;
use crate::ui::drag::{self, DragFrame, ListDrag};

/// The key of the plugin list's drag (the tables' keys are `plugin:<id>:<field>`).
pub(super) const LIST: &str = "#plugins";

/// A drag that runs.
pub(super) struct RowDrag {
    key: String,
    from: usize,
    model: ListDrag,
    last: Option<DragFrame>,
}

impl SettingsBridge {
    /// A drag starts at row `row` of the list `key`, the pointer at `pointer` (the list's coordinates).
    pub(super) fn plugin_drag_start(&self, key: &str, row: usize, pointer: f32, pitch: f32, height: f32) {
        let (count, title, detail) = if key == LIST {
            // Unfolded details would make the rows differ in height: the list folds for the drag.
            self.plugin_pages.expanded.borrow_mut().clear();
            self.publish();
            let list = self.plugins().map(|p| p.list()).unwrap_or_default();
            let name = list.get(row).map(|p| p.name().to_owned()).unwrap_or_default();
            (list.len(), name, "Drop it where it should run".to_owned())
        } else {
            let Some(parsed) = PluginKey::parse(key) else { return };
            let rows = self.plugin_table_rows(&parsed).len();
            (rows, format!("Rule {}", row + 1), String::new())
        };
        if row >= count {
            return;
        }
        let mut dragged = vec![false; count];
        if let Some(slot) = dragged.get_mut(row) {
            *slot = true;
        }
        let Some(model) = ListDrag::new(vec![height; count], (pitch - height).max(0.0), dragged, row, pointer) else {
            return;
        };
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<PluginsUi<'_>>();
        ui.set_drag_key(key.into());
        ui.set_drag_row(i32::try_from(row).unwrap_or(-1));
        ui.set_drag_title(title.into());
        ui.set_drag_detail(detail.into());
        ui.set_drag_shifts(slint::ModelRc::from(std::rc::Rc::clone(&self.plugin_pages.shifts)));
        *self.plugin_pages.drag.borrow_mut() = Some(RowDrag {
            key: key.to_owned(),
            from: row,
            model,
            last: None,
        });
        let weak = Weak::clone(&self.me);
        drag::begin(window, "settings", move || {
            if let Some(b) = weak.upgrade() {
                b.plugin_drag_end(false);
            }
        });
        ui.set_dragging(true);
        tracing::debug!(key, row, "[Plugins] Row drag started");
        drop(ui);
        self.plugin_drag_move(pointer);
    }

    /// The pointer moved (or the page scrolled under it).
    pub(super) fn plugin_drag_move(&self, pointer: f32) {
        let frame = {
            let mut slot = self.plugin_pages.drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let frame = drag.model.frame(pointer);
            drag.last = Some(frame.clone());
            frame
        };
        self.plugin_pages.shifts.set_vec(frame.shifts.clone());
        if let Some(window) = self.window.borrow().as_ref() {
            let ui = window.global::<PluginsUi<'_>>();
            ui.set_drag_gap_y(frame.gap_top);
            ui.set_drag_gap_height(frame.gap_height);
            ui.set_drag_ghost_y(frame.ghost_top);
        }
    }

    /// Released (`drop`) or canceled: a drop that changes the order moves the row.
    pub(super) fn plugin_drag_end(&self, drop: bool) {
        let Some(drag) = self.plugin_pages.drag.borrow_mut().take() else {
            return;
        };
        if let Some(window) = self.window.borrow().as_ref() {
            drag::end(window);
            let ui = window.global::<PluginsUi<'_>>();
            ui.set_dragging(false);
            ui.set_drag_instant(true);
            let weak = window.as_weak();
            slint::Timer::single_shot(Duration::from_millis(50), move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<PluginsUi<'_>>().set_drag_instant(false);
                }
            });
        }
        self.plugin_pages.shifts.set_vec(Vec::new());
        // What changed during the drag (a check's answer) shows now.
        self.publish();
        let Some(before) = drag.last.filter(|f| drop && f.moves).map(|f| f.drop) else {
            tracing::debug!(dropped = drop, "[Plugins] Row drag ended without a move");
            return;
        };
        // `before` counts in the old order; the row's new place once it is taken out.
        let to = if before > drag.from { before - 1 } else { before };
        tracing::debug!(key = drag.key, from = drag.from, to, "[Plugins] Row dropped");
        if drag.key == LIST {
            self.plugin_list_move_to(drag.from, to);
        } else {
            self.plugin_rows_move_to(&drag.key, drag.from, to);
        }
    }

    /// The plugin list's drag: the order lists every plugin then.
    fn plugin_list_move_to(&self, from: usize, to: usize) {
        let list = self.plugins().map(|p| p.list()).unwrap_or_default();
        let mut ids: Vec<String> = list.iter().map(|p| p.id.clone()).collect();
        if from >= ids.len() || to >= ids.len() {
            return;
        }
        let id = ids.remove(from);
        ids.insert(to, id);
        ids.dedup();
        self.update(|s| s.plugins.order = ids);
    }
}
