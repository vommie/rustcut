//! The thumbnails of one video in memory, as stored (WebP): a thousand of them take a few MB. The hover decodes the one it
//! shows, and every transformation of crop or rotation stays with that one picture, so the stored ones never change.

use std::collections::BTreeMap;
use std::sync::Arc;

/// One thumbnail. `time_ms` is where the picture really is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub time_ms: u64,
    pub exact: bool,
    pub width: u32,
    pub height: u32,
    pub webp: Arc<[u8]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThumbSet {
    by_time: BTreeMap<u64, Thumb>,
}

impl ThumbSet {
    /// True when the set changed. An exact picture is not replaced by a keyframe at the same time.
    pub fn insert(&mut self, thumb: Thumb) -> bool {
        match self.by_time.get(&thumb.time_ms) {
            Some(known) if known.exact && !thumb.exact => false,
            _ => {
                self.by_time.insert(thumb.time_ms, thumb);
                true
            }
        }
    }

    pub fn len(&self) -> usize {
        self.by_time.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_time.is_empty()
    }

    pub fn nearest(&self, ms: u64) -> Option<&Thumb> {
        let before = self.by_time.range(..=ms).next_back().map(|(_, thumb)| thumb);
        let after = self.by_time.range(ms..).next().map(|(_, thumb)| thumb);
        match (before, after) {
            (Some(a), Some(b)) => Some(if ms.abs_diff(a.time_ms) <= ms.abs_diff(b.time_ms) {
                a
            } else {
                b
            }),
            (one, other) => one.or(other),
        }
    }

    /// Distance to the nearest picture, `None` without any.
    pub fn gap(&self, ms: u64) -> Option<u64> {
        self.nearest(ms).map(|thumb| ms.abs_diff(thumb.time_ms))
    }

    /// The last picture at or before the time.
    pub fn floor(&self, ms: u64) -> Option<u64> {
        self.by_time.range(..=ms).next_back().map(|(&time, _)| time)
    }

    pub fn times(&self) -> impl Iterator<Item = u64> + '_ {
        self.by_time.keys().copied()
    }

    pub fn thumbs(&self) -> impl Iterator<Item = &Thumb> {
        self.by_time.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb(time_ms: u64, exact: bool) -> Thumb {
        Thumb {
            time_ms,
            exact,
            width: 320,
            height: 180,
            webp: Arc::from(&b"x"[..]),
        }
    }

    #[test]
    fn nearest_looks_both_ways() {
        let mut set = ThumbSet::default();
        assert!(set.nearest(5).is_none());
        set.insert(thumb(100, false));
        set.insert(thumb(200, false));
        assert_eq!(set.nearest(120).unwrap().time_ms, 100);
        assert_eq!(set.nearest(180).unwrap().time_ms, 200);
        assert_eq!(set.nearest(5000).unwrap().time_ms, 200);
        assert_eq!(set.gap(5000), Some(4800));
        assert_eq!(set.floor(150), Some(100));
        assert_eq!(set.floor(50), None);
    }

    #[test]
    fn a_keyframe_does_not_replace_an_exact_picture() {
        let mut set = ThumbSet::default();
        assert!(set.insert(thumb(100, true)));
        assert!(!set.insert(thumb(100, false)));
        assert!(set.nearest(100).unwrap().exact);
        assert!(set.insert(thumb(100, true)));
    }
}
