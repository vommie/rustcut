//! Crop edges in pixels of the frame that reaches the crop filter, and how they look through a later rotation or mirror.

use serde::{Deserialize, Serialize};

use super::chain::Size;
use super::filter::{Mirror, Rotation};

/// The aspect ratio a crop keeps while it is dragged on the video (persisted in `ui-state.toml`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CropAspect {
    #[default]
    #[serde(rename = "free")]
    Free,
    /// The shape of the uncropped picture.
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "4:3")]
    Standard,
    #[serde(rename = "3:2")]
    Photo,
    #[serde(rename = "1:1")]
    Square,
    #[serde(rename = "9:16")]
    Portrait,
}

impl CropAspect {
    pub const ALL: [Self; 7] = [
        Self::Free,
        Self::Source,
        Self::Wide,
        Self::Standard,
        Self::Photo,
        Self::Square,
        Self::Portrait,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Source => "Source",
            Self::Wide => "16:9",
            Self::Standard => "4:3",
            Self::Photo => "3:2",
            Self::Square => "1:1",
            Self::Portrait => "9:16",
        }
    }

    /// The display width / height to keep; `source` is the uncropped picture's. `None` = free.
    pub fn ratio(self, source: f64) -> Option<f64> {
        match self {
            Self::Free => None,
            Self::Source => (source.is_finite() && source > 0.0).then_some(source),
            Self::Wide => Some(16.0 / 9.0),
            Self::Standard => Some(4.0 / 3.0),
            Self::Photo => Some(1.5),
            Self::Square => Some(1.0),
            Self::Portrait => Some(9.0 / 16.0),
        }
    }
}

/// Pixels removed from each edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Crop {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl Crop {
    pub const fn new(top: u32, right: u32, bottom: u32, left: u32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn is_identity(self) -> bool {
        self.top == 0 && self.right == 0 && self.bottom == 0 && self.left == 0
    }

    /// The size that stays of a `frame`-sized picture; `None` when the crop removes everything.
    pub fn kept(self, frame: Size) -> Option<Size> {
        let width = frame.width.checked_sub(self.left)?.checked_sub(self.right)?;
        let height = frame.height.checked_sub(self.top)?.checked_sub(self.bottom)?;
        (width > 0 && height > 0).then_some(Size::new(width, height))
    }

    /// The crop limited so that at least `min` pixels of a `frame`-sized picture stay in each direction (or the whole
    /// side when the picture is smaller than that). Each edge gives up the same share when both are too large.
    pub fn clamped(self, frame: Size, min: u32) -> Self {
        let fit = |a: u32, b: u32, side: u32| -> (u32, u32) {
            let room = side.saturating_sub(min.min(side));
            let total = u64::from(a) + u64::from(b);
            if total <= u64::from(room) {
                return (a, b);
            }
            // Shrink both in proportion; the rounding remainder goes to the second edge.
            let a2 = u32::try_from(u64::from(a) * u64::from(room) / total.max(1)).unwrap_or(room);
            (a2, room - a2)
        };
        let (top, bottom) = fit(self.top, self.bottom, frame.height);
        let (left, right) = fit(self.left, self.right, frame.width);
        Self::new(top, right, bottom, left)
    }

    /// The crop with an even kept size of a `frame`-sized picture (yuv420 encoders need it): one more pixel is taken at
    /// the right and the bottom when the size left is odd and there is room for it.
    pub fn with_even_size(self, frame: Size) -> Self {
        let mut c = self;
        if let Some(kept) = c.kept(frame) {
            if !kept.width.is_multiple_of(2) && kept.width > 1 {
                c.right += 1;
            }
            if !kept.height.is_multiple_of(2) && kept.height > 1 {
                c.bottom += 1;
            }
        }
        c
    }

    /// The crop moved inward onto even pixels (top and left rounded up to even numbers) with an even kept size: what
    /// auto-crop applies, so no thin dark line of a border stays and chroma planes line up.
    pub fn evened_inward(self, frame: Size) -> Self {
        let up = |v: u32| v.saturating_add(v & 1);
        Self {
            top: up(self.top),
            left: up(self.left),
            ..self
        }
        .with_even_size(frame)
    }

    /// The same crop described in the orientation the viewer sees after `rotation` (crop placed before the rotation in the
    /// chain): the edge fields of the UI follow the picture (PyCutRv parity, "labels follow rotation").
    pub const fn seen_through_rotation(self, rotation: Rotation) -> Self {
        let Self {
            top,
            right,
            bottom,
            left,
        } = self;
        match rotation {
            // Clockwise: the source's left edge appears at the top, its top at the right, ...
            Rotation::Clockwise => Self::new(left, top, right, bottom),
            Rotation::CounterClockwise => Self::new(right, bottom, left, top),
            Rotation::UpsideDown => Self::new(bottom, left, top, right),
        }
    }

    /// Inverse of [`Crop::seen_through_rotation`]: edges typed in the rotated view, back in source orientation.
    pub const fn unseen_through_rotation(self, rotation: Rotation) -> Self {
        let inverse = match rotation {
            Rotation::Clockwise => Rotation::CounterClockwise,
            Rotation::CounterClockwise => Rotation::Clockwise,
            Rotation::UpsideDown => Rotation::UpsideDown,
        };
        self.seen_through_rotation(inverse)
    }

    /// The same crop as it looks after a mirror (its own inverse).
    pub const fn seen_through_mirror(self, mirror: Mirror) -> Self {
        let Self {
            mut top,
            mut right,
            mut bottom,
            mut left,
        } = self;
        if mirror.horizontal {
            (left, right) = (right, left);
        }
        if mirror.vertical {
            (top, bottom) = (bottom, top);
        }
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn clockwise_moves_left_to_top() {
        let c = Crop::new(1, 2, 3, 4);
        assert_eq!(c.seen_through_rotation(Rotation::Clockwise), Crop::new(4, 1, 2, 3));
        assert_eq!(
            c.seen_through_rotation(Rotation::CounterClockwise),
            Crop::new(2, 3, 4, 1)
        );
        assert_eq!(c.seen_through_rotation(Rotation::UpsideDown), Crop::new(3, 4, 1, 2));
        assert_eq!(
            c.seen_through_mirror(Mirror {
                horizontal: true,
                vertical: false
            }),
            Crop::new(1, 4, 3, 2)
        );
    }

    fn arb_rotation() -> impl Strategy<Value = Rotation> {
        prop_oneof![
            Just(Rotation::Clockwise),
            Just(Rotation::CounterClockwise),
            Just(Rotation::UpsideDown)
        ]
    }

    #[test]
    fn kept_size_and_clamping() {
        let frame = Size::new(100, 50);
        assert_eq!(Crop::new(5, 10, 5, 10).kept(frame), Some(Size::new(80, 40)));
        assert_eq!(Crop::new(25, 0, 25, 0).kept(frame), None);
        assert_eq!(
            Crop::new(40, 0, 40, 0).clamped(frame, 10),
            Crop::new(20, 0, 20, 0),
            "both edges give up the same share"
        );
        assert_eq!(Crop::new(1, 2, 3, 4).clamped(frame, 10), Crop::new(1, 2, 3, 4));
        assert_eq!(
            Crop::new(0, 0, 9, 0).clamped(Size::new(4, 8), 10).kept(Size::new(4, 8)),
            Some(Size::new(4, 8))
        );
    }

    #[test]
    fn auto_crop_rounding_moves_inward_to_even_pixels() {
        let frame = Size::new(1920, 1080);
        assert_eq!(
            Crop::new(139, 1, 141, 0).evened_inward(frame),
            Crop::new(140, 2, 142, 0)
        );
        // An odd source keeps an even size by taking the extra pixel at the right/bottom.
        let odd = Size::new(853, 479);
        let c = Crop::new(0, 0, 0, 0).evened_inward(odd);
        assert_eq!(c, Crop::new(0, 1, 1, 0));
        assert!(c.kept(odd).unwrap().is_even());
    }

    proptest! {
        #[test]
        fn clamped_crops_keep_the_minimum(w in 1u32..5000, h in 1u32..5000, t: u32, r: u32, b: u32, l: u32, min in 0u32..64) {
            let frame = Size::new(w, h);
            let c = Crop::new(t, r, b, l).clamped(frame, min);
            let kept = c.kept(frame);
            prop_assert!(kept.is_some() || min == 0);
            if let Some(kept) = kept {
                prop_assert!(kept.width >= min.min(w) && kept.height >= min.min(h));
            }
        }

        #[test]
        fn evened_crops_are_even_inside_the_frame(w in 4u32..5000, h in 4u32..5000, t in 0u32..500, r in 0u32..500,
            b in 0u32..500, l in 0u32..500) {
            let frame = Size::new(w, h);
            let c = Crop::new(t, r, b, l).clamped(frame, 4).evened_inward(frame);
            let kept = c.kept(frame).unwrap();
            prop_assert!(kept.is_even() && c.top.is_multiple_of(2) && c.left.is_multiple_of(2));
        }

        #[test]
        fn rotation_mapping_is_invertible(t: u32, r: u32, b: u32, l: u32, rot in arb_rotation()) {
            let c = Crop::new(t, r, b, l);
            prop_assert_eq!(c.seen_through_rotation(rot).unseen_through_rotation(rot), c);
        }

        #[test]
        fn mirror_is_its_own_inverse(t: u32, r: u32, b: u32, l: u32, h: bool, v: bool) {
            let c = Crop::new(t, r, b, l);
            let m = Mirror { horizontal: h, vertical: v };
            prop_assert_eq!(c.seen_through_mirror(m).seen_through_mirror(m), c);
        }
    }
}
