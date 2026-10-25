//! The tag filter window (P8b, user decision Q-P8b, `ui/tag_filter_window.slint`): RustView's tag tree with a check
//! box per tag (a check = hidden, Q-P9) in a window of its own, a dialog of the main window on X11 (like the color
//! picker), centered over it. The window edits a draft of the hidden branches: Apply makes it the panel's and closes,
//! Escape and the window's × drop it (D-P9n). The window's size and folded branches are kept in `ui-state.toml` with the
//! panel's state. UI thread only.

use std::rc::Rc;

use slint::{ComponentHandle as _, ModelRc};

use super::TagsBridge;
use super::hidden::{Check, FilterRow};
use crate::desktop::x11_dialog;
use crate::{HoldConfirm, TagFilterRowView, TagFilterWindow, TooltipSettings};

impl TagsBridge {
    /// The panel's filter button: opens the window (or brings it to the front).
    pub(super) fn open_filter(self: &Rc<Self>) {
        let Some(main) = self.window.upgrade() else { return };
        if self.filter_window.borrow().is_none() {
            match TagFilterWindow::new() {
                Ok(window) => {
                    crate::ui::tooltip::install(&window);
                    x11_dialog::parent_when_set_up(&window, &main);
                    window.set_rows(ModelRc::from(Rc::clone(&self.filter_rows)));
                    connect(self, &window);
                    *self.filter_window.borrow_mut() = Some(window);
                }
                Err(e) => {
                    tracing::error!(error = %e, "[Tags] The tag filter window could not be made");
                    return;
                }
            }
        }
        let filter = self.filter_window.borrow();
        let Some(window) = filter.as_ref() else { return };
        // The main window's hold time and tooltips (each window has its own globals).
        window
            .global::<HoldConfirm<'_>>()
            .set_duration(main.global::<HoldConfirm<'_>>().get_duration());
        let tips = window.global::<TooltipSettings<'_>>();
        let main_tips = main.global::<TooltipSettings<'_>>();
        tips.set_enabled(main_tips.get_enabled());
        tips.set_delay(main_tips.get_delay());
        let (width, height) = self.state.borrow().filter_size;
        window.window().set_size(slint::LogicalSize::new(
            az::cast::<u32, f32>(width),
            az::cast::<u32, f32>(height),
        ));
        let shown = window.window().is_visible();
        if !shown {
            self.state.borrow_mut().begin_filter();
        }
        if let Err(e) = window.show() {
            tracing::error!(error = %e, "[Tags] The tag filter window could not be shown");
            return;
        }
        if !shown {
            x11_dialog::center_over(window.window(), main.window());
            tracing::info!("[Tags] Tag filter opened");
        }
        drop(filter);
        self.publish_filter();
    }

    /// The window's rows, count and texts (when it is open).
    pub(super) fn publish_filter(&self) {
        let filter = self.filter_window.borrow();
        let Some(window) = filter.as_ref().filter(|w| w.window().is_visible()) else {
            return;
        };
        let search = window.get_search().to_string();
        let state = self.state.borrow();
        let rows: Vec<TagFilterRowView> = state.filter_rows(&search).into_iter().map(row_view).collect();
        window.set_hidden_count(i32::try_from(state.filter_hidden_count()).unwrap_or(i32::MAX));
        window.set_empty_text(
            if state.has_tree() {
                "RustView's library has no tags yet."
            } else {
                "RustView's tags appear here once it answers."
            }
            .into(),
        );
        drop(state);
        crate::util::model::sync(&self.filter_rows, rows);
    }

    /// RustCut quits: the window is hidden and dropped (its size is in the panel's state already; a draft is dropped).
    pub fn close_filter(&self) {
        if let Some(window) = self.filter_window.borrow_mut().take() {
            let _ = window.hide();
        }
        self.state.borrow_mut().discard_filter();
    }

    /// A change of the window's draft (the panel shows it only after Apply).
    fn filter_changed(&self, change: impl FnOnce(&mut super::TagPanel)) {
        change(&mut self.state.borrow_mut());
        self.publish_filter();
    }

    /// Apply (`true`) or Escape and × (`false`): the draft becomes the panel's or is dropped, and the window closes.
    fn end_filter(&self, apply: bool) {
        let changed = {
            let mut state = self.state.borrow_mut();
            if apply {
                state.apply_filter()
            } else {
                state.discard_filter();
                false
            }
        };
        if changed {
            tracing::info!(hidden = self.state.borrow().hidden_count(), "[Tags] Tag filter applied");
        }
        if let Some(window) = self.filter_window.borrow().as_ref() {
            let _ = window.hide();
        }
        self.publish();
    }
}

fn row_view(row: FilterRow) -> TagFilterRowView {
    TagFilterRowView {
        id: i32::try_from(row.id).unwrap_or(i32::MAX),
        name: row.name.into(),
        depth: i32::try_from(row.depth).unwrap_or(i32::MAX),
        has_children: row.has_children,
        expanded: row.expanded,
        check: match row.check {
            Check::Shown => 0,
            Check::Hidden => 1,
            Check::Mixed => 2,
        },
        dimmed: row.dimmed,
    }
}

fn connect(bridge: &Rc<TagsBridge>, window: &TagFilterWindow) {
    let weak = Rc::downgrade(bridge);
    window.on_toggled(move |id| {
        if let (Some(b), Ok(id)) = (weak.upgrade(), u32::try_from(id)) {
            b.filter_changed(|s| s.toggle_filter(id));
        }
    });
    let weak = Rc::downgrade(bridge);
    window.on_expand(move |id| {
        if let (Some(b), Ok(id)) = (weak.upgrade(), u32::try_from(id)) {
            b.filter_changed(|s| s.toggle_filter_expanded(id));
        }
    });
    let weak = Rc::downgrade(bridge);
    window.on_search_changed(move |_| {
        if let Some(b) = weak.upgrade() {
            b.publish_filter();
        }
    });
    let weak = Rc::downgrade(bridge);
    window.on_show_all(move || {
        if let Some(b) = weak.upgrade() {
            tracing::info!("[Tags] Tag filter cleared");
            b.filter_changed(super::TagPanel::clear_filter);
        }
    });
    let weak = Rc::downgrade(bridge);
    window.on_apply(move || {
        if let Some(b) = weak.upgrade() {
            b.end_filter(true);
        }
    });
    let weak = Rc::downgrade(bridge);
    window.on_discard(move || {
        if let Some(b) = weak.upgrade() {
            b.end_filter(false);
        }
    });
    // The window manager's × drops the draft like Escape.
    let weak = Rc::downgrade(bridge);
    window.window().on_close_requested(move || {
        if let Some(b) = weak.upgrade() {
            b.end_filter(false);
        }
        slint::CloseRequestResponse::HideWindow
    });
    let weak = Rc::downgrade(bridge);
    window.on_resized(move |width, height| {
        if let Some(b) = weak.upgrade() {
            let size = |v: f32| az::checked_cast::<f32, u32>(v.round()).unwrap_or(0);
            b.state.borrow_mut().filter_size = (size(width), size(height));
        }
    });
}
