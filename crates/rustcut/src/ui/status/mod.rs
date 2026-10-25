//! The status line's message (left part): the quiet confirmations the notice system routes there (`ui::notices`). The
//! decoder text on the right comes from the player bridge.

use slint::ComponentHandle as _;

pub use crate::ui::notices::Level;
use crate::{MainWindow, Status};

/// Replaces the status message (UI thread). Only the notice center calls it; bridges post notices.
pub(crate) fn show(window: &MainWindow, level: Level, message: &str) {
    let status = window.global::<Status<'_>>();
    status.set_level(level.index());
    status.set_message(message.into());
}

/// Empties the status message: what it said is over (UI thread; the notice center's `clear`).
pub(crate) fn clear(window: &MainWindow) {
    let status = window.global::<Status<'_>>();
    status.set_level(Level::Info.index());
    status.set_message(slint::SharedString::new());
}
