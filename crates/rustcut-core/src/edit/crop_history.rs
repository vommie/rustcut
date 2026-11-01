//! The crops used earlier (P12 4.3, Q-P12c): newest first, no duplicates, kept across starts in `ui-state.toml`. A crop
//! belongs to the frame the crop filter got (`crop_frame`, before any turning after it), so a video of another size
//! either scales it (the same shape) or cannot use it. Pinned entries stay at the top of their group and do not count
//! toward the length.

use serde::{Deserialize, Serialize};

use super::{Crop, Size};

/// The history's length (Settings > Filters), pinned entries not counted.
pub const HISTORY_LENGTHS: std::ops::RangeInclusive<u8> = 1..=50;
pub const DEFAULT_HISTORY_LENGTH: u8 = 10;

/// How far two frames' shapes may differ and still count as one (in percent; PAL's 854×480 next to 1280×720).
const SHAPE_TOLERANCE: u64 = 1;

/// One crop of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropMemory {
    /// The frame the crop was made on.
    pub frame: Size,
    pub crop: Crop,
    /// Stays at the top and is never pushed out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// When it was last used (Unix seconds).
    #[serde(default)]
    pub used: i64,
    /// The video's file name (the entry's tooltip).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

impl CropMemory {
    /// What makes two entries one: exact (Q-P12c: 140 and 138 pixels stay two).
    pub fn same_crop(&self, other: &Self) -> bool {
        self.frame == other.frame && self.crop == other.crop
    }

    fn is_usable(&self) -> bool {
        !self.crop.is_identity() && self.crop.kept(self.frame).is_some()
    }
}

/// What a crop of the history means for a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Made on a frame of this size.
    Same,
    /// A frame of the same shape: this crop keeps the same part.
    Scaled(Crop),
    /// Another shape (landscape on portrait): not offered.
    Other,
}

/// How `memory` fits a `frame`-sized picture.
pub fn fit(memory: &CropMemory, frame: Size) -> Fit {
    if memory.frame == frame {
        return Fit::Same;
    }
    if !same_shape(memory.frame, frame) {
        return Fit::Other;
    }
    let crop = scaled(memory.crop, memory.frame, frame);
    if crop.is_identity() || crop.kept(frame).is_none() {
        return Fit::Other;
    }
    Fit::Scaled(crop)
}

fn same_shape(a: Size, b: Size) -> bool {
    let one = u64::from(a.width) * u64::from(b.height);
    let two = u64::from(b.width) * u64::from(a.height);
    let most = one.max(two);
    most > 0 && one.abs_diff(two).saturating_mul(100) <= most.saturating_mul(SHAPE_TOLERANCE)
}

fn scaled(crop: Crop, from: Size, to: Size) -> Crop {
    let part = |value: u32, to: u32, from: u32| -> u32 {
        let rounded = (u64::from(value) * u64::from(to) + u64::from(from) / 2) / u64::from(from.max(1));
        u32::try_from(rounded).unwrap_or(to)
    };
    Crop::new(
        part(crop.top, to.height, from.height),
        part(crop.right, to.width, from.width),
        part(crop.bottom, to.height, from.height),
        part(crop.left, to.width, from.width),
    )
    .clamped(to, 2)
    .evened_inward(to)
}

/// The list, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CropHistory {
    entries: Vec<CropMemory>,
}

impl CropHistory {
    /// The list `entries` (from `ui-state.toml`) leave: unusable and repeated entries go, the first of equal ones stays.
    pub fn new(entries: Vec<CropMemory>, length: usize) -> Self {
        let mut history = Self::default();
        for entry in entries {
            if entry.is_usable() && !history.entries.iter().any(|e| e.same_crop(&entry)) {
                history.entries.push(entry);
            }
        }
        history.trim(length);
        history
    }

    pub fn entries(&self) -> &[CropMemory] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `memory` was used: it goes to the top; an entry that is the same crop goes there instead, as a new one (the
    /// user's note) and stays pinned. `false` for a crop that cuts nothing.
    pub fn remember(&mut self, memory: CropMemory, length: usize) -> bool {
        if !memory.is_usable() {
            return false;
        }
        let pinned = self
            .entries
            .iter()
            .position(|e| e.same_crop(&memory))
            .is_some_and(|at| {
                let was = self.entries.get(at).is_some_and(|e| e.pinned);
                self.entries.remove(at);
                was
            });
        self.entries.insert(0, CropMemory { pinned, ..memory });
        self.trim(length);
        true
    }

    pub fn remove(&mut self, entry: &CropMemory) {
        self.entries.retain(|e| !e.same_crop(entry));
    }

    pub fn toggle_pin(&mut self, entry: &CropMemory) {
        if let Some(found) = self.entries.iter_mut().find(|e| e.same_crop(entry)) {
            found.pinned = !found.pinned;
        }
    }

    /// Everything but the pinned entries goes.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// The oldest entries beyond `length` go; pinned ones stay and do not count.
    pub fn trim(&mut self, length: usize) {
        let mut kept = 0;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            kept += 1;
            kept <= length
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: Size = Size::new(1920, 1080);

    fn memory(top: u32, frame: Size) -> CropMemory {
        CropMemory {
            frame,
            crop: Crop::new(top, 0, top, 0),
            pinned: false,
            used: 0,
            source: String::new(),
        }
    }

    fn tops(history: &CropHistory) -> Vec<u32> {
        history.entries().iter().map(|e| e.crop.top).collect()
    }

    #[test]
    fn a_repeated_crop_counts_as_new_and_is_not_added_twice() {
        let mut history = CropHistory::default();
        for top in [10, 20, 30] {
            assert!(history.remember(memory(top, HD), 10));
        }
        assert_eq!(tops(&history), [30, 20, 10]);
        history.remember(
            CropMemory {
                used: 9,
                ..memory(10, HD)
            },
            10,
        );
        assert_eq!(tops(&history), [10, 30, 20]);
        assert_eq!(history.entries().first().map(|e| e.used), Some(9));
    }

    #[test]
    fn the_same_edges_on_another_frame_are_another_entry() {
        let mut history = CropHistory::default();
        history.remember(memory(10, HD), 10);
        history.remember(memory(10, Size::new(1280, 720)), 10);
        assert_eq!(history.entries().len(), 2);
    }

    #[test]
    fn a_crop_that_cuts_nothing_or_everything_is_never_remembered() {
        let mut history = CropHistory::default();
        assert!(!history.remember(memory(0, HD), 10));
        assert!(!history.remember(memory(540, HD), 10));
        assert!(history.is_empty());
    }

    #[test]
    fn the_length_pushes_out_the_oldest_but_never_a_pinned_entry() {
        let mut history = CropHistory::default();
        history.remember(memory(10, HD), 2);
        let first = memory(10, HD);
        history.toggle_pin(&first);
        for top in [20, 30, 40] {
            history.remember(memory(top, HD), 2);
        }
        assert_eq!(tops(&history), [40, 30, 10]);
        history.remember(memory(10, HD), 2);
        assert!(
            history.entries().first().is_some_and(|e| e.pinned),
            "a used pinned entry stays pinned"
        );
        history.trim(1);
        assert_eq!(tops(&history), [10, 40]);
        history.clear();
        assert_eq!(tops(&history), [10]);
    }

    #[test]
    fn a_stored_list_loses_what_cannot_be_used() {
        let history = CropHistory::new(vec![memory(10, HD), memory(0, HD), memory(10, HD), memory(600, HD)], 10);
        assert_eq!(tops(&history), [10]);
    }

    #[test]
    fn a_frame_of_the_same_shape_scales_the_crop_onto_even_pixels() {
        let entry = memory(140, HD);
        assert_eq!(fit(&entry, HD), Fit::Same);
        assert_eq!(fit(&entry, Size::new(1280, 720)), Fit::Scaled(Crop::new(94, 0, 94, 0)));
        assert_eq!(fit(&entry, Size::new(854, 480)), Fit::Scaled(Crop::new(62, 0, 62, 0)));
    }

    #[test]
    fn another_shape_is_not_offered() {
        let entry = memory(140, HD);
        assert_eq!(fit(&entry, Size::new(1080, 1920)), Fit::Other);
        assert_eq!(fit(&entry, Size::new(1440, 1080)), Fit::Other);
    }

    proptest::proptest! {
        #[test]
        fn a_scaled_crop_always_leaves_a_picture(
            top in 0u32..500, right in 0u32..900, bottom in 0u32..500, left in 0u32..900, width in 1u32..400,
        ) {
            let entry = CropMemory { crop: Crop::new(top, right, bottom, left), ..memory(0, HD) };
            let frame = Size::new(width * 2 * 16 / 9 / 2 * 2, width * 2);
            if let Fit::Scaled(crop) = fit(&entry, frame) {
                proptest::prop_assert!(crop.kept(frame).is_some());
            }
        }
    }
}
