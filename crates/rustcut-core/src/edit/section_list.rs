//! The ordered list of sections of a job. List order is output order (reorderable, ADR-8); sorting by time is an explicit
//! action, and overlaps are reported, not forbidden.

use serde::{Deserialize, Serialize};

use super::Section;
use crate::time::Timestamp;

/// Identity of a section inside one [`SectionList`], stable while the list lives (UI rows are updated in place by id).
/// Ids are not persisted: a list read back from JSON numbers its sections again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SectionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    id: SectionId,
    section: Section,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<Section>", into = "Vec<Section>")]
pub struct SectionList {
    entries: Vec<Entry>,
    next_id: u64,
}

impl SectionList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (SectionId, Section)> + '_ {
        self.entries.iter().map(|e| (e.id, e.section))
    }

    pub fn sections(&self) -> impl ExactSizeIterator<Item = Section> + '_ {
        self.entries.iter().map(|e| e.section)
    }

    pub fn ids(&self) -> impl ExactSizeIterator<Item = SectionId> + '_ {
        self.entries.iter().map(|e| e.id)
    }

    pub fn get(&self, id: SectionId) -> Option<Section> {
        self.entries.iter().find(|e| e.id == id).map(|e| e.section)
    }

    pub fn index_of(&self, id: SectionId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// Appends a section and returns its id.
    pub fn push(&mut self, section: Section) -> SectionId {
        let id = self.allocate_id();
        self.entries.push(Entry { id, section });
        id
    }

    /// Inserts a section at `index` (clamped to the end) and returns its id.
    pub fn insert(&mut self, index: usize, section: Section) -> SectionId {
        let id = self.allocate_id();
        self.entries
            .insert(index.min(self.entries.len()), Entry { id, section });
        id
    }

    pub fn remove(&mut self, id: SectionId) -> Option<Section> {
        let index = self.index_of(id)?;
        Some(self.entries.remove(index).section)
    }

    /// Replaces the bounds of a section; `false` when the id is unknown.
    pub fn replace(&mut self, id: SectionId, section: Section) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => {
                entry.section = section;
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Moves a section to `index` (clamped); `false` when the id is unknown.
    pub fn move_to(&mut self, id: SectionId, index: usize) -> bool {
        let Some(from) = self.index_of(id) else { return false };
        let entry = self.entries.remove(from);
        self.entries.insert(index.min(self.entries.len()), entry);
        true
    }

    /// Moves a section one place up; `false` when it is unknown or already first.
    pub fn move_up(&mut self, id: SectionId) -> bool {
        match self.index_of(id) {
            Some(i) if i > 0 => {
                self.entries.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves a section one place down; `false` when it is unknown or already last.
    pub fn move_down(&mut self, id: SectionId) -> bool {
        match self.index_of(id) {
            Some(i) if i + 1 < self.entries.len() => {
                self.entries.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// Sorts by start time (then end), keeping ids. Stable: equal sections keep their order.
    pub fn sort_by_time(&mut self) {
        self.entries.sort_by_key(|e| (e.section.start(), e.section.end()));
    }

    /// Length of the output: the sum of all section durations (overlaps count twice, as they are rendered twice).
    pub fn total_duration(&self) -> Timestamp {
        self.sections().fold(Timestamp::ZERO, |acc, s| acc + s.duration())
    }

    /// Every pair of sections that overlap, in list order.
    pub fn overlapping_pairs(&self) -> Vec<(SectionId, SectionId)> {
        let mut pairs = Vec::new();
        for (i, a) in self.entries.iter().enumerate() {
            for b in self.entries.iter().skip(i + 1) {
                if a.section.overlaps(b.section) {
                    pairs.push((a.id, b.id));
                }
            }
        }
        pairs
    }

    /// Limits every section to `[0, duration)` and drops sections that start at or after it. Returns how many were dropped.
    pub fn clamp_to(&mut self, duration: Timestamp) -> usize {
        let before = self.entries.len();
        self.entries.retain_mut(|e| match e.section.clamped_to(duration) {
            Some(s) => {
                e.section = s;
                true
            }
            None => false,
        });
        before - self.entries.len()
    }

    fn allocate_id(&mut self) -> SectionId {
        let id = SectionId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

impl From<Vec<Section>> for SectionList {
    fn from(sections: Vec<Section>) -> Self {
        let mut list = Self::new();
        for s in sections {
            list.push(s);
        }
        list
    }
}

impl From<SectionList> for Vec<Section> {
    fn from(list: SectionList) -> Self {
        list.sections().collect()
    }
}

impl FromIterator<Section> for SectionList {
    fn from_iter<I: IntoIterator<Item = Section>>(iter: I) -> Self {
        let mut list = Self::new();
        for s in iter {
            list.push(s);
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn s(a: i64, b: i64) -> Section {
        Section::new(Timestamp::from_secs(a), Timestamp::from_secs(b)).unwrap()
    }

    #[test]
    fn ids_stay_with_their_sections() {
        let mut list = SectionList::new();
        let a = list.push(s(0, 1));
        let b = list.push(s(5, 6));
        let c = list.insert(0, s(2, 3));
        assert_eq!(list.ids().collect::<Vec<_>>(), vec![c, a, b]);
        assert!(list.move_down(c));
        assert_eq!(list.ids().collect::<Vec<_>>(), vec![a, c, b]);
        assert!(!list.move_up(a));
        assert!(!list.move_down(b));
        assert!(list.move_to(b, 0));
        assert_eq!(list.get(b), Some(s(5, 6)));
        assert_eq!(list.index_of(b), Some(0));
        assert_eq!(list.remove(c), Some(s(2, 3)));
        assert_eq!(list.remove(c), None);
        let d = list.push(s(9, 10));
        assert_ne!(d, c, "ids are never reused");
    }

    #[test]
    fn sorting_and_totals() {
        let mut list: SectionList = vec![s(10, 12), s(0, 5), s(3, 4)].into();
        assert_eq!(list.total_duration(), Timestamp::from_secs(8));
        let overlapping = list.overlapping_pairs();
        assert_eq!(overlapping.len(), 1);
        list.sort_by_time();
        assert_eq!(list.sections().collect::<Vec<_>>(), vec![s(0, 5), s(3, 4), s(10, 12)]);
    }

    #[test]
    fn clamping_drops_what_is_past_the_end() {
        let mut list: SectionList = vec![s(0, 5), s(8, 12), s(20, 30)].into();
        assert_eq!(list.clamp_to(Timestamp::from_secs(10)), 1);
        assert_eq!(list.sections().collect::<Vec<_>>(), vec![s(0, 5), s(8, 10)]);
    }

    #[test]
    fn serializes_as_a_plain_list() {
        let list: SectionList = vec![s(0, 1)].into();
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(json, r#"[{"start":0,"end":1000000}]"#);
        assert_eq!(serde_json::from_str::<SectionList>(&json).unwrap().len(), 1);
    }

    fn arb_list() -> impl Strategy<Value = Vec<(i64, i64)>> {
        prop::collection::vec((0i64..1_000, 1i64..100), 0..12)
    }

    #[derive(Debug, Clone)]
    enum Op {
        Up(usize),
        Down(usize),
        MoveTo(usize, usize),
        Sort,
    }

    fn arb_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..12).prop_map(Op::Up),
            (0usize..12).prop_map(Op::Down),
            (0usize..12, 0usize..14).prop_map(|(a, b)| Op::MoveTo(a, b)),
            Just(Op::Sort),
        ]
    }

    proptest! {
        #[test]
        fn reordering_keeps_every_section_once(raw in arb_list(), ops in prop::collection::vec(arb_op(), 0..20)) {
            let mut list: SectionList = raw.iter().map(|&(a, len)| s(a, a + len)).collect();
            let mut before: Vec<_> = list.iter().collect();
            for op in ops {
                let ids: Vec<_> = list.ids().collect();
                match op {
                    Op::Up(i) => { if let Some(&id) = ids.get(i) { list.move_up(id); } }
                    Op::Down(i) => { if let Some(&id) = ids.get(i) { list.move_down(id); } }
                    Op::MoveTo(i, to) => { if let Some(&id) = ids.get(i) { list.move_to(id, to); } }
                    Op::Sort => list.sort_by_time(),
                }
            }
            let mut after: Vec<_> = list.iter().collect();
            before.sort_by_key(|(id, _)| *id);
            after.sort_by_key(|(id, _)| *id);
            prop_assert_eq!(before, after);
        }

        #[test]
        fn sorted_lists_are_ordered(raw in arb_list()) {
            let mut list: SectionList = raw.iter().map(|&(a, len)| s(a, a + len)).collect();
            list.sort_by_time();
            let starts: Vec<_> = list.sections().map(Section::start).collect();
            prop_assert!(starts.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
