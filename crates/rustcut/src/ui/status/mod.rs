//! The status line's message (left part). The decoder text on the right comes from the player bridge.

use slint::ComponentHandle as _;

use crate::{MainWindow, Status};

/// Severity of a status message (the icon in front of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

/// Replaces the status message (UI thread). The caller logs it.
pub fn show(window: &MainWindow, level: Level, message: &str) {
    let status = window.global::<Status<'_>>();
    status.set_level(level as i32);
    status.set_message(message.into());
}
