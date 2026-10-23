//! Crop edges in pixels of the frame that reaches the crop filter, and how they look through a later rotation or mirror.

use serde::{Deserialize, Serialize};

use super::filter::{Mirror, Rotation};

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

    proptest! {
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
