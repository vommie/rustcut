//! Dragging a target folder of Settings > Export & Naming to another place (P9.7, the user's wish with the UI review's
//! notes 03), with the drag & drop module of P9.4: the other rows move aside, a ghost with the folder's label follows
//! the pointer, the page scrolls near its edges (`DragState.area`) and Escape cancels. The order changes on the drop.

use std::rc::Weak;
use std::time::Duration;

use slint::{ComponentHandle, Model as _};

use super::{SettingsBridge, folders};
use crate::ui::drag::{self, DragFrame, ListDrag};
use crate::{SettingsUi, SettingsWindow};

/// The scroll area of the Settings window's page (`settings_window.slint`).
const AREA: &str = "settings";

/// A drag of one folder row.
pub(super) struct FolderDrag {
    model: ListDrag,
    row: usize,
    last: Option<DragFrame>,
}

impl SettingsBridge {
    /// A drag starts at row `row`, the pointer at `pointer` (the list's coordinates).
    pub(super) fn folder_drag_start(&self, row: usize, pointer: f32, pitch: f32, height: f32) {
        let Some(window) = self.window.borrow().as_ref().map(ComponentHandle::clone_strong) else {
            return;
        };
        let count = self.models.folders.row_count();
        let dragged: Vec<bool> = (0..count).map(|i| i == row).collect();
        let Some(model) = ListDrag::new(vec![height; count], pitch - height, dragged, row, pointer) else {
            return;
        };
        tracing::debug!(row, "[Settings] Folder drag started");
        let ui = window.global::<SettingsUi<'_>>();
        ui.set_folder_drag_title(self.models.folders.row_data(row).map(|f| f.label).unwrap_or_default());
        ui.set_folder_drag_row(i32::try_from(row).unwrap_or(-1));
        *self.folder_drag.borrow_mut() = Some(FolderDrag { model, row, last: None });
        let me = Weak::clone(&self.me);
        drag::begin(&window, AREA, move || {
            if let Some(bridge) = me.upgrade() {
                bridge.folder_drag_finish(false);
            }
        });
        self.folder_drag_move(pointer);
        ui.set_folder_dragging(true);
    }

    /// The pointer moved (or the page scrolled under it).
    pub(super) fn folder_drag_move(&self, pointer: f32) {
        let frame = {
            let mut slot = self.folder_drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let frame = drag.model.frame(pointer);
            drag.last = Some(frame.clone());
            frame
        };
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<SettingsUi<'_>>();
        self.folder_shifts.set_vec(frame.shifts.clone());
        ui.set_folder_drag_gap_y(frame.gap_top);
        ui.set_folder_drag_gap_height(frame.gap_height);
        ui.set_folder_drag_ghost_y(frame.ghost_top);
    }

    /// The button was released (`drop`) or Escape pressed: a drop that changes the order moves the folder.
    pub(super) fn folder_drag_finish(&self, drop: bool) {
        let Some(drag) = self.folder_drag.borrow_mut().take() else {
            return;
        };
        let target = drag.last.filter(|f| drop && f.moves).map(|f| f.drop);
        let mut moved = false;
        if let Some(before) = target {
            self.update(|s| moved = folders::drop_before(&mut s.export.folders.entries, drag.row, before));
        }
        tracing::debug!(dropped = drop, moved, "[Settings] Folder drag ended");
        let window: Option<SettingsWindow> = self.window.borrow().as_ref().map(ComponentHandle::clone_strong);
        let Some(window) = window else { return };
        drag::end(&window);
        let ui = window.global::<SettingsUi<'_>>();
        ui.set_folder_drag_instant(moved);
        ui.set_folder_dragging(false);
        self.folder_shifts.set_vec(Vec::new());
        if moved {
            tracing::info!(row = drag.row, "[Settings] Target folder moved");
            self.publish();
            let weak = window.as_weak();
            slint::Timer::single_shot(Duration::from_millis(50), move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<SettingsUi<'_>>().set_folder_drag_instant(false);
                }
            });
        }
    }
}
