//! The Job Finalization card's folder list without Slint (P11.4, user decision Q-FP2): opened from the card's field, it
//! shows "Next to the source", the favorites, the last three targets and the tree (folded groups are remembered); typing
//! searches every level. The keys move a highlight over the entries (Up/Down), fold and unfold groups (Left/Right),
//! pick (Enter) and close (Escape). While a loaded job's folder is the target, the temporary entry "Job target" stands
//! first (the user's answer, 2026-10-28).

use std::collections::BTreeSet;

use rustcut_core::folders::{FolderId, FolderTree, ListInput, ListRow, rows};

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// The rows or the highlight changed.
    Changed,
    Pick(FolderId),
    Close,
}

/// A key the list handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

/// The id of the temporary "Job target" row (never an id of the tree: ids are given out from 1 upwards).
pub const JOB_TARGET: FolderId = FolderId(u32::MAX);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderList {
    /// The loaded job's folder while it is the target (its row comes first; the text is searched too).
    pub job_target: Option<String>,
    pub open: bool,
    pub query: String,
    /// The highlighted row (an entry, never a heading).
    pub highlight: Option<usize>,
    pub collapsed: BTreeSet<FolderId>,
    /// The rows as shown.
    pub rows: Vec<ListRow>,
}

impl FolderList {
    pub fn new(collapsed: &[FolderId]) -> Self {
        Self {
            collapsed: collapsed.iter().copied().collect(),
            ..Self::default()
        }
    }

    /// Builds the rows again (the tree, the recent targets, the folding or the search changed).
    pub fn refresh(&mut self, tree: &FolderTree, recent: &[FolderId]) {
        let highlighted = self.highlight.and_then(|i| self.rows.get(i)).and_then(ListRow::id);
        self.rows = rows(&ListInput {
            tree,
            recent,
            collapsed: &self.collapsed,
            query: &self.query,
        });
        let query = self.query.trim().to_lowercase();
        if let Some(path) = &self.job_target
            && (query.is_empty() || path.to_lowercase().contains(&query) || "job target".contains(&query))
        {
            self.rows.insert(
                0,
                ListRow::Entry {
                    id: JOB_TARGET,
                    depth: 0,
                    group: false,
                    expanded: false,
                    context: path.clone(),
                },
            );
        }
        self.highlight = highlighted
            .and_then(|id| self.rows.iter().position(|r| r.id() == Some(id)))
            .or_else(|| self.first_entry());
    }

    fn first_entry(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.id().is_some())
    }

    /// Opens with the current target highlighted (its first row) and no search.
    pub fn open(&mut self, tree: &FolderTree, recent: &[FolderId], current: FolderId) {
        self.open = true;
        self.query.clear();
        self.highlight = None;
        self.refresh(tree, recent);
        self.highlight = self
            .rows
            .iter()
            .position(|r| r.id() == Some(current))
            .or_else(|| self.first_entry());
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
    }

    /// The search text changed: the first hit is highlighted.
    pub fn search(&mut self, tree: &FolderTree, recent: &[FolderId], text: &str) {
        text.clone_into(&mut self.query);
        self.highlight = None;
        self.refresh(tree, recent);
        self.highlight = self
            .rows
            .iter()
            .position(ListRow::pickable)
            .or_else(|| self.first_entry());
    }

    /// A group's fold button or Left/Right on it.
    pub fn toggle(&mut self, tree: &FolderTree, recent: &[FolderId], id: FolderId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
        self.refresh(tree, recent);
    }

    /// A click on row `index`: a folder is picked, a group folds or unfolds.
    pub fn click(&mut self, tree: &FolderTree, recent: &[FolderId], index: usize) -> Outcome {
        match self.rows.get(index) {
            Some(row @ ListRow::Entry { id, .. }) if row.pickable() => Outcome::Pick(*id),
            Some(ListRow::Entry { id, .. }) => {
                let id = *id;
                self.highlight = Some(index);
                self.toggle(tree, recent, id);
                Outcome::Changed
            }
            _ => Outcome::Nothing,
        }
    }

    pub fn key(&mut self, tree: &FolderTree, recent: &[FolderId], key: ListKey) -> Outcome {
        let current = self.highlight.and_then(|i| self.rows.get(i)).cloned();
        match key {
            ListKey::Escape => Outcome::Close,
            ListKey::Up | ListKey::Down => {
                let entries: Vec<usize> = (0..self.rows.len())
                    .filter(|i| self.rows.get(*i).and_then(ListRow::id).is_some())
                    .collect();
                let at = self.highlight.and_then(|h| entries.iter().position(|i| *i == h));
                let next = match (at, key) {
                    (None, _) => entries.first(),
                    (Some(p), ListKey::Up) => entries.get(p.saturating_sub(1)),
                    (Some(p), _) => entries.get((p + 1).min(entries.len().saturating_sub(1))),
                };
                self.highlight = next.copied();
                Outcome::Changed
            }
            ListKey::Left | ListKey::Right => match current {
                Some(ListRow::Entry {
                    id,
                    group: true,
                    expanded,
                    ..
                }) if expanded == (key == ListKey::Left) && self.query.is_empty() => {
                    self.toggle(tree, recent, id);
                    Outcome::Changed
                }
                _ => Outcome::Nothing,
            },
            ListKey::Enter => match current {
                Some(row @ ListRow::Entry { id, .. }) if row.pickable() => Outcome::Pick(id),
                Some(ListRow::Entry { id, group: true, .. }) => {
                    self.toggle(tree, recent, id);
                    Outcome::Changed
                }
                _ => Outcome::Nothing,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rustcut_core::folders::Spot;

    use super::*;

    /// Clients (1: North 2), Holiday 3; Holiday is a favorite.
    fn tree() -> FolderTree {
        let mut t = FolderTree::default();
        let c = t.add_group("Clients", Spot::End).unwrap();
        t.add_folder(Path::new("/w/north"), Spot::Into(c)).unwrap();
        let h = t.add_folder(Path::new("/h"), Spot::End).unwrap();
        t.set_favorite(h, true, 8).unwrap();
        t
    }

    fn ids(list: &FolderList) -> Vec<Option<u32>> {
        list.rows.iter().map(|r| r.id().map(|i| i.0)).collect()
    }

    #[test]
    fn the_list_opens_on_the_target_and_moves_with_keys() {
        let t = tree();
        let mut l = FolderList::new(&[]);
        l.open(&t, &[], FolderId(2));
        // Source, [Favorites], Holiday, [All], Clients, North, Holiday.
        assert_eq!(ids(&l), [Some(0), None, Some(3), None, Some(1), Some(2), Some(3)]);
        assert_eq!(l.highlight, Some(5), "the target's row");
        assert_eq!(l.key(&t, &[], ListKey::Up), Outcome::Changed);
        assert_eq!(l.highlight, Some(4), "headings are skipped");
        assert_eq!(l.key(&t, &[], ListKey::Enter), Outcome::Changed, "a group folds");
        assert_eq!(ids(&l), [Some(0), None, Some(3), None, Some(1), Some(3)]);
        assert_eq!(l.key(&t, &[], ListKey::Right), Outcome::Changed, "and unfolds");
        assert_eq!(l.key(&t, &[], ListKey::Right), Outcome::Nothing);
        assert_eq!(l.key(&t, &[], ListKey::Up), Outcome::Changed);
        assert_eq!(l.highlight, Some(2));
        assert_eq!(l.key(&t, &[], ListKey::Enter), Outcome::Pick(FolderId(3)));
        assert_eq!(l.click(&t, &[], 1), Outcome::Nothing, "a heading");
        assert_eq!(l.click(&t, &[], 0), Outcome::Pick(FolderId::SOURCE));
        assert_eq!(l.key(&t, &[], ListKey::Escape), Outcome::Close);
        l.close();
        assert!(!l.open && l.query.is_empty());
    }

    #[test]
    fn a_loaded_jobs_folder_stands_first() {
        let t = tree();
        let mut l = FolderList::new(&[]);
        l.job_target = Some("/media/Old".into());
        l.open(&t, &[], JOB_TARGET);
        assert_eq!(l.rows.first().and_then(ListRow::id), Some(JOB_TARGET));
        assert_eq!(l.highlight, Some(0));
        assert_eq!(l.key(&t, &[], ListKey::Enter), Outcome::Pick(JOB_TARGET));
        l.search(&t, &[], "old");
        assert_eq!(ids(&l), [Some(u32::MAX)]);
        l.search(&t, &[], "drone");
        assert!(ids(&l).is_empty());
        l.job_target = None;
        l.search(&t, &[], "");
        assert_eq!(l.rows.first().and_then(ListRow::id), Some(FolderId::SOURCE));
    }

    #[test]
    fn typing_searches_and_folds_are_kept() {
        let t = tree();
        let mut l = FolderList::new(&[FolderId(1)]);
        l.open(&t, &[], FolderId::SOURCE);
        assert!(!ids(&l).contains(&Some(2)), "Clients is folded");
        l.search(&t, &[], "nor");
        assert_eq!(ids(&l), [Some(2)], "hits from folded groups too");
        assert_eq!(l.highlight, Some(0));
        assert_eq!(
            l.key(&t, &[], ListKey::Left),
            Outcome::Nothing,
            "no folding while searching"
        );
        l.search(&t, &[], "");
        assert_eq!(l.highlight, Some(0));
        assert!(l.collapsed.contains(&FolderId(1)));
    }
}
