//! A list's selection of several rows (click, Ctrl+click, Shift+click, select all), the anchor of a Shift range and
//! the current row (the last one clicked). The queue panel selects jobs with it (the current job is the one the Job
//! and Logs panels show), the Sections card selects sections (P9.5). Only visible rows can be selected; a filter that
//! hides a selected row drops it from the selection (an action never reaches a row the user cannot see).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection<T> {
    /// In the order the rows were selected.
    ids: Vec<T>,
    /// Where a Shift+click range starts.
    anchor: Option<T>,
    /// The row the panels show; stays when the selection is cleared.
    current: Option<T>,
}

impl<T> Default for Selection<T> {
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            anchor: None,
            current: None,
        }
    }
}

impl<T: Copy + PartialEq> Selection<T> {
    /// The selection saved in `ui-state.toml` (ids that are gone are dropped by [`Selection::retain`]).
    pub fn restore(ids: &[T], current: Option<T>) -> Self {
        let mut selection = Self::default();
        for &id in ids {
            if !selection.ids.contains(&id) {
                selection.ids.push(id);
            }
        }
        selection.anchor = selection.ids.last().copied();
        selection.current = current.or(selection.anchor);
        selection
    }

    pub fn ids(&self) -> &[T] {
        &self.ids
    }

    pub fn current(&self) -> Option<T> {
        self.current
    }

    pub fn contains(&self, id: T) -> bool {
        self.ids.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// The selected rows in the order of `order` (the list's order).
    pub fn in_order(&self, order: &[T]) -> Vec<T> {
        order.iter().copied().filter(|id| self.ids.contains(id)).collect()
    }

    /// A plain click: only this row. A click on the only selected row keeps it (no toggling by accident).
    pub fn click(&mut self, id: T) {
        self.ids = vec![id];
        self.anchor = Some(id);
        self.current = Some(id);
    }

    /// Ctrl+click: this row in or out; the anchor moves to it.
    pub fn toggle(&mut self, id: T) {
        if let Some(at) = self.ids.iter().position(|i| *i == id) {
            self.ids.remove(at);
        } else {
            self.ids.push(id);
        }
        self.anchor = Some(id);
        self.current = Some(id);
    }

    /// Shift+click: everything between the anchor and this row in `visible` (the rows as shown); with Ctrl the range
    /// is added to the selection. Without an anchor it is a plain click.
    pub fn extend(&mut self, id: T, visible: &[T], add: bool) {
        let from = self.anchor.and_then(|a| visible.iter().position(|v| *v == a));
        let to = visible.iter().position(|v| *v == id);
        let (Some(from), Some(to)) = (from, to) else {
            self.click(id);
            return;
        };
        let (low, high) = (from.min(to), from.max(to));
        let range = visible.get(low..=high).unwrap_or_default();
        if !add {
            self.ids.clear();
        }
        for &row in range {
            if !self.ids.contains(&row) {
                self.ids.push(row);
            }
        }
        self.current = Some(id);
    }

    /// A click with the keys held: Shift a range (with Ctrl added to the selection), Ctrl one row in or out, else
    /// only this row.
    pub fn pick(&mut self, id: T, visible: &[T], control: bool, shift: bool) {
        match (control, shift) {
            (_, true) => self.extend(id, visible, control),
            (true, false) => self.toggle(id),
            (false, false) => self.click(id),
        }
    }

    /// Ctrl+A: every visible row.
    pub fn select_all(&mut self, visible: &[T]) {
        self.ids = visible.to_vec();
        if self.current.is_none_or(|c| !visible.contains(&c)) {
            self.current = visible.first().copied();
        }
        self.anchor = self.current;
    }

    /// Exactly these rows; `current` becomes the current one (the anchor of the next Shift range).
    pub fn set(&mut self, ids: Vec<T>, current: Option<T>) {
        self.ids = ids;
        self.current = current;
        self.anchor = current;
    }

    /// Nothing selected; the Job panel keeps its job.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    /// Keeps only rows that `keep` accepts (rows that are gone or hidden by a filter). The current row stays while it
    /// exists (`exists`), so the Job panel does not go blank when a filter hides it.
    pub fn retain(&mut self, keep: impl Fn(T) -> bool, exists: impl Fn(T) -> bool) {
        self.ids.retain(|id| keep(*id));
        if self.anchor.is_some_and(|a| !keep(a)) {
            self.anchor = None;
        }
        if self.current.is_some_and(|c| !exists(c)) {
            self.current = None;
        }
    }

    /// The row the panels show changes without touching the selection (a job loaded into the editor).
    pub fn set_current(&mut self, id: Option<T>) {
        self.current = id;
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::job::JobId;

    use super::*;

    fn ids(v: &[i64]) -> Vec<JobId> {
        v.iter().map(|i| JobId(*i)).collect()
    }

    #[test]
    fn clicks_ranges_and_filters() {
        let visible = ids(&[1, 2, 3, 4, 5]);
        let mut s = Selection::default();
        s.click(JobId(2));
        s.extend(JobId(4), &visible, false);
        assert_eq!(s.ids(), ids(&[2, 3, 4]).as_slice());
        assert_eq!(s.current(), Some(JobId(4)));
        s.toggle(JobId(3));
        assert_eq!(s.ids(), ids(&[2, 4]).as_slice());
        s.toggle(JobId(1));
        assert_eq!(s.in_order(&visible), ids(&[1, 2, 4]), "actions run in queue order");
        // The anchor moved to 1: Shift+click on 2 replaces the selection, Ctrl+Shift adds the range.
        s.extend(JobId(2), &visible, false);
        assert_eq!(s.ids(), ids(&[1, 2]).as_slice());
        s.click(JobId(5));
        s.extend(JobId(4), &visible, true);
        assert_eq!(s.ids(), ids(&[5, 4]).as_slice());

        s.select_all(&visible);
        assert_eq!(s.len(), 5);
        // A filter hides 2 and 4; job 4 (current) still exists, so the Job panel keeps it.
        s.retain(|id| id.0 % 2 == 1, |_| true);
        assert_eq!(s.ids(), ids(&[1, 3, 5]).as_slice());
        assert_eq!(s.current(), Some(JobId(4)));
        s.retain(|_| true, |id| id != JobId(4));
        assert_eq!(s.current(), None, "a removed job is no longer shown");

        s.clear();
        assert!(s.is_empty());
        s.extend(JobId(3), &visible, false);
        assert_eq!(
            s.ids(),
            ids(&[3]).as_slice(),
            "without an anchor Shift+click is a click"
        );
    }

    #[test]
    fn restored_selection() {
        let s = Selection::restore(&ids(&[4, 4, 9]), None);
        assert_eq!((s.ids(), s.current()), (ids(&[4, 9]).as_slice(), Some(JobId(9))));
        let s = Selection::restore(&[], Some(JobId(2)));
        assert!(s.is_empty());
        assert_eq!(s.current(), Some(JobId(2)));
    }

    #[test]
    fn picks_follow_the_keys() {
        let visible = [1, 2, 3, 4];
        let mut s = Selection::default();
        s.pick(2, &visible, false, false);
        s.pick(4, &visible, false, true);
        assert_eq!(s.ids(), [2, 3, 4]);
        s.pick(3, &visible, true, false);
        assert_eq!(s.ids(), [2, 4]);
        s.pick(1, &visible, true, true);
        assert_eq!(
            s.in_order(&visible),
            [1, 2, 3, 4],
            "Ctrl+Shift adds the range from the anchor"
        );
        s.set(vec![4], Some(4));
        assert_eq!((s.ids(), s.current()), ([4].as_slice(), Some(4)));
        s.pick(2, &visible, false, true);
        assert_eq!(s.in_order(&visible), [2, 3, 4], "the anchor moved with `set`");
    }
}
