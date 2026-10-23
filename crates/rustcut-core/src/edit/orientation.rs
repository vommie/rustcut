//! Rotations and mirrors as one transform of the picture (the eight symmetries of a rectangle), so a crop, a point or a
//! rectangle can be followed through every rotate and mirror filter of a chain, and back.

use super::chain::Size;
use super::crop::Crop;
use super::filter::{Mirror, Rotation};

/// A combination of quarter turns and mirrors, as a 2x2 matrix acting on picture coordinates centered on the picture
/// (x to the right, y downward). Every entry is -1, 0 or 1 and the matrix is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Orientation {
    m: [[i8; 2]; 2],
}

impl Default for Orientation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Orientation {
    pub const IDENTITY: Self = Self { m: [[1, 0], [0, 1]] };
    /// A quarter turn clockwise: the top-left corner moves to the top right.
    pub const CLOCKWISE: Self = Self { m: [[0, -1], [1, 0]] };
    pub const COUNTER_CLOCKWISE: Self = Self { m: [[0, 1], [-1, 0]] };
    pub const UPSIDE_DOWN: Self = Self { m: [[-1, 0], [0, -1]] };
    /// Left and right trade places (`hflip`).
    pub const MIRROR_HORIZONTAL: Self = Self { m: [[-1, 0], [0, 1]] };
    /// Top and bottom trade places (`vflip`).
    pub const MIRROR_VERTICAL: Self = Self { m: [[1, 0], [0, -1]] };

    pub const fn from_rotation(rotation: Rotation) -> Self {
        match rotation {
            Rotation::Clockwise => Self::CLOCKWISE,
            Rotation::CounterClockwise => Self::COUNTER_CLOCKWISE,
            Rotation::UpsideDown => Self::UPSIDE_DOWN,
        }
    }

    pub fn from_mirror(mirror: Mirror) -> Self {
        let mut o = Self::IDENTITY;
        if mirror.horizontal {
            o = o.then(Self::MIRROR_HORIZONTAL);
        }
        if mirror.vertical {
            o = o.then(Self::MIRROR_VERTICAL);
        }
        o
    }

    /// The display rotation of a source, in degrees clockwise as mpv applies it (0, 90, 180, 270; anything else is not
    /// a quarter turn and counts as none).
    pub const fn from_degrees_clockwise(degrees: u16) -> Self {
        match degrees % 360 {
            90 => Self::CLOCKWISE,
            180 => Self::UPSIDE_DOWN,
            270 => Self::COUNTER_CLOCKWISE,
            _ => Self::IDENTITY,
        }
    }

    /// This transform followed by `next`.
    pub const fn then(self, next: Self) -> Self {
        let a = next.m;
        let b = self.m;
        Self {
            m: [
                [
                    a[0][0] * b[0][0] + a[0][1] * b[1][0],
                    a[0][0] * b[0][1] + a[0][1] * b[1][1],
                ],
                [
                    a[1][0] * b[0][0] + a[1][1] * b[1][0],
                    a[1][0] * b[0][1] + a[1][1] * b[1][1],
                ],
            ],
        }
    }

    /// The transform that undoes this one (the transposed matrix).
    pub const fn inverse(self) -> Self {
        Self {
            m: [[self.m[0][0], self.m[1][0]], [self.m[0][1], self.m[1][1]]],
        }
    }

    /// Whether width and height trade places.
    pub const fn swaps_axes(self) -> bool {
        self.m[0][0] == 0
    }

    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// The size of a `size` picture after the transform.
    pub const fn size(self, size: Size) -> Size {
        if self.swaps_axes() {
            Size::new(size.height, size.width)
        } else {
            size
        }
    }

    /// A sample aspect ratio (pixel width / height) after the transform: a quarter turn inverts it (as ffmpeg's
    /// `transpose` does).
    pub fn sample_aspect(self, sar: f64) -> f64 {
        if self.swaps_axes() && sar > 0.0 { 1.0 / sar } else { sar }
    }

    /// Where the point `(x, y)` of a `size` picture lands after the transform (continuous coordinates: `(0, 0)` is the
    /// top-left corner of the first pixel, `(width, height)` the bottom-right corner of the last).
    pub fn map_point(self, size: Size, (x, y): (f64, f64)) -> (f64, f64) {
        let (cx, cy) = (f64::from(size.width) / 2.0, f64::from(size.height) / 2.0);
        let out = self.size(size);
        let (ox, oy) = (f64::from(out.width) / 2.0, f64::from(out.height) / 2.0);
        let (dx, dy) = (x - cx, y - cy);
        let m = self.m;
        (
            ox + f64::from(m[0][0]) * dx + f64::from(m[0][1]) * dy,
            oy + f64::from(m[1][0]) * dx + f64::from(m[1][1]) * dy,
        )
    }

    /// The crop of a `size` picture as it looks after the transform: the same kept area, described by the edges of the
    /// transformed picture. Exact for integer edges (quarter turns and mirrors only permute the edges).
    pub fn map_crop(self, crop: Crop) -> Crop {
        // Each edge of the result is the source edge whose outward direction the matrix turns into it.
        let edge = |dir: (i8, i8)| -> u32 {
            // The source direction that maps onto `dir` is M^T * dir.
            let m = self.m;
            let sx = m[0][0] * dir.0 + m[1][0] * dir.1;
            let sy = m[0][1] * dir.0 + m[1][1] * dir.1;
            match (sx, sy) {
                (0, -1) => crop.top,
                (1, 0) => crop.right,
                (0, 1) => crop.bottom,
                _ => crop.left,
            }
        };
        Crop::new(edge((0, -1)), edge((1, 0)), edge((0, 1)), edge((-1, 0)))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const ALL: [Orientation; 8] = [
        Orientation::IDENTITY,
        Orientation::CLOCKWISE,
        Orientation::COUNTER_CLOCKWISE,
        Orientation::UPSIDE_DOWN,
        Orientation::MIRROR_HORIZONTAL,
        Orientation::MIRROR_VERTICAL,
        Orientation { m: [[0, 1], [1, 0]] },
        Orientation { m: [[0, -1], [-1, 0]] },
    ];

    fn arb_orientation() -> impl Strategy<Value = Orientation> {
        (0..ALL.len()).prop_map(|i| ALL[i])
    }

    #[test]
    fn the_group_is_closed_and_every_element_has_an_inverse() {
        for a in ALL {
            assert_eq!(a.then(a.inverse()), Orientation::IDENTITY);
            assert_eq!(a.inverse().then(a), Orientation::IDENTITY);
            for b in ALL {
                assert!(ALL.contains(&a.then(b)), "{a:?} then {b:?}");
            }
        }
        assert_eq!(
            Orientation::CLOCKWISE.then(Orientation::CLOCKWISE),
            Orientation::UPSIDE_DOWN
        );
        assert_eq!(
            Orientation::MIRROR_HORIZONTAL.then(Orientation::MIRROR_VERTICAL),
            Orientation::UPSIDE_DOWN
        );
    }

    #[test]
    fn a_clockwise_turn_moves_the_top_left_corner_to_the_top_right() {
        let size = Size::new(640, 360);
        assert_eq!(Orientation::CLOCKWISE.map_point(size, (0.0, 0.0)), (360.0, 0.0));
        assert_eq!(Orientation::CLOCKWISE.size(size), Size::new(360, 640));
        assert_eq!(Orientation::COUNTER_CLOCKWISE.map_point(size, (0.0, 0.0)), (0.0, 640.0));
        assert_eq!(
            Orientation::MIRROR_HORIZONTAL.map_point(size, (10.0, 20.0)),
            (630.0, 20.0)
        );
    }

    #[test]
    fn crops_follow_the_rotation_like_the_crop_fields() {
        let c = Crop::new(1, 2, 3, 4);
        for rotation in [Rotation::Clockwise, Rotation::CounterClockwise, Rotation::UpsideDown] {
            assert_eq!(
                Orientation::from_rotation(rotation).map_crop(c),
                c.seen_through_rotation(rotation)
            );
        }
        let mirror = Mirror {
            horizontal: true,
            vertical: true,
        };
        assert_eq!(
            Orientation::from_mirror(mirror).map_crop(c),
            c.seen_through_mirror(mirror)
        );
        assert_eq!(Orientation::from_degrees_clockwise(450), Orientation::CLOCKWISE);
        assert_eq!(Orientation::from_degrees_clockwise(45), Orientation::IDENTITY);
    }

    #[test]
    fn a_quarter_turn_inverts_the_sample_aspect() {
        assert!((Orientation::CLOCKWISE.sample_aspect(2.0) - 0.5).abs() < 1e-12);
        assert!((Orientation::UPSIDE_DOWN.sample_aspect(2.0) - 2.0).abs() < 1e-12);
    }

    proptest! {
        #[test]
        fn crop_mapping_matches_the_mapped_corners(o in arb_orientation(), w in 2u32..4000, h in 2u32..4000,
            t in 0u32..1000, r in 0u32..1000, b in 0u32..1000, l in 0u32..1000) {
            prop_assume!(l + r < w && t + b < h);
            let size = Size::new(w, h);
            let crop = Crop::new(t, r, b, l);
            let p1 = o.map_point(size, (f64::from(l), f64::from(t)));
            let p2 = o.map_point(size, (f64::from(w - r), f64::from(h - b)));
            let out = o.size(size);
            let mapped = o.map_crop(crop);
            let near = |a: u32, b: f64| (f64::from(a) - b).abs() < 1e-9;
            prop_assert!(near(mapped.left, p1.0.min(p2.0)) && near(mapped.top, p1.1.min(p2.1)));
            prop_assert!(near(out.width - mapped.right, p1.0.max(p2.0)));
            prop_assert!(near(out.height - mapped.bottom, p1.1.max(p2.1)));
            prop_assert_eq!(o.inverse().map_crop(mapped), crop);
        }

        #[test]
        fn points_come_back_through_the_inverse(o in arb_orientation(), w in 1u32..4000, h in 1u32..4000,
            x in 0.0f64..4000.0, y in 0.0f64..4000.0) {
            let size = Size::new(w, h);
            let there = o.map_point(size, (x, y));
            let back = o.inverse().map_point(o.size(size), there);
            prop_assert!((back.0 - x).abs() < 1e-9 && (back.1 - y).abs() < 1e-9);
        }
    }
}
