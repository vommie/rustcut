//! The switches of the row under the tag history (user decision Q-P8b): "Active" (off = the next jobs are saved without
//! RustView; kept in `ui-state.toml` with the panel's state) and "Warn" (`warnings.missing_metadata`, changed through
//! the settings writer). When either is still off as RustView answers for the first time in a run, a message says so
//! once, with a button that switches both on.

use super::{BRIDGE, TagsBridge};
use crate::ui::notices::{self, Kind, Notice};

impl TagsBridge {
    /// The "Active" switch: off = the next job is saved without RustView (user decision Q-P8b).
    pub fn active(&self) -> bool {
        self.state.borrow().active
    }

    /// `warnings.missing_metadata` changed (the Settings window, the save question's "Don't ask again", this panel).
    pub fn set_warn(&self, on: bool) {
        if self.warn.replace(on) != on {
            if on && self.active() {
                notices::clear_kind(Kind::TaggingOff);
            }
            self.publish();
        }
    }

    /// A click switches "Active" on, a hold (Slint) switches it off.
    pub(super) fn toggle_active(&self) {
        let on = {
            let mut state = self.state.borrow_mut();
            state.active = !state.active;
            if !state.active {
                state.hide_mode = false;
            }
            state.active
        };
        tracing::info!(active = on, "[Tags] Active switched");
        if on && self.warn.get() {
            notices::clear_kind(Kind::TaggingOff);
        }
        self.publish();
    }

    /// A click switches "Warn" on, a hold (Slint) switches it off; the setting changes, the listener calls `set_warn`.
    pub(super) fn toggle_warn(&self) {
        let on = !self.warn.get();
        tracing::info!(warn = on, "[Tags] Warn switched");
        if !self.writer.update(|s| s.warnings.missing_metadata = on) {
            Self::tell(
                Notice::new(Kind::SettingsReadOnly, "Not saved")
                    .detail("config.toml is from a newer RustCut: the choice is kept for this run only."),
            );
        }
    }

    /// The start message's button: both switches on.
    fn switch_on(&self) {
        if !self.active() {
            self.toggle_active();
        }
        if !self.warn.get() {
            self.toggle_warn();
        }
    }

    /// RustView answered: the first time in a run, a switch left off says so once (user decision Q-P8b).
    pub(super) fn connected(&self) {
        if self.start_checked.replace(true) {
            return;
        }
        let (active, warn) = (self.active(), self.warn.get());
        if active && warn {
            return;
        }
        let (title, detail) = match (active, warn) {
            (false, false) => (
                "Tagging is off",
                "Jobs are saved without RustView, and missing tags or ratings are not asked about. Both were switched \
                 off in an earlier run.",
            ),
            (false, true) => (
                "Tagging is off",
                "Jobs are saved without RustView (no entry, tags or rating). Switched off in an earlier run.",
            ),
            _ => (
                "Warnings about missing tags are off",
                "Jobs without tags or rating are saved without asking. Switched off in an earlier run or in the settings.",
            ),
        };
        tracing::info!(active, warn, "[Tags] A switch is off at the start");
        let weak = BRIDGE.with(|b| b.borrow().clone());
        notices::post(
            Notice::new(Kind::TaggingOff, title)
                .detail(detail)
                .action("Switch on", move || {
                    if let Some(bridge) = weak.upgrade() {
                        bridge.switch_on();
                    }
                }),
        );
    }
}
