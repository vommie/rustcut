//! Which tags the panel's tree shows (P8b, user decision Q-P8b): a mark on a tag hides or shows it with everything below
//! it, also tags RustView adds there later; the nearest mark on the way up decides, no mark = shown. Checking a branch
//! in the filter window (a check = hidden, Q-P9) makes the whole branch follow: the marks below it go, and a single tag
//! below can be switched on again afterwards. The window edits a copy of the marks (its draft) until Apply (D-P9n);
//! its rows come from here too.

use std::collections::{HashMap, HashSet};

use super::state::TagPanel;

/// The marks that hide tags: a tag in `hidden` hides with everything below it, one in `shown` shows again inside a
/// hidden branch (with everything below it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pub hidden: HashSet<u32>,
    pub shown: HashSet<u32>,
}

impl Marks {
    /// Drops the marks of tags RustView does not know any more.
    pub fn retain(&mut self, valid: &HashSet<u32>) {
        self.hidden.retain(|id| valid.contains(id));
        self.shown.retain(|id| valid.contains(id));
    }
}

/// A tag's check box in the filter window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// It and everything below it show (no check).
    Shown,
    /// It and everything below it are hidden (checked).
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

    fn shape(marks: &Marks, children: &HashMap<Option<u32>, Vec<u32>>) -> Shape {
        fn walk(
            marks: &Marks,
            id: u32,
            parent_visible: bool,
            children: &HashMap<Option<u32>, Vec<u32>>,
            shape: &mut Shape,
        ) -> (bool, bool) {
            let visible = if marks.hidden.contains(&id) {
                false
            } else if marks.shown.contains(&id) {
                true
            } else {
                parent_visible
            };
            shape.visible.insert(id, visible);
            let mut below = (false, false);
            for kid in children.get(&Some(id)).into_iter().flatten() {
                // Reached once: every tag has one parent, so the walk from the roots is a tree.
                let (shown, hidden) = walk(marks, *kid, visible, children, shape);
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
            walk(marks, *root, true, children, &mut shape);
        }
        shape
    }

    /// Whether the tree shows tag `id` itself: the nearest mark on its way up decides (a loop in the parents ends it).
    pub fn visible(&self, id: u32) -> bool {
        self.visible_in(&self.marks, id)
    }

    fn visible_in(&self, marks: &Marks, id: u32) -> bool {
        let mut next = Some(id);
        for _ in 0..=self.tag_count() {
            let Some(at) = next else { break };
            if marks.hidden.contains(&at) {
                return false;
            }
            if marks.shown.contains(&at) {
                return true;
            }
            next = self.tag(at).and_then(|t| t.parent_id);
        }
        true
    }

    /// Hides tag `id` with its branch, or shows it again. The branch follows as a whole: the marks below it go.
    pub fn toggle_branch(&mut self, id: u32) {
        let mut marks = std::mem::take(&mut self.marks);
        self.toggle_in(&mut marks, id);
        self.marks = marks;
    }

    fn toggle_in(&self, marks: &mut Marks, id: u32) {
        if self.tag(id).is_none() {
            return;
        }
        let want = !self.visible_in(marks, id);
        for below in self.below(id) {
            marks.hidden.remove(&below);
            marks.shown.remove(&below);
        }
        marks.hidden.remove(&id);
        marks.shown.remove(&id);
        let inherited = self
            .tag(id)
            .and_then(|t| t.parent_id)
            .is_none_or(|parent| self.visible_in(marks, parent));
        if want != inherited {
            if want {
                marks.shown.insert(id);
            } else {
                marks.hidden.insert(id);
            }
        }
    }

    /// How many tags the tree does not show (the filter button's number).
    pub fn hidden_count(&self) -> usize {
        self.hidden_count_in(&self.marks)
    }

    fn hidden_count_in(&self, marks: &Marks) -> usize {
        if marks.hidden.is_empty() {
            return 0;
        }
        self.tags().filter(|t| !self.visible_in(marks, t.id)).count()
    }

    /// The marks the filter window shows: its draft while it is open, else the panel's.
    fn filter_marks(&self) -> &Marks {
        self.filter_draft.as_ref().unwrap_or(&self.marks)
    }

    /// The filter window opens: it edits a copy of the marks (an open draft is kept).
    pub fn begin_filter(&mut self) {
        if self.filter_draft.is_none() {
            self.filter_draft = Some(self.marks.clone());
        }
    }

    /// A check box of the filter window: hides the branch in the draft or shows it again.
    pub fn toggle_filter(&mut self, id: u32) {
        let mut draft = self.filter_draft.take().unwrap_or_else(|| self.marks.clone());
        self.toggle_in(&mut draft, id);
        self.filter_draft = Some(draft);
    }

    /// "Show all": the draft hides nothing.
    pub fn clear_filter(&mut self) {
        self.filter_draft = Some(Marks::default());
    }

    /// Apply: the draft becomes the panel's marks. `true` when that changed them.
    pub fn apply_filter(&mut self) -> bool {
        let Some(draft) = self.filter_draft.take() else {
            return false;
        };
        let changed = draft != self.marks;
        self.marks = draft;
        changed
    }

    /// Escape or the window's ×: the draft is dropped.
    pub fn discard_filter(&mut self) {
        self.filter_draft = None;
    }

    /// How many tags the filter window's draft hides.
    pub fn filter_hidden_count(&self) -> usize {
        self.hidden_count_in(self.filter_marks())
    }

    /// The filter window's rows (from its draft): every tag in tree order (folded branches without their children);
    /// while `search` holds text, only the tags whose names contain it and the way to them, all of it open.
    pub fn filter_rows(&self, search: &str) -> Vec<FilterRow> {
        let search = search.trim().to_lowercase();
        let children = self.children_map();
        let shape = Self::shape(self.filter_marks(), &children);
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
        assert!(p.visible(7) && p.marks == Marks::default());
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
        assert!(p.visible(2) && p.marks == Marks::default());
        p.toggle_branch(1);
        assert!(!p.visible(3), "hidden again as a whole");
        // Hiding a tag inside a shown branch is a plain mark.
        p.marks = Marks::default();
        p.toggle_branch(4);
        assert_eq!((p.marks.hidden.len(), p.marks.shown.len(), p.hidden_count()), (1, 0, 1));
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
    fn the_filter_window_edits_a_draft_until_apply() {
        let mut p = panel();
        p.expanded = (1..=6).collect();
        p.begin_filter();
        p.toggle_filter(1);
        p.toggle_filter(3);
        assert_eq!(p.hidden_count(), 0, "the tree does not change before Apply");
        assert_eq!(p.filter_hidden_count(), 2, "the window counts its draft");
        assert_eq!(
            checks(&p, "").get(..3).map(<[_]>::to_vec),
            Some(vec![
                ("People".to_owned(), Check::Mixed),
                ("Family".to_owned(), Check::Hidden),
                ("Friends".to_owned(), Check::Shown),
            ])
        );
        p.discard_filter();
        assert_eq!(
            (p.filter_hidden_count(), p.filter_draft.is_none()),
            (0, true),
            "Escape drops the draft"
        );

        p.begin_filter();
        p.toggle_filter(1);
        p.toggle_filter(3);
        assert!(p.apply_filter());
        assert!(!p.apply_filter(), "nothing left to apply");
        assert_eq!(p.draft.tags, Vec::<u32>::new(), "hiding assigns nothing");
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
        assert_eq!((restored.marks.hidden.len(), restored.marks.shown.len()), (1, 1));

        // "Show all" clears the draft's checks; a new tree prunes the draft too.
        p.begin_filter();
        p.clear_filter();
        assert_eq!((p.filter_hidden_count(), p.hidden_count()), (0, 2));
        p.toggle_filter(6);
        let mut smaller = (*tree(&[])).clone();
        smaller.tags.retain(|t| t.id != 6);
        p.set_tree(Arc::new(smaller));
        assert_eq!(p.filter_draft, Some(Marks::default()));
        assert!(p.apply_filter() && p.hidden_count() == 0);
    }
}
