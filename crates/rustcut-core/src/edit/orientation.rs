//! Rotations and mirrors as one transform of the picture (the eight symmetries of a rectangle): the Orientation filter,
//! and the way a crop, a point or a rectangle is followed through it, and back.

use serde::{Deserialize, Serialize};

use super::chain::Size;
use super::crop::Crop;
use super::filter::{Mirror, Rotation};

/// A combination of quarter turns and mirrors, as a 2x2 matrix acting on picture coordinates centered on the picture
/// (x to the right, y downward). Every entry is -1, 0 or 1 and the matrix is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Orientation {
    m: [[i8; 2]; 2],
}

/// The Orientation filter's values as the user set them (user decision Q-P9.13b): a quarter turn to the right
/// (`turns` 1) or to the left (3) or none (0), like radio buttons, then mirrored left to right and top to bottom, two
/// switches of their own (both on is the picture upside down). Half a turn (2, from an old Rotate) means the same as
/// both mirrors and is stored as them ([`OrientationParams::normalized`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct OrientationParams {
    /// 0, 1 (right) or 3 (left); more wrap around.
    pub turns: u8,
    pub mirror_horizontal: bool,
    pub mirror_vertical: bool,
}

impl OrientationParams {
    pub const UPRIGHT: Self = Self {
        turns: 0,
        mirror_horizontal: false,
        mirror_vertical: false,
    };

    /// The transform these values make.
    pub const fn orientation(self) -> Orientation {
        let mut o = Orientation::turned(self.turns);
        if self.mirror_horizontal {
            o = o.then(Orientation::MIRROR_HORIZONTAL);
        }
        if self.mirror_vertical {
            o = o.then(Orientation::MIRROR_VERTICAL);
        }
        o
    }

    /// Whether the picture stays as it is.
    pub const fn is_identity(self) -> bool {
        self.orientation().is_identity()
    }

    /// The same picture with `turns` 0, 1 or 3: half a turn becomes both mirrors switched (it is the same as them).
    #[must_use]
    pub const fn normalized(self) -> Self {
        let turns = self.turns % 4;
        if turns == 2 {
            Self {
                turns: 0,
                mirror_horizontal: !self.mirror_horizontal,
                mirror_vertical: !self.mirror_vertical,
            }
        } else {
            Self { turns, ..self }
        }
    }

    /// The turn buttons and keys: a quarter to the right (`clockwise`) or to the left; the one that is on switches off.
    #[must_use]
    pub const fn pick_turn(self, clockwise: bool) -> Self {
        let wanted = if clockwise { 1 } else { 3 };
        let current = self.normalized();
        Self {
            turns: if current.turns == wanted { 0 } else { wanted },
            ..current
        }
    }

    /// The mirror buttons and keys: switch the mirror left to right (`horizontal`) or top to bottom.
    #[must_use]
    pub const fn toggle_mirror(self, horizontal: bool) -> Self {
        let mut next = self.normalized();
        if horizontal {
            next.mirror_horizontal = !next.mirror_horizontal;
        } else {
            next.mirror_vertical = !next.mirror_vertical;
        }
        next
    }

    /// Values that make `o`, with the fewest switches on.
    pub fn of(o: Orientation) -> Self {
        let mut best: Option<Self> = None;
        for turns in [0, 1, 3] {
            for (mirror_horizontal, mirror_vertical) in [(false, false), (true, false), (false, true), (true, true)] {
                let params = Self {
                    turns,
                    mirror_horizontal,
                    mirror_vertical,
                };
                let count =
                    |p: Self| u8::from(p.turns != 0) + u8::from(p.mirror_horizontal) + u8::from(p.mirror_vertical);
                if params.orientation() == o && best.is_none_or(|b| count(params) < count(b)) {
                    best = Some(params);
                }
            }
        }
        best.unwrap_or(Self::UPRIGHT)
    }

    /// These values followed by `next`: kept as set when only turns come before only mirrors (an old Rotate before an
    /// old Mirror), else the values of the combined transform.
    pub fn then(self, next: Self) -> Self {
        if !self.mirror_horizontal && !self.mirror_vertical && next.turns.is_multiple_of(4) {
            Self {
                turns: self.turns,
                ..next
            }
            .normalized()
        } else {
            Self::of(self.orientation().then(next.orientation()))
        }
    }

    /// The result in words ("90° right, mirrored").
    pub const fn label(self) -> &'static str {
        self.orientation().label()
    }
}

impl From<Orientation> for OrientationParams {
    fn from(o: Orientation) -> Self {
        Self::of(o)
    }
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

    /// `turns` quarter turns clockwise (taken modulo 4).
    pub const fn turned(turns: u8) -> Self {
        match turns % 4 {
            1 => Self::CLOCKWISE,
            2 => Self::UPSIDE_DOWN,
            3 => Self::COUNTER_CLOCKWISE,
            _ => Self::IDENTITY,
        }
    }

    /// The quarter turns clockwise (0 to 3) and whether a mirror left to right follows them: how a picture of the
    /// result is drawn (Slint turns first, then scales).
    pub fn turns_and_mirror(self) -> (u8, bool) {
        for mirrored in [false, true] {
            for turns in 0..4 {
                let mut o = Self::turned(turns);
                if mirrored {
                    o = o.then(Self::MIRROR_HORIZONTAL);
                }
                if o == self {
                    return (turns, mirrored);
                }
            }
        }
        (0, false)
    }

    /// The ffmpeg filters that produce this orientation (one per case, checked with the real ffmpeg: `transpose`'s
    /// `cclock_flip` mirrors over the main diagonal, `clock_flip` over the other one); `None` when upright.
    pub const fn lavfi(self) -> Option<&'static str> {
        match self.m {
            [[0, -1], [1, 0]] => Some("transpose=dir=clock"),
            [[0, 1], [-1, 0]] => Some("transpose=dir=cclock"),
            [[-1, 0], [0, -1]] => Some("hflip,vflip"),
            [[-1, 0], [0, 1]] => Some("hflip"),
            [[1, 0], [0, -1]] => Some("vflip"),
            [[0, 1], [1, 0]] => Some("transpose=dir=cclock_flip"),
            [[0, -1], [-1, 0]] => Some("transpose=dir=clock_flip"),
            _ => None,
        }
    }

    /// The orientation in words ("90° right, mirrored").
    pub const fn label(self) -> &'static str {
        match self.m {
            [[0, -1], [1, 0]] => "90° right",
            [[0, 1], [-1, 0]] => "90° left",
            [[-1, 0], [0, -1]] => "180°",
            [[-1, 0], [0, 1]] => "Mirrored left to right",
            [[1, 0], [0, -1]] => "Mirrored top to bottom",
            [[0, 1], [1, 0]] => "90° right, mirrored",
            [[0, -1], [-1, 0]] => "90° left, mirrored",
            _ => "Upright",
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

    pub const fn is_identity(self) -> bool {
        matches!(self.m, [[1, 0], [0, 1]])
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

    fn arb_params() -> impl Strategy<Value = OrientationParams> {
        (0u8..8, any::<bool>(), any::<bool>()).prop_map(|(turns, mirror_horizontal, mirror_vertical)| {
            OrientationParams {
                turns,
                mirror_horizontal,
                mirror_vertical,
            }
        })
    }

    #[test]
    fn every_orientation_can_be_set_and_has_a_filter_and_a_name() {
        let mut names = std::collections::HashSet::new();
        for o in ALL {
            let params = OrientationParams::of(o);
            assert!(params.turns != 2, "{params:?}");
            assert_eq!(params.orientation(), o);
            assert_eq!(o.lavfi().is_none(), o.is_identity());
            assert!(names.insert(o.label()), "{o:?}");
        }
        // Upside down is both mirrors.
        assert_eq!(
            OrientationParams::of(Orientation::UPSIDE_DOWN),
            OrientationParams {
                turns: 0,
                mirror_horizontal: true,
                mirror_vertical: true
            }
        );
        let five = OrientationParams {
            turns: 5,
            ..OrientationParams::UPRIGHT
        };
        assert_eq!(five.orientation(), Orientation::CLOCKWISE, "turns wrap around");
        assert_eq!(Orientation::UPSIDE_DOWN.turns_and_mirror(), (2, false));
        assert_eq!(Orientation::MIRROR_VERTICAL.turns_and_mirror(), (2, true));
        assert_eq!(
            Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL).lavfi(),
            Some("transpose=dir=cclock_flip")
        );
        // An old Rotate before an old Mirror keeps both as set; an old half turn becomes both mirrors.
        let rotate = OrientationParams {
            turns: 1,
            ..OrientationParams::UPRIGHT
        };
        let mirror = OrientationParams {
            mirror_vertical: true,
            ..OrientationParams::UPRIGHT
        };
        assert_eq!(
            rotate.then(mirror),
            OrientationParams {
                turns: 1,
                mirror_horizontal: false,
                mirror_vertical: true
            }
        );
        let half = OrientationParams {
            turns: 2,
            ..OrientationParams::UPRIGHT
        };
        assert_eq!(
            half.then(mirror),
            OrientationParams {
                turns: 0,
                mirror_horizontal: true,
                mirror_vertical: false
            }
        );
        assert_eq!(
            serde_json::to_string(&rotate.toggle_mirror(true)).unwrap(),
            r#"{"turns":1,"mirror_horizontal":true,"mirror_vertical":false}"#
        );
    }

    #[test]
    fn the_turn_buttons_work_like_radio_buttons_and_the_mirrors_like_switches() {
        let p = OrientationParams::UPRIGHT.pick_turn(true);
        assert_eq!(p.turns, 1);
        assert_eq!(p.pick_turn(false).turns, 3, "left replaces right");
        assert_eq!(p.pick_turn(true).turns, 0, "the one that is on switches off");
        let both = p.toggle_mirror(true).toggle_mirror(false);
        assert!(both.mirror_horizontal && both.mirror_vertical && both.turns == 1);
        assert!(!both.toggle_mirror(true).mirror_horizontal);
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
        fn normalizing_keeps_the_picture(p in arb_params()) {
            prop_assert_eq!(p.normalized().orientation(), p.orientation());
            prop_assert!(p.normalized().turns != 2 && p.normalized().turns < 4);
            prop_assert!(p.pick_turn(true).turns != 2 && p.toggle_mirror(false).turns != 2);
        }

        #[test]
        fn values_followed_by_values_make_both_transforms(a in arb_params(), b in arb_params()) {
            prop_assert_eq!(a.then(b).orientation(), a.orientation().then(b.orientation()));
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
