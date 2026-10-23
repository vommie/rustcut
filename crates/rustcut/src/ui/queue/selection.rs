//! The queue panel's selection: several jobs (click, Ctrl+click, Shift+click, select all), the anchor of a Shift range
//! and the current job that the Job and Logs panels show (the last one clicked). Only visible rows can be selected;
//! a filter that hides a selected job drops it from the selection (an action never reaches a job the user cannot see).

use rustcut_core::job::JobId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// In the order the jobs were selected.
    ids: Vec<JobId>,
    /// Where a Shift+click range starts.
    anchor: Option<JobId>,
    /// The job the Job and Logs panels show; stays when the selection is cleared.
    current: Option<JobId>,
}

impl Selection {
    /// The selection saved in `ui-state.toml` (ids that are gone are dropped by [`Selection::retain`]).
    pub fn restore(ids: &[JobId], current: Option<JobId>) -> Self {
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

    pub fn ids(&self) -> &[JobId] {
        &self.ids
    }

    pub fn current(&self) -> Option<JobId> {
        self.current
    }

    pub fn contains(&self, id: JobId) -> bool {
        self.ids.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// The selected jobs in the order of `order` (the queue's order).
    pub fn in_order(&self, order: &[JobId]) -> Vec<JobId> {
        order.iter().copied().filter(|id| self.ids.contains(id)).collect()
    }

    /// A plain click: only this job. A click on the only selected job keeps it (no toggling by accident).
    pub fn click(&mut self, id: JobId) {
        self.ids = vec![id];
        self.anchor = Some(id);
        self.current = Some(id);
    }

    /// Ctrl+click: this job in or out; the anchor moves to it.
    pub fn toggle(&mut self, id: JobId) {
        if let Some(at) = self.ids.iter().position(|i| *i == id) {
            self.ids.remove(at);
        } else {
            self.ids.push(id);
        }
        self.anchor = Some(id);
        self.current = Some(id);
    }

    /// Shift+click: everything between the anchor and this job in `visible` (the rows as shown); with Ctrl the range
    /// is added to the selection. Without an anchor it is a plain click.
    pub fn extend(&mut self, id: JobId, visible: &[JobId], add: bool) {
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
        for &job in range {
            if !self.ids.contains(&job) {
                self.ids.push(job);
            }
        }
        self.current = Some(id);
    }

    /// Ctrl+A: every visible job.
    pub fn select_all(&mut self, visible: &[JobId]) {
        self.ids = visible.to_vec();
        if self.current.is_none_or(|c| !visible.contains(&c)) {
            self.current = visible.first().copied();
        }
        self.anchor = self.current;
    }

    /// Nothing selected; the Job panel keeps its job.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    /// Keeps only jobs that `keep` accepts (jobs that are gone or hidden by a filter). The current job stays while it
    /// exists (`exists`), so the Job panel does not go blank when a filter hides it.
    pub fn retain(&mut self, keep: impl Fn(JobId) -> bool, exists: impl Fn(JobId) -> bool) {
        self.ids.retain(|id| keep(*id));
        if self.anchor.is_some_and(|a| !keep(a)) {
            self.anchor = None;
        }
        if self.current.is_some_and(|c| !exists(c)) {
            self.current = None;
        }
    }

    /// The job the panels show changes without touching the selection (a job loaded into the editor).
    pub fn set_current(&mut self, id: Option<JobId>) {
        self.current = id;
    }
}

#[cfg(test)]
mod tests {
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
}
