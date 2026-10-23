//! The ordered filter chain of a job: every filter kind exactly once, each enabled or not, in a user-defined order.

use serde::{Deserialize, Serialize};

use super::{Filter, FilterKind};

/// Width and height of a video frame in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_even(self) -> bool {
        self.width.is_multiple_of(2) && self.height.is_multiple_of(2)
    }

    /// The size rounded down to even numbers (yuv420 encoders need them).
    pub const fn even_floor(self) -> Self {
        Self {
            width: self.width & !1,
            height: self.height & !1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChainError {
    #[error("the crop removes the whole picture ({width}x{height} left of the {source_width}x{source_height} frame)")]
    CropTooLarge {
        width: i64,
        height: i64,
        source_width: u32,
        source_height: u32,
    },
    #[error("the output would be smaller than 2x2 pixels ({0}x{1})")]
    TooSmall(u32, u32),
    #[error("deshake is enabled but the render pass has no transform file (two passes are needed)")]
    DeshakeNeedsPass,
    #[error("the path of the deshake transform file is not valid UTF-8")]
    NonUtf8Path,
}

/// One filter of the chain with its on/off state. A disabled filter keeps its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FilterSlot {
    pub enabled: bool,
    pub filter: Filter,
}

/// The filter chain of a job. Invariant: each [`FilterKind`] appears exactly once; reading an old or hand-edited list
/// repairs it (duplicates dropped, missing kinds appended disabled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<FilterSlot>", into = "Vec<FilterSlot>")]
pub struct FilterChain {
    slots: Vec<FilterSlot>,
}

impl Default for FilterChain {
    fn default() -> Self {
        Self {
            slots: FilterKind::DEFAULT_ORDER
                .iter()
                .map(|&kind| FilterSlot {
                    enabled: false,
                    filter: Filter::default_for(kind),
                })
                .collect(),
        }
    }
}

impl FilterChain {
    pub fn slots(&self) -> &[FilterSlot] {
        &self.slots
    }

    pub fn order(&self) -> impl ExactSizeIterator<Item = FilterKind> + '_ {
        self.slots.iter().map(|s| s.filter.kind())
    }

    pub fn get(&self, kind: FilterKind) -> Option<&FilterSlot> {
        self.slots.iter().find(|s| s.filter.kind() == kind)
    }

    pub fn is_enabled(&self, kind: FilterKind) -> bool {
        self.get(kind).is_some_and(|s| s.enabled)
    }

    /// Replaces the parameters of the filter's kind; position and on/off state stay.
    pub fn set(&mut self, filter: Filter) {
        if let Some(slot) = self.slot_mut(filter.kind()) {
            slot.filter = filter;
        }
    }

    pub fn set_enabled(&mut self, kind: FilterKind, enabled: bool) {
        if let Some(slot) = self.slot_mut(kind) {
            slot.enabled = enabled;
        }
    }

    /// Moves a filter one place earlier; `false` when it is first already.
    pub fn move_up(&mut self, kind: FilterKind) -> bool {
        match self.position(kind) {
            Some(i) if i > 0 => {
                self.slots.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves a filter one place later; `false` when it is last already.
    pub fn move_down(&mut self, kind: FilterKind) -> bool {
        match self.position(kind) {
            Some(i) if i + 1 < self.slots.len() => {
                self.slots.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// The enabled filters that change something, in chain order.
    pub fn active(&self) -> impl Iterator<Item = &Filter> + '_ {
        self.slots
            .iter()
            .filter(|s| s.enabled && !s.filter.is_identity())
            .map(|s| &s.filter)
    }

    /// Whether the render needs two passes (deshake enabled).
    pub fn needs_two_passes(&self) -> bool {
        self.active().any(|f| f.kind() == FilterKind::Deshake)
    }

    /// The frame size after the chain for a source of `source` pixels (as ffmpeg computes it, before the even-size guard).
    pub fn output_size(&self, source: Size) -> Result<Size, ChainError> {
        let mut size = source;
        for filter in self.active() {
            size = match *filter {
                Filter::Crop(c) => {
                    let width = i64::from(size.width) - i64::from(c.left) - i64::from(c.right);
                    let height = i64::from(size.height) - i64::from(c.top) - i64::from(c.bottom);
                    match (u32::try_from(width), u32::try_from(height)) {
                        (Ok(w), Ok(h)) if w > 0 && h > 0 => Size::new(w, h),
                        _ => {
                            return Err(ChainError::CropTooLarge {
                                width,
                                height,
                                source_width: size.width,
                                source_height: size.height,
                            });
                        }
                    }
                }
                Filter::Rotate { rotation } if rotation.swaps_axes() => Size::new(size.height, size.width),
                Filter::Resize { width, height, .. } => resized(size, width, height),
                Filter::Rotate { .. } | Filter::Mirror(_) | Filter::Deinterlace { .. } | Filter::Deshake(_) => size,
            };
        }
        if size.width < 2 || size.height < 2 {
            return Err(ChainError::TooSmall(size.width, size.height));
        }
        Ok(size)
    }

    fn position(&self, kind: FilterKind) -> Option<usize> {
        self.slots.iter().position(|s| s.filter.kind() == kind)
    }

    fn slot_mut(&mut self, kind: FilterKind) -> Option<&mut FilterSlot> {
        self.slots.iter_mut().find(|s| s.filter.kind() == kind)
    }
}

/// ffmpeg's `scale=w:-2` arithmetic: the missing side keeps the aspect ratio, rounded to the nearest multiple of 2.
fn resized(size: Size, width: Option<u32>, height: Option<u32>) -> Size {
    let rescale_even = |value: u32, num: u32, den: u32| -> u32 {
        let den = u64::from(den.max(1)) * 2;
        let scaled = (u64::from(value) * u64::from(num) + den / 2) / den * 2;
        u32::try_from(scaled).unwrap_or(u32::MAX)
    };
    match (width, height) {
        (Some(w), Some(h)) => Size::new(w, h),
        (Some(w), None) => Size::new(w, rescale_even(w, size.height, size.width)),
        (None, Some(h)) => Size::new(rescale_even(h, size.width, size.height), h),
        (None, None) => size,
    }
}

impl From<Vec<FilterSlot>> for FilterChain {
    fn from(slots: Vec<FilterSlot>) -> Self {
        let mut repaired: Vec<FilterSlot> = Vec::with_capacity(FilterKind::DEFAULT_ORDER.len());
        for slot in slots {
            if !repaired.iter().any(|s| s.filter.kind() == slot.filter.kind()) {
                repaired.push(slot);
            }
        }
        for kind in FilterKind::DEFAULT_ORDER {
            if !repaired.iter().any(|s| s.filter.kind() == kind) {
                repaired.push(FilterSlot {
                    enabled: false,
                    filter: Filter::default_for(kind),
                });
            }
        }
        Self { slots: repaired }
    }
}

impl From<FilterChain> for Vec<FilterSlot> {
    fn from(chain: FilterChain) -> Self {
        chain.slots
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::edit::{Crop, Rotation};

    fn chain_with(filters: &[Filter]) -> FilterChain {
        let mut chain = FilterChain::default();
        for &f in filters {
            chain.set(f);
            chain.set_enabled(f.kind(), true);
        }
        chain
    }

    #[test]
    fn default_chain_has_every_kind_disabled() {
        let chain = FilterChain::default();
        assert_eq!(chain.order().collect::<Vec<_>>(), FilterKind::DEFAULT_ORDER.to_vec());
        assert_eq!(chain.active().count(), 0);
        assert!(!chain.needs_two_passes());
    }

    #[test]
    fn output_size_follows_crop_rotate_resize() {
        let chain = chain_with(&[
            Filter::Crop(Crop::new(10, 20, 10, 20)),
            Filter::Rotate {
                rotation: Rotation::Clockwise,
            },
            Filter::Resize {
                width: None,
                height: Some(720),
                algorithm: crate::edit::ResizeAlgorithm::Lanczos,
            },
        ]);
        // 1920x1080 -> crop 1880x1060 -> rotate 1060x1880 -> height 720 keeps the aspect: 406x720 (405.96 rounded to even)
        assert_eq!(chain.output_size(Size::new(1920, 1080)).unwrap(), Size::new(406, 720));
    }

    #[test]
    fn a_crop_larger_than_the_frame_is_an_error() {
        let chain = chain_with(&[Filter::Crop(Crop::new(0, 100, 0, 100))]);
        assert!(matches!(
            chain.output_size(Size::new(200, 100)),
            Err(ChainError::CropTooLarge { width: 0, .. })
        ));
    }

    #[test]
    fn disabled_filters_do_not_count() {
        let mut chain = chain_with(&[Filter::Rotate {
            rotation: Rotation::Clockwise,
        }]);
        chain.set_enabled(FilterKind::Rotate, false);
        assert_eq!(chain.output_size(Size::new(640, 480)).unwrap(), Size::new(640, 480));
    }

    #[test]
    fn reading_repairs_the_invariant() {
        let json =
            r#"[{"enabled":true,"filter":{"kind":"resize","width":640}},{"enabled":false,"filter":{"kind":"resize"}}]"#;
        let chain: FilterChain = serde_json::from_str(json).unwrap();
        assert_eq!(chain.slots().len(), FilterKind::DEFAULT_ORDER.len());
        assert_eq!(chain.order().next(), Some(FilterKind::Resize));
        assert!(chain.is_enabled(FilterKind::Resize));
    }

    #[test]
    fn reorder_moves_one_step() {
        let mut chain = FilterChain::default();
        assert!(!chain.move_up(FilterKind::Deinterlace));
        assert!(chain.move_up(FilterKind::Crop));
        assert_eq!(
            chain.order().take(2).collect::<Vec<_>>(),
            vec![FilterKind::Crop, FilterKind::Deinterlace]
        );
        assert!(!chain.move_down(FilterKind::Resize));
    }

    proptest! {
        #[test]
        fn resize_with_one_side_gives_an_even_other_side(w in 2u32..8000, h in 2u32..8000, target in 2u32..8000) {
            let out = resized(Size::new(w, h), Some(target), None);
            prop_assert_eq!(out.width, target);
            prop_assert!(out.height.is_multiple_of(2));
        }
    }
}
