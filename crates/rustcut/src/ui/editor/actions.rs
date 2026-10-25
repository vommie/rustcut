//! What each keymap action does, and the `Shortcuts` callbacks: key presses no element took, menu items without a
//! shortcut of their own, text fields taking and giving back the keyboard.

use std::rc::Rc;

use rustcut_core::queue::MoveTarget;
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;
use slint::platform::Key;

use super::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::keymap::{Action, KeyPress};
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::player::ShuttleKey;
use crate::ui::queue::QueueBridge;
use crate::ui::sections::Bound;
use crate::{MainWindow, Playback, Shortcuts};

/// Whether a key event's text is `key` (Slint sends special keys as private-use characters).
fn is_key(text: &str, key: Key) -> bool {
    text == slint::SharedString::from(key).as_str()
}

/// Cut points closer to the playhead than this count as "here" (the next one is the one after).
const CUT_EPSILON: Timestamp = Timestamp::from_millis(1);
/// Zoom factor of one zoom key press.
pub(super) const ZOOM_STEP: f64 = 1.5;

pub(super) fn connect(editor: &Rc<Editor>, window: &MainWindow) {
    let sc = window.global::<Shortcuts<'_>>();
    let this = Rc::downgrade(editor);
    sc.on_key_pressed(move |text, control, alt, shift, meta, repeat| {
        let press = KeyPress {
            text: text.as_str(),
            control,
            alt,
            shift,
            meta,
            repeat,
        };
        this.upgrade().is_some_and(|editor| editor.key_pressed(&press))
    });
    let this = Rc::downgrade(editor);
    sc.on_run(move |id| match (this.upgrade(), Action::from_id(id.as_str())) {
        (Some(editor), Some(action)) => editor.run(action),
        (_, None) => tracing::warn!(id = %id, "[Keymap] Unknown action from a menu"),
        (None, _) => {}
    });
    let this = Rc::downgrade(editor);
    sc.on_key_released(move |text| {
        if let Some(editor) = this.upgrade()
            && is_key(&text, Key::Control)
            && let Some(filters) = editor.filters.upgrade()
        {
            filters.control_changed(false);
        }
    });
    let this = Rc::downgrade(editor);
    sc.on_modal_changed(move |modal| {
        if let Some(editor) = this.upgrade() {
            editor.set_menu_modal(modal);
        }
    });
    let window_weak = window.as_weak();
    sc.on_release_focus(move || {
        if let Some(window) = window_weak.upgrade() {
            window.invoke_focus_keys();
        }
    });
}

impl Editor {
    /// A key press no focused element took: `true` when it belongs to the keymap (a held key that must not repeat its
    /// action is taken and ignored).
    fn key_pressed(self: &Rc<Self>, press: &KeyPress<'_>) -> bool {
        if let Some(filters) = self.filters.upgrade() {
            // Ctrl held over the video edits the crop; any other key (a shortcut like Ctrl+O) cancels that start.
            if is_key(press.text, Key::Control) {
                if !press.repeat {
                    filters.control_changed(true);
                }
                return false;
            }
            filters.other_key();
            if filters.crop_key(press.text) {
                return true;
            }
        }
        if is_key(press.text, Key::Escape) && self.escape.borrow().as_ref().is_some_and(|hook| hook()) {
            return true;
        }
        if self.probe_playing.get() && is_key(press.text, Key::Escape) {
            if let Some(probe) = self.probe.borrow().upgrade() {
                probe.back();
            }
            return true;
        }
        if is_key(press.text, Key::Escape)
            && let Some(tags) = self.tags.borrow().upgrade()
            && tags.editing()
        {
            tags.cancel_edit();
            return true;
        }
        let found = self.keymap.borrow().lookup(press);
        let Some(action) = found else {
            return false;
        };
        // Behind a modal dialog only Quit acts (a key the dialog did not take must not reach the editor, P8).
        if self.menu_modal.get() && action != Action::Quit {
            tracing::debug!(action = action.id(), "[Keymap] Key ignored behind a modal dialog");
            return true;
        }
        if press.repeat && !action.repeats() {
            return true;
        }
        tracing::debug!(action = action.id(), "[Keymap] Key");
        self.run(action);
        true
    }

    /// Runs one keymap action.
    pub(super) fn run(self: &Rc<Self>, action: Action) {
        let Some(window) = self.window.upgrade() else { return };
        if action.edits_the_source() && self.probe_blocks() {
            return;
        }
        let playback = window.global::<Playback<'_>>();
        match action {
            Action::Open => window.invoke_open_file(),
            Action::Save => {
                if let Some(export) = self.export.borrow().upgrade() {
                    export.save();
                }
            }
            Action::Quit => window.invoke_request_quit(),
            Action::OpenSettings => window.invoke_open_settings(),
            // Keys flash the play or pause symbol when they change between paused and moving (P9.9: only keys).
            Action::TogglePause => self.with_player(|p| {
                let moving = p.is_moving();
                p.toggle_pause();
                p.key_flash(!moving);
            }),
            Action::ShuttleReverse | Action::ShuttleStop | Action::ShuttleForward => self.with_player(|p| {
                let key = match action {
                    Action::ShuttleReverse => ShuttleKey::Reverse,
                    Action::ShuttleStop => ShuttleKey::Stop,
                    _ => ShuttleKey::Forward,
                };
                let moving = p.is_moving();
                if let Some(now) = p.shuttle(key)
                    && now != moving
                {
                    p.key_flash(now);
                }
            }),
            Action::FrameBack => self.with_player(|p| p.step_frame(true)),
            Action::FrameForward => self.with_player(|p| p.step_frame(false)),
            Action::SeekBack => self.with_player(|p| p.seek_by(-self.seek_step.get())),
            Action::SeekForward => self.with_player(|p| p.seek_by(self.seek_step.get())),
            Action::SeekBackLong => self.with_player(|p| p.seek_by(-self.seek_step_long.get())),
            Action::SeekForwardLong => self.with_player(|p| p.seek_by(self.seek_step_long.get())),
            Action::PreviousCut => self.jump_to_cut(false),
            Action::NextCut => self.jump_to_cut(true),
            Action::ToggleMute => playback.invoke_toggle_mute(),
            Action::Reload => playback.invoke_reload(),
            Action::SetStart => self.set_mark_here(Bound::Start),
            Action::SetEnd => self.set_mark_here(Bound::End),
            Action::GoToStart => self.go_to_mark(Bound::Start),
            Action::GoToEnd => self.go_to_mark(Bound::End),
            Action::AddSection => self.add_section(),
            Action::DeleteSection => self.delete_section(),
            Action::ZoomIn => self.zoom_step(ZOOM_STEP),
            Action::ZoomOut => self.zoom_step(1.0 / ZOOM_STEP),
            Action::ZoomFit => {
                self.state.borrow_mut().viewport.fit();
                self.publish();
            }
            Action::CropEdit => self.with_filters(FiltersBridge::toggle_crop_edit),
            Action::AutoCrop => self.with_filters(FiltersBridge::auto_crop),
            Action::ToggleFilterPreview => self.with_filters(FiltersBridge::toggle_preview),
            Action::ToggleKeepFilters => self.with_filters(FiltersBridge::toggle_keep),
            Action::RotateLeft => self.with_filters(|f| f.turn(false)),
            Action::RotateRight => self.with_filters(|f| f.turn(true)),
            Action::MirrorHorizontal => self.with_filters(|f| f.flip(true)),
            Action::MirrorVertical => self.with_filters(|f| f.flip(false)),
            Action::ResetFilters => self.with_filters(FiltersBridge::reset),
            Action::ProbeRender => {
                let probe = self.probe.borrow().upgrade();
                if let Some(probe) = probe {
                    probe.render();
                }
            }
            Action::QueueMoveUp => self.with_queue(|q| q.move_selected(MoveTarget::Up)),
            Action::QueueMoveDown => self.with_queue(|q| q.move_selected(MoveTarget::Down)),
            Action::QueueMoveTop => self.with_queue(|q| q.move_selected(MoveTarget::Top)),
            Action::QueueMoveBottom => self.with_queue(|q| q.move_selected(MoveTarget::Bottom)),
            Action::QueueSelectAll => self.with_queue(QueueBridge::select_all),
        }
    }

    /// Runs `f` with the queue panel when it exists.
    fn with_queue(&self, f: impl FnOnce(&QueueBridge)) {
        let queue = self.queue.borrow().upgrade();
        if let Some(queue) = queue {
            f(&queue);
        }
    }

    /// Runs `f` with the filters when they exist.
    fn with_filters(&self, f: impl FnOnce(&FiltersBridge)) {
        if let Some(filters) = self.filters.upgrade() {
            f(&filters);
        }
    }

    /// Sets the start or end mark at the playhead.
    pub(super) fn set_mark_here(&self, bound: Bound) {
        if self.probe_blocks() {
            return;
        }
        let Some(player) = self.player.upgrade().filter(|p| p.is_loaded()) else {
            return;
        };
        let t = player.position();
        self.state.borrow_mut().sections.set_mark(bound, t);
        self.publish();
    }

    pub(super) fn go_to_mark(&self, bound: Bound) {
        let mark = self.state.borrow().sections.mark(bound);
        if let Some(t) = mark {
            self.with_player(|p| p.seek_to(t, true));
        }
    }

    pub(super) fn add_section(&self) {
        if self.probe_blocks() {
            return;
        }
        let result = {
            let mut st = self.state.borrow_mut();
            st.sections
                .add_current()
                .map(|id| st.sections.list().index_of(id).map_or(0, |i| i + 1))
        };
        match result {
            Ok(number) => {
                self.edit_accepted();
                notices::post(Notice::new(Kind::SectionAdded, format!("Section {number} added")));
                self.publish();
            }
            Err(e) => self.refused(e.to_string()),
        }
    }

    pub(super) fn delete_section(&self) {
        if self.probe_blocks() {
            return;
        }
        if self.state.borrow_mut().sections.delete_selected() {
            self.publish();
        }
    }

    /// Jumps to the next (or previous) section bound, mark, beginning or end.
    fn jump_to_cut(&self, forward: bool) {
        let Some(player) = self.player.upgrade().filter(|p| p.is_loaded()) else {
            return;
        };
        let here = player.position();
        let points = self.state.borrow().sections.cut_points();
        let target = if forward {
            points.into_iter().find(|p| *p > here + CUT_EPSILON)
        } else {
            points.into_iter().rev().find(|p| *p + CUT_EPSILON < here)
        };
        if let Some(t) = target {
            player.seek_to(t, true);
        }
    }

    /// Zooms around the playhead when it is visible, else around the middle.
    fn zoom_step(&self, factor: f64) {
        {
            let mut st = self.state.borrow_mut();
            let x = st.viewport.x_of(st.playhead);
            let anchor = if (0.0..=st.viewport.width()).contains(&x) {
                x
            } else {
                st.viewport.width() / 2.0
            };
            st.viewport.zoom_by(factor, anchor);
        }
        self.publish();
    }
}
