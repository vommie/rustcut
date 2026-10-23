//! One section of the source to keep: `[start, end)` in microseconds.

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SectionError {
    #[error("a section needs a start before its end (both are {0})")]
    Empty(Timestamp),
    #[error("a section cannot start before the beginning of the video ({0})")]
    Negative(Timestamp),
}

/// A part of the source that goes into the output. `start < end` always holds; the constructor swaps reversed bounds (PyCutRv:
/// "end before start is corrected").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawSection", into = "RawSection")]
pub struct Section {
    start: Timestamp,
    end: Timestamp,
}

impl Section {
    pub fn new(a: Timestamp, b: Timestamp) -> Result<Self, SectionError> {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if start == end {
            return Err(SectionError::Empty(start));
        }
        if start.is_negative() {
            return Err(SectionError::Negative(start));
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> Timestamp {
        self.start
    }

    pub const fn end(self) -> Timestamp {
        self.end
    }

    pub fn duration(self) -> Timestamp {
        self.end - self.start
    }

    /// Whether `t` lies inside `[start, end)`.
    pub fn contains(self, t: Timestamp) -> bool {
        self.start <= t && t < self.end
    }

    /// Whether the two sections share at least one microsecond (touching ends do not overlap).
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// The section limited to `[0, duration)`; `None` when nothing of it is left.
    pub fn clamped_to(self, duration: Timestamp) -> Option<Self> {
        Self::new(self.start.min(duration), self.end.min(duration)).ok()
    }

    /// A copy with a new start (the end stays); reversed bounds are swapped like in [`Section::new`].
    pub fn with_start(self, start: Timestamp) -> Result<Self, SectionError> {
        Self::new(start, self.end)
    }

    /// A copy with a new end (the start stays); reversed bounds are swapped like in [`Section::new`].
    pub fn with_end(self, end: Timestamp) -> Result<Self, SectionError> {
        Self::new(self.start, end)
    }
}

/// Serialized form; deserializing goes through [`Section::new`], so a hand-edited or old row cannot break the invariant.
#[derive(Serialize, Deserialize)]
struct RawSection {
    start: Timestamp,
    end: Timestamp,
}

impl TryFrom<RawSection> for Section {
    type Error = SectionError;

    fn try_from(raw: RawSection) -> Result<Self, Self::Error> {
        Self::new(raw.start, raw.end)
    }
}

impl From<Section> for RawSection {
    fn from(s: Section) -> Self {
        Self {
            start: s.start,
            end: s.end,
        }
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
    fn reversed_bounds_are_swapped() {
        assert_eq!(s(5, 2), s(2, 5));
        assert_eq!(s(2, 5).duration(), Timestamp::from_secs(3));
    }

    #[test]
    fn empty_and_negative_sections_are_refused() {
        let t = Timestamp::from_secs(1);
        assert_eq!(Section::new(t, t), Err(SectionError::Empty(t)));
        let neg = Timestamp::from_secs(-1);
        assert_eq!(Section::new(neg, t), Err(SectionError::Negative(neg)));
    }

    #[test]
    fn overlap_and_contains() {
        assert!(s(0, 5).overlaps(s(4, 8)));
        assert!(!s(0, 5).overlaps(s(5, 8)));
        assert!(s(0, 5).contains(Timestamp::ZERO));
        assert!(!s(0, 5).contains(Timestamp::from_secs(5)));
    }

    #[test]
    fn clamping_to_the_duration() {
        let d = Timestamp::from_secs(10);
        assert_eq!(s(8, 12).clamped_to(d), Some(s(8, 10)));
        assert_eq!(s(10, 12).clamped_to(d), None);
    }

    #[test]
    fn deserializing_checks_the_invariant() {
        let ok: Section = serde_json::from_str(r#"{"start":3000000,"end":1000000}"#).unwrap();
        assert_eq!(ok, s(1, 3));
        assert!(serde_json::from_str::<Section>(r#"{"start":1,"end":1}"#).is_err());
        assert_eq!(
            serde_json::to_string(&s(1, 3)).unwrap(),
            r#"{"start":1000000,"end":3000000}"#
        );
    }

    proptest! {
        #[test]
        fn constructed_sections_are_ordered(a in 0i64..1_000_000_000_000, b in 0i64..1_000_000_000_000) {
            prop_assume!(a != b);
            let sec = Section::new(Timestamp::from_micros(a), Timestamp::from_micros(b)).unwrap();
            prop_assert!(sec.start() < sec.end());
            prop_assert_eq!(sec.duration(), Timestamp::from_micros(a.abs_diff(b).try_into().unwrap()));
        }
    }
}
