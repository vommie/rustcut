//! The card's check as a bubble (M6.5c, user decision Q-N10): at the name field or the folder choice instead of a line
//! under the file name, after the grace time of typing; a conflict and similar names lead to the target folder's files
//! (the conflict dialog to look, `browse.rs`). A bubble is posted only when it changes, so the card's many refreshes
//! never restart its grace time.

use std::rc::Weak;

use super::ExportBridge;
use super::check::{At, CardCheck, Level as CheckLevel};
use crate::ui::notices::{self, Area, Kind, Level, Notice, TYPING};

/// The anchors' keys (`BubbleAnchor` in `export_card.slint`).
const NAME: &str = "export.name";
const FOLDER: &str = "export.folder";

/// What the check's bubble shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckBubble {
    at: At,
    level: CheckLevel,
    title: String,
    detail: String,
    browse: bool,
}

const fn key(at: At) -> &'static str {
    match at {
        At::Name => NAME,
        At::Folder => FOLDER,
    }
}

impl ExportBridge {
    /// Shows the check `line` as its bubble (or none); a check that is still reading the folder changes nothing.
    pub(super) fn show_check(&self, line: &CardCheck) {
        if line.level == CheckLevel::Hint && line.at.is_none() {
            return;
        }
        let wanted = line.at.map(|at| CheckBubble {
            at,
            level: line.level,
            title: line.text.clone(),
            detail: line.detail.clone(),
            browse: line.browse,
        });
        if *self.shown_check.borrow() == wanted {
            return;
        }
        let before = self.shown_check.replace(wanted.clone());
        if let Some(before) = before
            && wanted.as_ref().is_none_or(|w| w.at != before.at)
        {
            notices::clear_soon(key(before.at), TYPING);
        }
        let Some(bubble) = wanted else { return };
        let (kind, level) = match bubble.level {
            CheckLevel::Error => (Kind::TargetProblem, Level::Error),
            CheckLevel::Conflict => (Kind::TargetConflict, Level::Warning),
            CheckLevel::Hint | CheckLevel::Free => (Kind::TargetSimilar, Level::Info),
        };
        let mut notice = Notice::new(kind, bubble.title)
            .level(level)
            .detail(bubble.detail)
            .at(Area::Export, [key(bubble.at)])
            .grace(TYPING);
        if bubble.browse {
            let me = Weak::clone(&self.me);
            notice = notice.action("Show the files", move || {
                if let Some(bridge) = me.upgrade() {
                    bridge.browse_target();
                }
            });
        }
        notices::post(notice);
    }
}
