//! The Sections card's bubbles (M6.5c, user decision Q-N10): a typed time that is refused shows why at its field until
//! a time is kept or the field closes (while it stays open, every edit is checked again after the typing grace time),
//! and every section that shares a part with an earlier one says so at its row.

use crate::ui::notices::{self, Area, Kind, Level, Notice, TYPING};
use crate::ui::sections::EditError;

use super::Editor;

/// The anchor of a mark's field (`sections_card.slint`): 0 = start, 1 = end.
pub(super) fn mark_anchor(which: i32) -> String {
    format!("sections.mark.{}", side(which))
}

/// The anchor of a time field in the list.
pub(super) fn row_anchor(row: i32, which: i32) -> String {
    format!("sections.row.{row}.{}", side(which))
}

const fn side(which: i32) -> &'static str {
    if which == 0 { "start" } else { "end" }
}

fn words(e: &EditError) -> (String, String) {
    match e {
        EditError::NotATime(text) => (
            format!("\"{}\" is not a time", text.trim()),
            "Type H:MM:SS.mmm, for example 0:01:23.500.".to_owned(),
        ),
        EditError::Empty => (
            "Start and end are the same".to_owned(),
            "A section needs a length.".to_owned(),
        ),
        other => (other.to_string(), String::new()),
    }
}

impl Editor {
    /// A typed time was refused (Enter): its bubble says why at once.
    pub(super) fn time_refused(&self, anchor: &str, e: &EditError) {
        tracing::info!(why = %e, "[Editor] Typed time refused");
        self.time_bubbles.borrow_mut().insert(anchor.to_owned());
        post_time(anchor, e, std::time::Duration::ZERO);
    }

    /// The field with a refused time was typed in: `check` is what the new text would give.
    pub(super) fn time_typed(&self, anchor: &str, check: Result<(), EditError>) {
        if !self.time_bubbles.borrow().contains(anchor) {
            return;
        }
        match check {
            Ok(()) => {
                self.time_bubbles.borrow_mut().remove(anchor);
                notices::clear_soon(anchor, TYPING);
            }
            Err(e) => post_time(anchor, &e, TYPING),
        }
    }

    /// The field closed (a time was kept, or the refused text was dropped).
    pub(super) fn time_done(&self, anchor: &str) {
        if self.time_bubbles.borrow_mut().remove(anchor) {
            notices::clear(anchor);
        }
    }

    /// The overlaps of the list (`SectionsState::overlaps`) at the later sections' rows.
    pub(super) fn publish_overlaps(&self, overlaps: &[(usize, usize)]) {
        let keys: Vec<String> = overlaps
            .iter()
            .map(|(row, _)| format!("sections-overlap:{row}"))
            .collect();
        for ((row, earlier), key) in overlaps.iter().zip(&keys) {
            notices::post(
                Notice::new(Kind::SectionsOverlap, format!("Overlaps section {earlier}"))
                    .level(Level::Warning)
                    .detail("The shared part is rendered twice.")
                    .key(key.as_str())
                    .at(Area::Export, [format!("sections.row.{row}")]),
            );
        }
        let before = self.overlap_bubbles.replace(keys.clone());
        for gone in before.iter().filter(|k| !keys.contains(k)) {
            notices::clear(gone);
        }
    }
}

fn post_time(anchor: &str, e: &EditError, grace: std::time::Duration) {
    let (title, detail) = words(e);
    notices::post(
        Notice::new(Kind::TimeInvalid, title)
            .detail(detail)
            .at(Area::Export, [anchor])
            .grace(grace),
    );
}
