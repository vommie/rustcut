//! The tag tree, ported from RustView (`crates/rustview/src/ui/tags/tree.rs`: `TagTreeBuilder::{build_hierarchy,
//! build_branch, flatten, chunk_into_columns}`; the constants come from `crates/rustview/src/ui/tags/mod.rs`, ADR-10).
//! What differs: the tags are the API's `TagItem`s; RustView's selection statistics over several files become the one
//! job's assigned tags (RustCut's panel is always in assign mode, so nothing is "selected" or "partial"); RustCut adds
//! locked tags and hidden branches (a tag shown again inside a hidden branch keeps the way to it in the tree, dimmed).
//! `tests/rustview_parity.rs` pins RustView's functions, so a change there is noticed.

use std::collections::{HashMap, HashSet};

use regex::Regex;
use rustcut_core::rustview::TagItem;
use slint::{ModelRc, VecModel};

use crate::{TagColumnView, TagNodeView};

pub const MAX_TAG_ITEM_WIDTH: f32 = 180.0;
pub const MIN_TAG_ITEM_WIDTH: f32 = 60.0;
pub const TAG_ITEM_HEIGHT: f32 = 22.0;
pub const TAG_ROW_SPACING: f32 = 2.0;
pub const TAG_COLUMN_SPACING: f32 = 6.0;
pub const TAG_PADDING_H: f32 = 4.0;
pub const TAG_PADDING_V: f32 = 4.0;

#[derive(Debug, Clone)]
pub struct TagTreeNode {
    pub record: TagItem,
    pub children: Vec<TagTreeNode>,
}

/// What one flattening pass needs to know.
#[derive(Debug, Clone)]
pub struct TagFlattenContext<'a> {
    pub expanded_set: &'a HashSet<u32>,
    /// The filter, trimmed and in lower case (empty = no filter).
    pub filter_text: &'a str,
    /// The filter as a regular expression (`None` = substring matching).
    pub filter_regex: Option<&'a Regex>,
    pub assigned: &'a HashSet<u32>,
    pub locked: &'a HashSet<u32>,
    /// Branches the user hid: left out, or shown dimmed while they are being chosen (`show_hidden`).
    pub hidden: &'a HashSet<u32>,
    /// Tags shown again inside a hidden branch (with their branch); the hidden tags above them stay as the way there.
    pub shown: &'a HashSet<u32>,
    pub show_hidden: bool,
}

impl TagFlattenContext<'_> {
    /// Whether a tag shows itself: its own mark, else its parent's state.
    fn visible(&self, id: u32, parent_visible: bool) -> bool {
        if self.hidden.contains(&id) {
            false
        } else if self.shown.contains(&id) {
            true
        } else {
            parent_visible
        }
    }

    /// Whether a tag of `nodes` (or below them) shows itself.
    fn shows_below(&self, nodes: &[TagTreeNode], parent_visible: bool) -> bool {
        nodes.iter().any(|n| {
            let visible = self.visible(n.record.id, parent_visible);
            visible || (!self.shown.is_empty() && self.shows_below(&n.children, visible))
        })
    }
}

pub struct TagTreeBuilder;

impl TagTreeBuilder {
    /// The tags as a forest, every level sorted by name (case-insensitive). A tag whose parent does not exist is not
    /// reachable from a root and left out (RustView's rule).
    pub fn build_hierarchy(tags: &[TagItem]) -> Vec<TagTreeNode> {
        let mut children_map: HashMap<Option<u32>, Vec<TagItem>> = HashMap::new();
        for t in tags {
            children_map.entry(t.parent_id).or_default().push(t.clone());
        }

        for list in children_map.values_mut() {
            list.sort_by_key(|a| a.name.to_lowercase());
        }

        let mut seen = HashSet::new();
        Self::build_branch(None, &children_map, &mut seen)
    }

    /// `seen` guards against a parent chain that loops back (RustView's ids come from one tree, but the API is data).
    fn build_branch(
        parent_id: Option<u32>,
        children_map: &HashMap<Option<u32>, Vec<TagItem>>,
        seen: &mut HashSet<u32>,
    ) -> Vec<TagTreeNode> {
        let mut nodes = Vec::new();
        if let Some(children) = children_map.get(&parent_id) {
            for child in children {
                if !seen.insert(child.id) {
                    continue;
                }
                let sub_nodes = Self::build_branch(Some(child.id), children_map, seen);
                nodes.push(TagTreeNode {
                    record: child.clone(),
                    children: sub_nodes,
                });
            }
        }
        nodes
    }

    /// Appends the visible rows of `nodes` to `output` (depth first); returns whether any of them matched the filter.
    /// While filtering every branch is searched, rows that only lead to a match are dimmed, others are dropped.
    pub fn flatten(
        nodes: &[TagTreeNode],
        depth: i32,
        ctx: &TagFlattenContext<'_>,
        output: &mut Vec<TagNodeView>,
    ) -> bool {
        Self::flatten_inner(nodes, depth, true, ctx, output)
    }

    fn flatten_inner(
        nodes: &[TagTreeNode],
        depth: i32,
        parent_visible: bool,
        ctx: &TagFlattenContext<'_>,
        output: &mut Vec<TagNodeView>,
    ) -> bool {
        let mut any_match_in_branch = false;
        let is_filtering = !ctx.filter_text.is_empty();

        for node in nodes {
            let visible = ctx.visible(node.record.id, parent_visible);
            let hidden = !visible;
            // A hidden tag stays as the way to a tag shown again below it.
            let way_there = hidden && !ctx.shown.is_empty() && ctx.shows_below(&node.children, visible);
            if hidden && !way_there && !ctx.show_hidden {
                continue;
            }
            let has_children = !node.children.is_empty();
            let is_expanded = ctx.expanded_set.contains(&node.record.id);

            let name_match = if !is_filtering || (hidden && !ctx.show_hidden) {
                false
            } else if let Some(re) = ctx.filter_regex {
                re.is_match(&node.record.name)
            } else {
                node.record.name.to_lowercase().contains(ctx.filter_text)
            };

            let start_idx = output.len();

            output.push(TagNodeView {
                id: i32::try_from(node.record.id).unwrap_or(i32::MAX),
                name: node.record.name.as_str().into(),
                depth,
                has_children,
                is_expanded,
                is_selected: false,
                is_assigned: ctx.assigned.contains(&node.record.id),
                is_partial: false,
                is_dimmed: hidden,
                is_locked: ctx.locked.contains(&node.record.id),
            });

            let mut child_matches = false;
            if has_children && (is_expanded || is_filtering) {
                child_matches = Self::flatten_inner(&node.children, depth + 1, visible, ctx, output);
            }

            if is_filtering && !name_match && !child_matches {
                output.truncate(start_idx);
            } else {
                let is_dimmed = hidden || (is_filtering && !name_match && child_matches);
                if let Some(row) = output.get_mut(start_idx) {
                    row.is_dimmed = is_dimmed;
                }
                any_match_in_branch = true;
            }
        }

        any_match_in_branch
    }

    /// Splits the rows into columns that fit `available_height` (logical pixels); returns them with the rows per column.
    pub fn chunk_into_columns(nodes: &[TagNodeView], available_height: f32) -> (Vec<Vec<TagNodeView>>, usize) {
        let rows_per_col = Self::rows_per_column(available_height);
        if nodes.is_empty() {
            return (Vec::new(), rows_per_col);
        }
        (nodes.chunks(rows_per_col).map(<[_]>::to_vec).collect(), rows_per_col)
    }

    /// How many rows fit into one column of `available_height` (at least one).
    pub fn rows_per_column(available_height: f32) -> usize {
        let usable_h = (available_height - (TAG_PADDING_V * 2.0)).max(TAG_ITEM_HEIGHT);
        let stride_y = TAG_ITEM_HEIGHT + TAG_ROW_SPACING;
        let rows = ((usable_h + TAG_ROW_SPACING) / stride_y).floor().max(1.0);
        az::checked_cast::<f32, usize>(rows).unwrap_or(1)
    }
}

/// A column for Slint around its row model.
pub fn column_view(rows: &std::rc::Rc<VecModel<TagNodeView>>) -> TagColumnView {
    TagColumnView {
        tags: ModelRc::from(std::rc::Rc::clone(rows)),
    }
}

/// Every id below `id` (the tag itself not included).
pub fn descendants(tags: &[TagItem], id: u32) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for t in tags {
        if let Some(parent) = t.parent_id {
            children.entry(parent).or_default().push(t.id);
        }
    }
    let mut found = HashSet::new();
    let mut stack = vec![id];
    while let Some(next) = stack.pop() {
        for &child in children.get(&next).into_iter().flatten() {
            if found.insert(child) {
                stack.push(child);
            }
        }
    }
    found.remove(&id);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: u32, parent: Option<u32>, name: &str) -> TagItem {
        TagItem {
            id,
            parent_id: parent,
            name: name.into(),
        }
    }

    fn sample() -> Vec<TagItem> {
        vec![
            tag(1, None, "Places"),
            tag(2, Some(1), "beach"),
            tag(3, Some(1), "Alps"),
            tag(4, None, "people"),
            tag(5, Some(4), "Anna"),
            tag(6, Some(3), "Glacier"),
            tag(7, Some(99), "orphan"),
        ]
    }

    struct Sets {
        expanded: HashSet<u32>,
        assigned: HashSet<u32>,
        locked: HashSet<u32>,
        hidden: HashSet<u32>,
        shown: HashSet<u32>,
    }

    fn sets(expanded: &[u32], hidden: &[u32]) -> Sets {
        Sets {
            expanded: expanded.iter().copied().collect(),
            assigned: [2, 5].into_iter().collect(),
            locked: [5].into_iter().collect(),
            hidden: hidden.iter().copied().collect(),
            shown: HashSet::new(),
        }
    }

    fn rows(sets: &Sets, filter: &str, regex: Option<&Regex>, show_hidden: bool) -> Vec<(String, i32, bool)> {
        let tree = TagTreeBuilder::build_hierarchy(&sample());
        let ctx = TagFlattenContext {
            expanded_set: &sets.expanded,
            filter_text: filter,
            filter_regex: regex,
            assigned: &sets.assigned,
            locked: &sets.locked,
            hidden: &sets.hidden,
            shown: &sets.shown,
            show_hidden,
        };
        let mut out = Vec::new();
        TagTreeBuilder::flatten(&tree, 0, &ctx, &mut out);
        out.into_iter()
            .map(|r| (r.name.to_string(), r.depth, r.is_dimmed))
            .collect()
    }

    fn names(rows: &[(String, i32, bool)]) -> Vec<&str> {
        rows.iter().map(|r| r.0.as_str()).collect()
    }

    #[test]
    fn builds_sorted_levels_and_follows_expansion() {
        let collapsed = rows(&sets(&[], &[]), "", None, false);
        assert_eq!(names(&collapsed), ["people", "Places"], "orphans are not reachable");
        let open = rows(&sets(&[1, 3], &[]), "", None, false);
        assert_eq!(names(&open), ["people", "Places", "Alps", "Glacier", "beach"]);
        assert_eq!(open.iter().map(|r| r.1).collect::<Vec<_>>(), [0, 0, 1, 2, 1]);
    }

    #[test]
    fn flags_assigned_and_locked_tags() {
        let tree = TagTreeBuilder::build_hierarchy(&sample());
        let s = sets(&[1, 4], &[]);
        let ctx = TagFlattenContext {
            expanded_set: &s.expanded,
            filter_text: "",
            filter_regex: None,
            assigned: &s.assigned,
            locked: &s.locked,
            hidden: &s.hidden,
            shown: &s.shown,
            show_hidden: false,
        };
        let mut out = Vec::new();
        TagTreeBuilder::flatten(&tree, 0, &ctx, &mut out);
        let anna = out.iter().find(|r| r.id == 5).unwrap();
        let beach = out.iter().find(|r| r.id == 2).unwrap();
        assert!(anna.is_assigned && anna.is_locked);
        assert!(beach.is_assigned && !beach.is_locked);
        assert!(out.iter().all(|r| !r.is_selected && !r.is_partial));
    }

    #[test]
    fn filtering_searches_closed_branches_and_dims_the_way_there() {
        let found = rows(&sets(&[], &[]), "glac", None, false);
        assert_eq!(
            found,
            [
                ("Places".to_owned(), 0, true),
                ("Alps".to_owned(), 1, true),
                ("Glacier".to_owned(), 2, false)
            ]
        );
        let re = regex::RegexBuilder::new("^a").case_insensitive(true).build().unwrap();
        assert_eq!(
            names(&rows(&sets(&[], &[]), "^a", Some(&re), false)),
            ["people", "Anna", "Places", "Alps"]
        );
        assert!(rows(&sets(&[], &[]), "nothing", None, false).is_empty());
    }

    #[test]
    fn hidden_branches_are_left_out_or_dimmed_while_chosen() {
        let s = sets(&[1, 3], &[3]);
        assert_eq!(names(&rows(&s, "", None, false)), ["people", "Places", "beach"]);
        let choosing = rows(&s, "", None, true);
        assert_eq!(names(&choosing), ["people", "Places", "Alps", "Glacier", "beach"]);
        let dimmed: Vec<_> = choosing.iter().filter(|r| r.2).map(|r| r.0.as_str()).collect();
        assert_eq!(dimmed, ["Alps", "Glacier"], "the hidden tag and everything below it");
        assert!(
            rows(&s, "glac", None, false).is_empty(),
            "a hidden tag is not found by the filter"
        );
    }

    #[test]
    fn a_tag_shown_again_keeps_the_way_to_it() {
        // Places (1) is hidden, Alps (3) below it is shown again: Places stays dimmed as the way to Alps and Glacier,
        // beach (2) is left out (user decision Q-P8b).
        let mut s = sets(&[1, 3], &[1]);
        s.shown.insert(3);
        let rows = rows(&s, "", None, false);
        assert_eq!(
            rows,
            [
                ("people".to_owned(), 0, false),
                ("Places".to_owned(), 0, true),
                ("Alps".to_owned(), 1, false),
                ("Glacier".to_owned(), 2, false)
            ]
        );
        assert_eq!(
            names(&rows_of(&s, "glac")),
            ["Places", "Alps", "Glacier"],
            "the filter finds the shown tag"
        );
        assert!(rows_of(&s, "beach").is_empty(), "a hidden tag is not found");
    }

    fn rows_of(s: &Sets, filter: &str) -> Vec<(String, i32, bool)> {
        rows(s, filter, None, false)
    }

    #[test]
    fn columns_fit_the_height() {
        // RustView: 4 px padding at the top and bottom, rows of 22 px with 2 px between them.
        assert_eq!(TagTreeBuilder::rows_per_column(0.0), 1);
        assert_eq!(TagTreeBuilder::rows_per_column(100.0), 3);
        assert_eq!(TagTreeBuilder::rows_per_column(104.0), 4);
        let s = sets(&[1, 3, 4], &[]);
        let tree = TagTreeBuilder::build_hierarchy(&sample());
        let ctx = TagFlattenContext {
            expanded_set: &s.expanded,
            filter_text: "",
            filter_regex: None,
            assigned: &s.assigned,
            locked: &s.locked,
            hidden: &s.hidden,
            shown: &s.shown,
            show_hidden: false,
        };
        let mut out = Vec::new();
        TagTreeBuilder::flatten(&tree, 0, &ctx, &mut out);
        let (columns, per) = TagTreeBuilder::chunk_into_columns(&out, 80.0);
        assert_eq!((per, columns.iter().map(Vec::len).collect::<Vec<_>>()), (3, vec![3, 3]));
        assert!(TagTreeBuilder::chunk_into_columns(&[], 80.0).0.is_empty());
    }

    #[test]
    fn loops_in_the_parent_chain_end() {
        let looped = [
            tag(1, None, "root"),
            tag(2, Some(3), "a"),
            tag(3, Some(2), "b"),
            tag(4, Some(1), "c"),
        ];
        let tree = TagTreeBuilder::build_hierarchy(&looped);
        assert_eq!(tree.len(), 1);
        assert_eq!(descendants(&looped, 2), [3].into_iter().collect());
        assert_eq!(descendants(&sample(), 1), [2, 3, 6].into_iter().collect());
    }
}
