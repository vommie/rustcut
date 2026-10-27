//! A tag history pill in its Slint form, and which pills are faint: the tags the last saved job did not have, so the
//! pills of "Apply the last saved job's tags" stand out (user request 2026-10-27).

use std::collections::HashSet;

use super::TagHistoryItem;
use crate::TagHistoryItemModel;

/// The tags of the last saved job; a pill outside them is faint unless it is assigned now. Before any job was saved
/// nothing is faint.
#[derive(Debug, Default)]
pub struct LastTags(HashSet<u32>);

impl LastTags {
    pub fn new(last: &[u32]) -> Self {
        Self(last.iter().copied().collect())
    }

    pub fn older(&self, item: &TagHistoryItem) -> bool {
        !self.0.is_empty() && !self.0.contains(&item.id) && !item.is_assigned && !item.is_partial
    }
}

pub fn history_item_view(item: TagHistoryItem, last: &LastTags) -> TagHistoryItemModel {
    let older = last.older(&item);
    TagHistoryItemModel {
        id: i32::try_from(item.id).unwrap_or(i32::MAX),
        name: item.name.into(),
        display_text: item.display_text.into(),
        is_assigned: item.is_assigned,
        is_partial: item.is_partial,
        is_bold: item.is_bold,
        is_italic: item.is_italic,
        width: item.width,
        tooltip: item.tooltip.into(),
        older,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32, assigned: bool) -> TagHistoryItem {
        TagHistoryItem {
            id,
            name: String::new(),
            display_text: String::new(),
            is_assigned: assigned,
            is_partial: false,
            is_bold: false,
            is_italic: false,
            width: 40.0,
            tooltip: String::new(),
        }
    }

    #[test]
    fn tags_outside_the_last_saved_job_are_faint() {
        let last = LastTags::new(&[2, 5]);
        assert!(!last.older(&item(2, false)), "the last job's tag");
        assert!(last.older(&item(7, false)), "used before the last job");
        assert!(!last.older(&item(7, true)), "assigned now");
        assert!(
            !LastTags::new(&[]).older(&item(7, false)),
            "no job saved yet: nothing faint"
        );
    }
}
