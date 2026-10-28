//! The tag history's pills in their Slint form: which are faint (the tags the last saved job did not have, so the pills
//! of "Apply the last saved job's tags" stand out, user request 2026-10-27) and where each one stands. The pills are
//! placed here (the flow layout with the widths Slint measured) and kept as stable rows, so a pill that moves (a tag
//! used again goes to the front, also from the last row) glides to its new place instead of jumping (a FLIP-style
//! layout animation done with Slint's property animation, user request 2026-10-27).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use slint::VecModel;

use super::{TagHistoryItem, TagHistoryLayoutEngine};
use crate::TagHistoryItemModel;
use crate::util::model::sync_stable;

/// The pills' model and the widths Slint measured, by tag and whether the pill shows its check (assigned or partial).
#[derive(Default)]
pub struct HistoryPills {
    pub model: Rc<VecModel<TagHistoryItemModel>>,
    widths: RefCell<HashMap<(u32, bool), f32>>,
    /// Each shown pill's index and whether it lies over the others.
    shown: RefCell<HashMap<u32, (usize, bool)>>,
}

impl HistoryPills {
    /// Shows `items` (newest first) in `container` pixels; returns the height they take.
    pub fn show(&self, items: Vec<TagHistoryItem>, last: &LastTags, container: f32) -> f32 {
        let widths = self.widths.borrow();
        let items: Vec<TagHistoryItem> = items
            .into_iter()
            .map(|mut item| {
                if let Some(w) = widths.get(&(item.id, item.is_assigned || item.is_partial)) {
                    item.width = *w;
                }
                item
            })
            .collect();
        drop(widths);
        let (placed, height) = place(items, container);
        let mut shown = self.shown.borrow_mut();
        let rows = placed
            .into_iter()
            .enumerate()
            .map(|(index, (item, x, y))| {
                let front = lies_over(shown.get(&item.id).copied(), index);
                shown.insert(item.id, (index, front));
                let mut view = history_item_view(item, last);
                view.x = x;
                view.y = y;
                view.front = front;
                view
            })
            .collect();
        drop(shown);
        sync_stable(&self.model, rows, |r| r.id);
        height
    }

    /// Slint measured a pill; `true` when its width changed (the pills are placed again).
    pub fn measured(&self, id: u32, check: bool, width: f32) -> bool {
        if !width.is_finite() || width <= 0.0 {
            return false;
        }
        let mut widths = self.widths.borrow_mut();
        let old = widths.insert((id, check), width);
        old.is_none_or(|w| (w - width).abs() > 0.5)
    }
}

/// Whether a pill now at `index` lies over the others: the pills that moved forward (several move together when the
/// history settles) glide over the ones that make room; a pill that kept its place keeps its layer, a new one lies
/// over the others in front.
fn lies_over(before: Option<(usize, bool)>, index: usize) -> bool {
    match before {
        Some((old, front)) if old == index => front,
        Some((old, _)) => index < old,
        None => index == 0,
    }
}

/// The pills flowed into rows of `container` pixels like `TagHistoryLayoutEngine::pack_into_rows`, each with its place
/// (x, y from the top left of the rows); and the rows' height.
pub fn place(items: Vec<TagHistoryItem>, container: f32) -> (Vec<(TagHistoryItem, f32, f32)>, f32) {
    let (rows, height) = TagHistoryLayoutEngine::pack_into_rows(items, container);
    let pitch = TagHistoryLayoutEngine::PILL_HEIGHT + TagHistoryLayoutEngine::ROW_VERTICAL_SPACING;
    let mut placed = Vec::new();
    let mut y = 0.0;
    for row in rows {
        let mut x = 0.0;
        for item in row.items {
            let width = item.width;
            placed.push((item, x, y));
            x += width + TagHistoryLayoutEngine::PILL_HORIZONTAL_SPACING;
        }
        y += pitch;
    }
    (placed, height)
}

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
        x: 0.0,
        y: 0.0,
        front: false,
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
    fn pills_that_move_forward_lie_over_the_others() {
        assert!(lies_over(Some((5, false)), 1), "moved forward");
        assert!(!lies_over(Some((1, true)), 2), "made room");
        assert!(lies_over(Some((0, true)), 0), "kept its place and layer");
        assert!(lies_over(None, 0) && !lies_over(None, 3), "new pills");
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
