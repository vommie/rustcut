//! What acts on several sections at once (P9.5, the review's notes 07): the selected rows are deleted, moved one place,
//! dropped somewhere else (drag & drop) or duplicated as one block in list order; a single row is deleted by its own
//! button; a section's body dragged on the timeline moves it whole.

use rustcut_core::edit::{Section, SectionId};
use rustcut_core::time::Timestamp;

use super::SectionsState;

impl SectionsState {
    /// The selected sections in list order.
    pub fn selected_ids(&self) -> Vec<SectionId> {
        self.selection.in_order(&self.order())
    }

    /// Every section selected (the current one stays current).
    pub fn select_all(&mut self) {
        let order = self.order();
        self.selection.select_all(&order);
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Deletes the selected sections; the row above the first of them is selected next (PyCutRv), else the one that
    /// took its place.
    pub fn delete_selected(&mut self) -> bool {
        let ids = self.selected_ids();
        let Some(first) = ids.first().and_then(|id| self.list.index_of(*id)) else {
            return false;
        };
        for id in &ids {
            self.list.remove(*id);
        }
        let next = first
            .checked_sub(1)
            .and_then(|i| self.id_at(i))
            .or_else(|| self.id_at(first));
        self.select(next);
        true
    }

    /// Deletes one section (its row's button); the rest of the selection stays.
    pub fn delete(&mut self, id: SectionId) -> bool {
        let gone = self.list.remove(id).is_some();
        self.forget_gone();
        gone
    }

    /// Whether the selected sections can move one place up (or down): one of them has an unselected row there.
    pub fn can_move(&self, up: bool) -> bool {
        let order = self.order();
        stepped(&order, &self.selected_ids(), up) != order
    }

    /// Moves the selected sections one place up or down; a selected row at the top (bottom) stays and the ones behind
    /// it close up.
    pub fn move_selected(&mut self, up: bool) -> bool {
        let order = self.order();
        let moved = stepped(&order, &self.selected_ids(), up);
        self.apply_order(&order, &moved)
    }

    /// Drops the selected sections before the row `before` of the current order (the row count = the end), as one
    /// block in their order.
    pub fn drop_selected(&mut self, before: usize) -> bool {
        let order = self.order();
        let dragged = self.selected_ids();
        let place = order.iter().take(before).filter(|id| !dragged.contains(id)).count();
        let mut moved: Vec<SectionId> = order.iter().copied().filter(|id| !dragged.contains(id)).collect();
        let tail = moved.split_off(place.min(moved.len()));
        moved.extend(dragged);
        moved.extend(tail);
        self.apply_order(&order, &moved)
    }

    /// Copies the selected sections and inserts the copies after the last of them, in their order; the copies are
    /// selected. Returns their row indices.
    pub fn duplicate_selected(&mut self) -> Vec<usize> {
        let ids = self.selected_ids();
        let Some(last) = ids.last().and_then(|id| self.list.index_of(*id)) else {
            return Vec::new();
        };
        let sections: Vec<Section> = ids.iter().filter_map(|id| self.list.get(*id)).collect();
        let copies: Vec<SectionId> = sections
            .into_iter()
            .enumerate()
            .map(|(k, section)| self.list.insert(last + 1 + k, section))
            .collect();
        let current = copies.last().copied();
        self.selection.set(copies.clone(), current);
        copies.iter().filter_map(|id| self.list.index_of(*id)).collect()
    }

    /// Moves a section whole so it starts at `start` (its length stays; it stays inside the video).
    pub fn move_section(&mut self, id: SectionId, start: Timestamp) {
        let Some(section) = self.list.get(id) else { return };
        let length = section.duration();
        let latest = self.duration().map(|d| (d - length).max(Timestamp::ZERO));
        let start = latest.map_or(start, |l| start.min(l)).max(Timestamp::ZERO);
        if let Ok(moved) = Section::new(start, start + length) {
            self.list.replace(id, moved);
        }
    }

    fn order(&self) -> Vec<SectionId> {
        self.list.ids().collect()
    }

    /// Brings the list from `order` into `moved` (the same ids); `false` when nothing changes.
    fn apply_order(&mut self, order: &[SectionId], moved: &[SectionId]) -> bool {
        if order == moved {
            return false;
        }
        for (index, id) in moved.iter().enumerate() {
            self.list.move_to(*id, index);
        }
        true
    }
}

/// `order` with every row of `selected` one place up (or down), unless an unmovable selected row blocks it.
fn stepped(order: &[SectionId], selected: &[SectionId], up: bool) -> Vec<SectionId> {
    let mut rows: Vec<SectionId> = order.to_vec();
    if !up {
        rows.reverse();
    }
    // Selected rows at the front stay; every later selected row swaps with the unselected one before it.
    let mut blocked = 0;
    for i in 0..rows.len() {
        let Some(id) = rows.get(i).copied() else { break };
        if !selected.contains(&id) {
            continue;
        }
        if i == blocked {
            blocked += 1;
        } else if let Some(before) = i.checked_sub(1) {
            rows.swap(before, i);
        }
    }
    if !up {
        rows.reverse();
    }
    rows
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::ui::sections::Bound;

    fn t(secs: i64) -> Timestamp {
        Timestamp::from_secs(secs)
    }

    /// Five sections 0-1, 10-11, 20-21, 30-31, 40-41 in a 100 s video.
    fn five() -> SectionsState {
        let mut s = SectionsState::default();
        s.set_duration(Some(t(100)));
        for i in 0..5 {
            s.set_mark(Bound::Start, t(i * 10));
            s.set_mark(Bound::End, t(i * 10 + 1));
            s.add_current().unwrap();
        }
        s
    }

    fn starts(s: &SectionsState) -> Vec<i64> {
        s.list().sections().map(|x| x.start().as_micros() / 1_000_000).collect()
    }

    fn select(s: &mut SectionsState, rows: &[usize]) {
        s.clear_selection();
        for &row in rows {
            s.pick(row, true, false);
        }
    }

    fn selected_starts(s: &SectionsState) -> Vec<i64> {
        s.selected_ids()
            .iter()
            .filter_map(|id| s.list().get(*id))
            .map(|x| x.start().as_micros() / 1_000_000)
            .collect()
    }

    #[test]
    fn several_rows_move_as_one_block() {
        let mut s = five();
        select(&mut s, &[1, 3]);
        assert!(s.can_move(true) && s.can_move(false));
        assert!(s.move_selected(true));
        assert_eq!(starts(&s), [10, 0, 30, 20, 40]);
        assert!(s.move_selected(true));
        assert_eq!(
            starts(&s),
            [10, 30, 0, 20, 40],
            "the first one stays at the top, the second closes up"
        );
        assert!(!s.can_move(true));
        assert!(!s.move_selected(true));
        assert!(s.move_selected(false));
        assert_eq!(starts(&s), [0, 10, 30, 20, 40]);
        assert_eq!(selected_starts(&s), [10, 30], "the selection moves along");
    }

    #[test]
    fn dropping_puts_the_block_before_a_row() {
        let mut s = five();
        select(&mut s, &[0, 2]);
        assert!(s.drop_selected(4));
        assert_eq!(starts(&s), [10, 30, 0, 20, 40]);
        assert!(s.drop_selected(5));
        assert_eq!(starts(&s), [10, 30, 40, 0, 20]);
        assert!(s.drop_selected(0));
        assert_eq!(starts(&s), [0, 20, 10, 30, 40]);
        assert!(!s.drop_selected(1), "inside the block: nothing changes");
        assert!(!s.drop_selected(0));
        s.clear_selection();
        assert!(!s.drop_selected(3), "nothing selected");
    }

    #[test]
    fn duplicates_follow_the_last_selected_row() {
        let mut s = five();
        select(&mut s, &[3, 1]);
        assert_eq!(s.duplicate_selected(), [4, 5]);
        assert_eq!(starts(&s), [0, 10, 20, 30, 10, 30, 40]);
        assert_eq!(selected_starts(&s), [10, 30], "the copies are selected");
        assert_eq!(s.rows().iter().filter(|r| r.overlapping).count(), 4);
        s.clear_selection();
        assert!(s.duplicate_selected().is_empty());
    }

    #[test]
    fn deleting_several_and_single_rows() {
        let mut s = five();
        select(&mut s, &[1, 2]);
        assert!(s.delete_selected());
        assert_eq!(starts(&s), [0, 30, 40]);
        assert_eq!(selected_starts(&s), [0], "the row above the first deleted one");
        select(&mut s, &[0, 2]);
        let middle = s.id_at(1).unwrap();
        assert!(s.delete(middle));
        assert_eq!(starts(&s), [0, 40]);
        assert_eq!(selected_starts(&s), [0, 40], "the selection stays");
        assert!(!s.delete(middle));
        s.select_all();
        assert!(s.delete_selected());
        assert!(s.list().is_empty() && s.selected().is_none());
    }

    #[test]
    fn a_moved_body_keeps_its_length_inside_the_video() {
        let mut s = five();
        let id = s.id_at(1).unwrap();
        s.move_section(id, t(50));
        assert_eq!(s.list().get(id), Section::new(t(50), t(51)).ok());
        s.move_section(id, t(-5));
        assert_eq!(s.list().get(id), Section::new(t(0), t(1)).ok());
        s.move_section(id, t(500));
        assert_eq!(s.list().get(id), Section::new(t(99), t(100)).ok());
    }

    proptest! {
        /// Whatever is moved or dropped, the list keeps every section once and the selection stays the same set.
        #[test]
        fn arranging_keeps_every_section(
            rows in prop::collection::vec(0usize..6, 0..4),
            steps in prop::collection::vec((0u8..3, 0usize..7), 0..12),
        ) {
            let mut s = five();
            select(&mut s, &rows);
            let mut before: Vec<Section> = s.list().sections().collect();
            before.sort_by_key(|x| x.start());
            let chosen = selected_starts(&s).len();
            for (kind, at) in steps {
                match kind {
                    0 => { s.move_selected(true); }
                    1 => { s.move_selected(false); }
                    _ => { s.drop_selected(at); }
                }
            }
            let mut after: Vec<Section> = s.list().sections().collect();
            after.sort_by_key(|x| x.start());
            prop_assert_eq!(before, after);
            prop_assert_eq!(selected_starts(&s).len(), chosen);
        }
    }
}
