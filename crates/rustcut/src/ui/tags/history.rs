//! The tag history, ported from RustView (`crates/rustview/src/ui/tags/history/manager.rs`: `TagHistoryManager::
//! {record_tag_used, prune_invalid, build_hierarchy_tooltip}`, `layout.rs`: `TagHistoryLayoutEngine`, `models.rs`; the
//! item flags follow `history_bridge.rs::sync_ui` for one selected file, ADR-10). RustCut's tags are the API's
//! `TagItem`s; RustCut's own since 0.0.72 (no parity check any more).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use rustcut_core::rustview::TagItem;

/// One ready-to-render pill of the tag history.
#[derive(Debug, Clone, PartialEq)]
pub struct TagHistoryItem {
    pub id: u32,
    pub name: String,
    pub display_text: String,
    pub is_assigned: bool,
    pub is_partial: bool,
    pub is_bold: bool,
    pub is_italic: bool,
    pub width: f32,
    pub tooltip: String,
}

/// A row of pills packed to the panel's width.
#[derive(Debug, Clone, PartialEq)]
pub struct TagHistoryRow {
    pub items: Vec<TagHistoryItem>,
}

pub struct TagHistoryManager;

impl TagHistoryManager {
    /// Records a tag as used: moved to the front, no duplicates, at most `max_items`.
    pub fn record_tag_used(history: &mut Vec<u32>, tag_id: u32, max_items: usize) {
        history.retain(|&id| id != tag_id);
        history.insert(0, tag_id);
        history.truncate(max_items);
    }

    /// Drops ids RustView does not know (any more).
    pub fn prune_invalid(history: &mut Vec<u32>, valid_tag_ids: &HashSet<u32>) {
        history.retain(|id| valid_tag_ids.contains(id));
    }

    /// The tag's lineage as a small text tree ("Places\n`-- Beach"); a root tag gives its name only.
    pub fn build_hierarchy_tooltip(tag_id: u32, all_tags_map: &HashMap<u32, TagItem>) -> String {
        let Some(tag) = all_tags_map.get(&tag_id) else {
            return String::new();
        };

        let clean_name = tag.name.trim();

        let mut ancestors = Vec::new();
        let mut current_pid = tag.parent_id;
        let mut seen = HashSet::from([tag_id]);

        while let Some(pid) = current_pid {
            if !seen.insert(pid) {
                break;
            }
            if let Some(parent) = all_tags_map.get(&pid) {
                ancestors.push(parent.name.trim().to_string());
                current_pid = parent.parent_id;
            } else {
                break;
            }
        }

        if ancestors.is_empty() {
            clean_name.to_string()
        } else {
            ancestors.reverse();
            ancestors.push(clean_name.to_string());

            let mut tree_repr = String::new();

            for (idx, name) in ancestors.iter().enumerate() {
                if idx == 0 {
                    tree_repr.push_str(name);
                } else {
                    let indent = "   ".repeat(idx.saturating_sub(1));
                    let _ = write!(tree_repr, "\n{indent}`-- {name}");
                }
            }

            tree_repr
        }
    }

    /// The pills of `history` for a job that has `assigned` (RustView's flags for one selected file: an assigned tag is
    /// bold with a check mark).
    pub fn items(
        history: &[u32],
        all_tags_map: &HashMap<u32, TagItem>,
        assigned: &HashSet<u32>,
    ) -> Vec<TagHistoryItem> {
        history
            .iter()
            .filter_map(|&tag_id| {
                let tag = all_tags_map.get(&tag_id)?;
                let name = tag.name.trim().to_string();
                let is_assigned = assigned.contains(&tag_id);
                Some(TagHistoryItem {
                    id: tag_id,
                    width: TagHistoryLayoutEngine::estimate_stable_pill_width(&name, is_assigned),
                    display_text: name.clone(),
                    name,
                    is_assigned,
                    is_partial: false,
                    is_bold: is_assigned,
                    is_italic: false,
                    tooltip: Self::build_hierarchy_tooltip(tag_id, all_tags_map),
                })
            })
            .collect()
    }
}

/// Packs the pills into rows (RustView's flow layout with its text metrics).
pub struct TagHistoryLayoutEngine;

impl TagHistoryLayoutEngine {
    pub const PILL_HEIGHT: f32 = 22.0;
    pub const PILL_HORIZONTAL_PADDING: f32 = 10.0;
    pub const PILL_HORIZONTAL_SPACING: f32 = 4.0;
    pub const ROW_VERTICAL_SPACING: f32 = 4.0;
    /// Room kept for the scroll bar, so its appearance does not change the wrapping.
    pub const SCROLLBAR_GUTTER_RESERVE: f32 = 14.0;
    /// Average width of a bold 11 px character.
    pub const CHAR_WIDTH_METRIC_BOLD: f32 = 5.8;

    /// The pill's width estimated from its text (bold, so it never grows when a tag gets assigned).
    pub fn estimate_stable_pill_width(text: &str, has_icon: bool) -> f32 {
        let char_count = az::cast::<usize, f32>(text.trim().chars().count());
        let icon_width = if has_icon { 14.0 } else { 0.0 };
        let content_w = (char_count * Self::CHAR_WIDTH_METRIC_BOLD) + icon_width;
        (content_w + Self::PILL_HORIZONTAL_PADDING).max(24.0)
    }

    /// Flow-packs the pills into rows of `container_width`; returns them with their total height.
    pub fn pack_into_rows(items: Vec<TagHistoryItem>, container_width: f32) -> (Vec<TagHistoryRow>, f32) {
        if items.is_empty() {
            return (Vec::new(), 0.0);
        }

        let available_w = (container_width - Self::SCROLLBAR_GUTTER_RESERVE).max(80.0);

        let mut rows = Vec::new();
        let mut current_row_items = Vec::new();
        let mut current_row_w = 0.0;

        for item in items {
            let item_w = item.width;
            if !current_row_items.is_empty() && (current_row_w + Self::PILL_HORIZONTAL_SPACING + item_w > available_w) {
                rows.push(TagHistoryRow {
                    items: std::mem::take(&mut current_row_items),
                });
                current_row_w = 0.0;
            }

            if !current_row_items.is_empty() {
                current_row_w += Self::PILL_HORIZONTAL_SPACING;
            }
            current_row_w += item_w;
            current_row_items.push(item);
        }

        if !current_row_items.is_empty() {
            rows.push(TagHistoryRow {
                items: current_row_items,
            });
        }

        let total_rows = rows.len();
        let total_height = if total_rows > 0 {
            (az::cast::<usize, f32>(total_rows) * Self::PILL_HEIGHT)
                + (az::cast::<usize, f32>(total_rows.saturating_sub(1)) * Self::ROW_VERTICAL_SPACING)
                + 4.0
        } else {
            0.0
        };

        (rows, total_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> HashMap<u32, TagItem> {
        [
            (1, None, "Places"),
            (2, Some(1), "Beach"),
            (3, Some(2), " Dunes "),
            (4, None, "Anna"),
        ]
        .into_iter()
        .map(|(id, parent_id, name)| {
            (
                id,
                TagItem {
                    id,
                    parent_id,
                    name: name.into(),
                },
            )
        })
        .collect()
    }

    #[test]
    fn records_most_recent_first_without_duplicates() {
        let mut history = vec![1, 2, 3];
        TagHistoryManager::record_tag_used(&mut history, 3, 3);
        assert_eq!(history, [3, 1, 2]);
        TagHistoryManager::record_tag_used(&mut history, 9, 3);
        assert_eq!(history, [9, 3, 1]);
        TagHistoryManager::prune_invalid(&mut history, &[3, 1].into_iter().collect());
        assert_eq!(history, [3, 1]);
    }

    #[test]
    fn tooltips_show_the_lineage() {
        let tags = map();
        assert_eq!(TagHistoryManager::build_hierarchy_tooltip(4, &tags), "Anna");
        assert_eq!(
            TagHistoryManager::build_hierarchy_tooltip(3, &tags),
            "Places\n`-- Beach\n   `-- Dunes"
        );
        assert_eq!(TagHistoryManager::build_hierarchy_tooltip(99, &tags), "");
    }

    #[test]
    fn items_mark_assigned_tags_and_rows_wrap() {
        let tags = map();
        let items = TagHistoryManager::items(&[3, 99, 4], &tags, &[4].into_iter().collect());
        assert_eq!(
            items.iter().map(|i| i.id).collect::<Vec<_>>(),
            [3, 4],
            "unknown ids are skipped"
        );
        let anna = &items[1];
        assert!(anna.is_assigned && anna.is_bold && !anna.is_italic);
        assert!((anna.width - (4.0 * 5.8 + 14.0 + 10.0)).abs() < 1e-3);
        let (rows, height) = TagHistoryLayoutEngine::pack_into_rows(items.clone(), 1000.0);
        assert_eq!((rows.len(), height), (1, 26.0));
        let (rows, height) = TagHistoryLayoutEngine::pack_into_rows(items, 0.0);
        assert_eq!((rows.len(), height), (2, 52.0), "at least 80 px wide: one pill per row");
        assert!(TagHistoryLayoutEngine::pack_into_rows(Vec::new(), 100.0).0.is_empty());
    }
}
