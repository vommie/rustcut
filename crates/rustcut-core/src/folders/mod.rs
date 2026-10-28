//! The target folder tree (P11, user decisions Q-FP1/Q-FP2): one tree in Settings feeds the export card's list and the
//! folder wheel. Groups are menu entries without a path, folders have one, "Next to the source" is a fixed entry; a
//! star puts an entry on its own place of the wheel's first level. Everything here is pure (no I/O).

mod edit;
mod levels;
mod list;
pub mod pictures;
mod sanitize;
#[cfg(test)]
mod tests;
mod tree;

pub use edit::{EditError, Spot, is_color};
pub use levels::{Layout, Level, LevelKey, Slot, level};
pub use list::{ListInput, ListRow, RECENT, Section, group_path, rows};
pub use tree::{
    EntryRef, FolderId, FolderNode, FolderTree, MAX_PLACES, Picture, SOURCE_NAME, SourceEntry, Walked, default_name,
};

/// Puts `id` in front of the recent targets (newest first, each once, at most `list::RECENT` kept besides the source).
pub fn remember_recent(recent: &mut Vec<FolderId>, id: FolderId) {
    recent.retain(|r| *r != id);
    recent.insert(0, id);
    recent.truncate(RECENT + 1);
}
