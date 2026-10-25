//! Which tags the panel's tree shows (P8b, user decision Q-P8b): a mark on a tag hides or shows it with everything below
//! it, also tags RustView adds there later; the nearest mark on the way up decides, no mark = shown. Unchecking a
//! branch in the filter window (or a click in the eye mode) makes the whole branch follow: the marks below it go, and a
//! single tag below can be switched on again afterwards. The filter window's rows come from here too.

use std::collections::HashMap;

use super::state::TagPanel;

/// A tag's check box in the filter window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// It and everything below it show.
    Shown,
    /// It and everything below it are hidden.
    Hidden,
    /// It shows or not, and some tags below it differ.
    Mixed,
}

/// One row of the filter window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRow {
    pub id: u32,
    pub name: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub check: Check,
    /// Only on the way to a tag the search found.
    pub dimmed: bool,
}

/// Every tag's visibility and what its branch below holds (shown ones, hidden ones), in one walk over the tree.
struct Shape {
    visible: HashMap<u32, bool>,
    below: HashMap<u32, (bool, bool)>,
}

impl TagPanel {
    /// The children of every tag (`None` = the roots), sorted by name like the tree (RustView's order).
    fn children_map(&self) -> HashMap<Option<u32>, Vec<u32>> {
        let mut map: HashMap<Option<u32>, Vec<(String, u32)>> = HashMap::new();
        for tag in self.tags() {
            map.entry(tag.parent_id)
                .or_default()
                .push((tag.name.to_lowercase(), tag.id));
        }
        map.into_iter()
            .map(|(parent, mut kids)| {
                kids.sort();
                (parent, kids.into_iter().map(|(_, id)| id).collect())
            })
            .collect()
    }

    fn shape(&self, children: &HashMap<Option<u32>, Vec<u32>>) -> Shape {
        fn walk(
            panel: &TagPanel,
            id: u32,
            parent_visible: bool,
            children: &HashMap<Option<u32>, Vec<u32>>,
            shape: &mut Shape,
        ) -> (bool, bool) {
            let visible = if panel.hidden.contains(&id) {
                false
            } else if panel.shown.contains(&id) {
                true
            } else {
                parent_visible
            };
            shape.visible.insert(id, visible);
            let mut below = (false, false);
            for kid in children.get(&Some(id)).into_iter().flatten() {
                // Reached once: every tag has one parent, so the walk from the roots is a tree.
                let (shown, hidden) = walk(panel, *kid, visible, children, shape);
                below = (below.0 || shown, below.1 || hidden);
            }
            shape.below.insert(id, below);
            (below.0 || visible, below.1 || !visible)
        }
        let mut shape = Shape {
            visible: HashMap::new(),
            below: HashMap::new(),
        };
        for root in children.get(&None).into_iter().flatten() {
            walk(self, *root, true, children, &mut shape);
        }
        shape
    }

    /// Whether the tree shows tag `id` itself: the nearest mark on its way up decides (a loop in the parents ends it).
    pub fn visible(&self, id: u32) -> bool {
        let mut next = Some(id);
        for _ in 0..=self.tag_count() {
            let Some(at) = next else { break };
            if self.hidden.contains(&at) {
                return false;
            }
            if self.shown.contains(&at) {
                return true;
            }
            next = self.tag(at).and_then(|t| t.parent_id);
        }
        true
    }

    /// Hides tag `id` with its branch, or shows it again (the filter window's check box, a click in the eye mode). The
    /// branch follows as a whole: the marks below it go.
    pub fn toggle_branch(&mut self, id: u32) {
        if self.tag(id).is_none() {
            return;
        }
        let want = !self.visible(id);
        for below in self.below(id) {
            self.hidden.remove(&below);
            self.shown.remove(&below);
        }
        self.hidden.remove(&id);
        self.shown.remove(&id);
        let inherited = self
            .tag(id)
            .and_then(|t| t.parent_id)
            .is_none_or(|parent| self.visible(parent));
        if want != inherited {
            if want {
                self.shown.insert(id);
            } else {
                self.hidden.insert(id);
            }
        }
    }

    /// Every tag shows again ("Show all").
    pub fn show_all(&mut self) {
        self.hidden.clear();
        self.shown.clear();
    }

    /// How many tags the tree does not show (the filter button's number).
    pub fn hidden_count(&self) -> usize {
        if self.hidden.is_empty() {
            return 0;
        }
        let ids: Vec<u32> = self.tags().map(|t| t.id).collect();
        ids.into_iter().filter(|id| !self.visible(*id)).count()
    }

    /// The filter window's rows: every tag in tree order (folded branches without their children); while `search`
    /// holds text, only the tags whose names contain it and the way to them, all of it open.
    pub fn filter_rows(&self, search: &str) -> Vec<FilterRow> {
        let search = search.trim().to_lowercase();
        let children = self.children_map();
        let shape = self.shape(&children);
        let mut rows = Vec::new();
        for root in children.get(&None).into_iter().flatten() {
            self.filter_rows_of(*root, 0, (&children, &shape), &search, &mut rows);
        }
        rows
    }

    fn filter_rows_of(
        &self,
        id: u32,
        depth: usize,
        tree: (&HashMap<Option<u32>, Vec<u32>>, &Shape),
        search: &str,
        rows: &mut Vec<FilterRow>,
    ) -> bool {
        let (children, shape) = tree;
        let Some(tag) = self.tag(id) else { return false };
        let kids = children.get(&Some(id)).map_or(&[][..], Vec::as_slice);
        let searching = !search.is_empty();
        let matched = searching && tag.name.to_lowercase().contains(search);
        let visible = shape.visible.get(&id).copied().unwrap_or(true);
        let (shown_below, hidden_below) = shape.below.get(&id).copied().unwrap_or_default();
        let check = match (visible, shown_below, hidden_below) {
            (true, _, true) | (false, true, _) => Check::Mixed,
            (true, _, false) => Check::Shown,
            (false, false, _) => Check::Hidden,
        };
        let expanded = searching || !self.filter_collapsed.contains(&id);
        let at = rows.len();
        rows.push(FilterRow {
            id,
            name: tag.name.trim().to_owned(),
            depth,
            has_children: !kids.is_empty(),
            expanded,
            check,
            dimmed: false,
        });
        let mut found_below = false;
        if expanded {
            for kid in kids {
                found_below |= self.filter_rows_of(*kid, depth + 1, tree, search, rows);
            }
        }
        if searching && !matched && !found_below {
            rows.truncate(at);
            return false;
        }
        if let Some(row) = rows.get_mut(at) {
            row.dimmed = searching && !matched;
        }
        true
    }

    /// A branch of the filter window opens or folds.
    pub fn toggle_filter_expanded(&mut self, id: u32) {
        if !self.filter_collapsed.remove(&id) {
            self.filter_collapsed.insert(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rustcut_core::rustview::{TagItem, TagTree};
    use rustcut_core::settings::TagPanelState;

    use super::*;

    /// People (1) with Family (2) and Friends (3), Friends with Anna (4); Travel (5) with Japan (6).
    fn tree(extra: &[(u32, Option<u32>, &str)]) -> Arc<TagTree> {
        let mut list = vec![
            (1, None, "People"),
            (2, Some(1), "Family"),
            (3, Some(1), "Friends"),
            (4, Some(3), "Anna"),
            (5, None, "Travel"),
            (6, Some(5), "Japan"),
        ];
        list.extend_from_slice(extra);
        let tags: Vec<TagItem> = list
            .into_iter()
            .map(|(id, parent_id, name)| TagItem {
                id,
                parent_id,
                name: name.into(),
            })
            .collect();
        Arc::new(TagTree {
            success: true,
            total_tags: tags.len(),
            tags,
        })
    }

    fn panel() -> TagPanel {
        let mut p = TagPanel::new(&TagPanelState::default());
        p.set_tree(tree(&[]));
        p
    }

    fn checks(p: &TagPanel, search: &str) -> Vec<(String, Check)> {
        p.filter_rows(search).into_iter().map(|r| (r.name, r.check)).collect()
    }

    #[test]
    fn a_branch_hides_with_tags_added_later() {
        let mut p = panel();
        p.toggle_branch(1);
        assert!(!p.visible(1) && !p.visible(4) && p.visible(6));
        assert_eq!(p.hidden_count(), 4);
        // RustView gets a new tag below People: it is hidden too.
        p.set_tree(tree(&[(7, Some(3), "Bob")]));
        assert!(!p.visible(7));
        assert_eq!(p.hidden_count(), 5);
        p.toggle_branch(1);
        assert!(p.visible(7) && p.hidden.is_empty() && p.shown.is_empty());
    }

    #[test]
    fn a_tag_below_a_hidden_one_can_be_shown_again() {
        let mut p = panel();
        p.toggle_branch(1);
        p.toggle_branch(3);
        assert!(p.visible(3) && p.visible(4), "Friends and Anna show again");
        assert!(!p.visible(1) && !p.visible(2));
        assert_eq!(p.hidden_count(), 2);
        assert_eq!(
            checks(&p, ""),
            [
                ("People".to_owned(), Check::Mixed),
                ("Family".to_owned(), Check::Hidden),
                ("Friends".to_owned(), Check::Shown),
                ("Anna".to_owned(), Check::Shown),
                ("Travel".to_owned(), Check::Shown),
                ("Japan".to_owned(), Check::Shown),
            ]
        );
        // A check on the branch makes it follow as a whole: the exception goes.
        p.toggle_branch(1);
        assert!(p.visible(2) && p.hidden.is_empty() && p.shown.is_empty());
        p.toggle_branch(1);
        assert!(!p.visible(3), "hidden again as a whole");
        // Hiding a tag inside a shown branch is a plain mark.
        p.show_all();
        p.toggle_branch(4);
        assert_eq!((p.hidden.len(), p.shown.len(), p.hidden_count()), (1, 0, 1));
        assert_eq!(checks(&p, "").get(2), Some(&("Friends".to_owned(), Check::Mixed)));
    }

    #[test]
    fn the_search_shows_matches_with_the_way_to_them() {
        let mut p = panel();
        p.toggle_filter_expanded(1);
        let rows = p.filter_rows("");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["People", "Travel", "Japan"],
            "a folded branch hides its rows"
        );
        let found = p.filter_rows("ann");
        assert_eq!(
            found
                .iter()
                .map(|r| (r.name.as_str(), r.depth, r.dimmed, r.expanded))
                .collect::<Vec<_>>(),
            [
                ("People", 0, true, true),
                ("Friends", 1, true, true),
                ("Anna", 2, false, true)
            ],
            "searching opens folded branches"
        );
        assert!(p.filter_rows("nothing").is_empty());
        p.toggle_filter_expanded(1);
        assert!(p.filter_collapsed.is_empty());
    }

    #[test]
    fn the_eye_mode_and_the_tree_use_the_same_marks() {
        let mut p = panel();
        p.expand_all();
        p.toggle_hide_mode();
        p.click_tag(1);
        p.click_tag(3);
        p.toggle_hide_mode();
        assert_eq!(p.draft.tags, Vec::<u32>::new(), "the eye mode assigns nothing");
        let rows: Vec<(u32, bool)> = p
            .rows()
            .iter()
            .map(|r| (u32::try_from(r.id).unwrap(), r.is_dimmed))
            .collect();
        assert_eq!(
            rows,
            [(1, true), (3, false), (4, false), (5, false), (6, false)],
            "People stays dimmed as the way to Friends; Family is left out"
        );
        let mut saved = TagPanelState::default();
        p.save_into(&mut saved);
        assert_eq!((saved.hidden.clone(), saved.shown.clone()), (vec![1], vec![3]));
        let restored = TagPanel::new(&saved);
        assert_eq!((restored.hidden.len(), restored.shown.len()), (1, 1));
    }
}
