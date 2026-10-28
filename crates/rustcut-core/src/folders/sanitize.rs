//! Brings a loaded tree into shape (a hand-edited `config.toml`): unique ids (0 belongs to the source), unique paths,
//! names, groups only without a path (a folder's entries move out behind it), valid colors and unique places.

use std::collections::HashSet;
use std::path::PathBuf;

use super::edit::is_color;
use super::tree::{FolderId, FolderNode, FolderTree, MAX_PLACES, default_name};

impl FolderTree {
    /// Returns a note per kind of change.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut fix = |note: &str, broken: bool| {
            if broken && !notes.iter().any(|n| n == note) {
                notes.push(note.to_owned());
            }
        };
        fix(
            "export.folders: entries of a folder moved out",
            lift_children(&mut self.entries),
        );
        let mut seen = HashSet::new();
        let mut paths: HashSet<PathBuf> = HashSet::new();
        let mut places: HashSet<u8> = self.source.place.into_iter().filter(|p| *p < MAX_PLACES).collect();
        if self.source.place.is_some_and(|p| p >= MAX_PLACES) {
            self.source.place = None;
            fix("export.folders: places out of range dropped", true);
        }
        if self.source.color.as_deref().is_some_and(|c| !is_color(c)) {
            self.source.color = None;
            fix("export.folders: colors that are not #RRGGBB dropped", true);
        }
        let mut next = self.next_id();
        let mut pass = |node: &mut FolderNode, fix: &mut dyn FnMut(&str, bool)| -> bool {
            if node.id.is_source() || !seen.insert(node.id) {
                node.id = next;
                seen.insert(next);
                next = FolderId(next.0.saturating_add(1));
                fix("export.folders: repeated ids renumbered", true);
            }
            if let Some(path) = &node.path
                && !paths.insert(path.clone())
            {
                fix("export.folders: repeated folders dropped", true);
                return false;
            }
            if node.name.trim().is_empty() {
                node.name = node.path.as_deref().map_or_else(|| "Group".to_owned(), default_name);
                fix("export.folders: empty names filled", true);
            } else if node.name.trim() != node.name {
                node.name = node.name.trim().to_owned();
            }
            if node.short_name.as_deref().is_some_and(|s| s.trim().is_empty()) {
                node.short_name = None;
            }
            if node.color.as_deref().is_some_and(|c| !is_color(c)) {
                node.color = None;
                fix("export.folders: colors that are not #RRGGBB dropped", true);
            }
            if let Some(p) = node.place
                && (p >= MAX_PLACES || !places.insert(p))
            {
                node.place = None;
                fix("export.folders: places out of range or repeated dropped", true);
            }
            true
        };
        retain_deep(&mut self.entries, &mut |n| pass(n, &mut fix));
        notes
    }
}

/// A folder (a path) cannot hold entries: they move to right behind it, at its level. `true` = something moved.
fn lift_children(nodes: &mut Vec<FolderNode>) -> bool {
    let mut moved = false;
    let mut i = 0;
    while let Some(node) = nodes.get_mut(i) {
        if node.is_group() {
            moved |= lift_children(&mut node.children);
            i += 1;
        } else if node.children.is_empty() {
            i += 1;
        } else {
            let lifted = std::mem::take(&mut node.children);
            let at = (i + 1).min(nodes.len());
            nodes.splice(at..at, lifted);
            moved = true;
            // The lifted entries are looked at in the next rounds (they may be folders with entries too).
            i += 1;
        }
    }
    moved
}

/// Keeps the nodes `keep` accepts, depth-first in the tree's order (a dropped group goes with its contents).
fn retain_deep(nodes: &mut Vec<FolderNode>, keep: &mut dyn FnMut(&mut FolderNode) -> bool) {
    let mut kept = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        if keep(&mut node) {
            retain_deep(&mut node.children, keep);
            kept.push(node);
        }
    }
    *nodes = kept;
}
