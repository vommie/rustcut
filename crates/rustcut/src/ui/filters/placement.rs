//! Where the crop's ratio button puts the crop (pure; P9.14). The size keeps the area of the crop the user made last
//! (dragged, or where editing started), so a ratio only changes the shape: a 3:2 crop becomes a 2:3 one of the same area,
//! never a sliver of it, and it is reduced only as far as the picture needs. The position keeps to where the user put
//! the crop: an edge of the picture it touches stays touched (a crop in a corner stays in that corner), otherwise its
//! center stays. Going through the ratios and back gives the same crops again: nothing drifts.
//!
//! Ported from RustView `crates/rustview-core/src/image_edit/crop_tool/placement.rs` (read-only for RustCut). RustCut
//! works in display units of the shown frame (a pixel is `sar` wide), so a ratio means what the viewer sees also for
//! anamorphic video; `CropView` turns the result into whole, even pixels.

/// A rectangle in display units of the shown frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DisplayRect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    /// Width / height (0 for an empty rectangle).
    pub fn ratio(self) -> f64 {
        if self.height > 0.0 {
            self.width / self.height
        } else {
            0.0
        }
    }
}

/// Where a rectangle keeps to on one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    /// At the left (top) edge of the picture.
    Start,
    /// At the right (bottom) edge of the picture.
    End,
    /// Around this center.
    Center(f64),
}

impl Anchor {
    /// `tolerance`: how close to an edge counts as touching it (a pixel in display units).
    fn of(start: f64, length: f64, limit: f64, tolerance: f64) -> Self {
        let end = start + length;
        match (start <= tolerance, end >= limit - tolerance) {
            (true, true) => Self::Center(limit / 2.0),
            (true, false) => Self::Start,
            (false, true) => Self::End,
            (false, false) => Self::Center(start + length / 2.0),
        }
    }

    fn start(self, length: f64, limit: f64) -> f64 {
        let free = (limit - length).max(0.0);
        match self {
            Self::Start => 0.0,
            Self::End => free,
            Self::Center(center) => (center - length / 2.0).clamp(0.0, free),
        }
    }
}

/// The size and the anchors the ratio button works with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The area to keep (display units squared).
    area: f64,
    x: Anchor,
    y: Anchor,
}

impl Placement {
    /// The user made `rect` in a picture of `image` (width, height) display units; `tolerance` = one pixel's width in
    /// display units.
    pub fn of(rect: DisplayRect, image: (f64, f64), tolerance: f64) -> Self {
        Self {
            area: rect.width * rect.height,
            x: Anchor::of(rect.x, rect.width, image.0, tolerance),
            y: Anchor::of(rect.y, rect.height, image.1, 1.0),
        }
    }

    /// The user moved the rectangle to `rect`: the area stays, the anchors follow it.
    pub fn moved(self, rect: DisplayRect, image: (f64, f64), tolerance: f64) -> Self {
        Self {
            x: Anchor::of(rect.x, rect.width, image.0, tolerance),
            y: Anchor::of(rect.y, rect.height, image.1, 1.0),
            ..self
        }
    }

    /// The rectangle of `ratio` (display width / height): the kept area in that shape (at most as large as the picture
    /// allows), at the anchors.
    pub fn place(self, ratio: f64, image: (f64, f64)) -> DisplayRect {
        let ratio = if ratio.is_finite() && ratio > 0.0 { ratio } else { 1.0 };
        let mut width = (self.area * ratio).sqrt();
        let mut height = width / ratio;
        if width > image.0 || height > image.1 {
            // The largest rectangle of that shape the picture holds.
            (width, height) = if image.0 / image.1 > ratio {
                (image.1 * ratio, image.1)
            } else {
                (image.0, image.0 / ratio)
            };
        }
        DisplayRect {
            x: self.x.start(width, image.0),
            y: self.y.start(height, image.1),
            width,
            height,
        }
    }
}

/// The rectangle has the shape of `ratio` up to the rounding to whole (even) pixels: `slack` display units.
pub fn follows(rect: DisplayRect, ratio: f64, slack: f64) -> bool {
    ratio.is_finite()
        && ratio > 0.0
        && ((rect.width - rect.height * ratio).abs() <= slack || (rect.height - rect.width / ratio).abs() <= slack)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> DisplayRect {
        DisplayRect { x, y, width, height }
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn the_whole_picture_gives_the_largest_rectangle_in_the_middle() {
        let image = (1000.0, 800.0);
        let place = Placement::of(rect(0.0, 0.0, 1000.0, 800.0), image, 1.0);
        let square = place.place(1.0, image);
        assert!(near(square.x, 100.0) && near(square.width, 800.0) && near(square.height, 800.0));
        let wide = place.place(16.0 / 9.0, image);
        assert!(near(wide.width, 1000.0) && near(wide.height, 562.5) && near(wide.y, 118.75));
    }

    #[test]
    fn a_rectangle_keeps_its_area_and_its_center() {
        let image = (1000.0, 1000.0);
        let place = Placement::of(rect(300.0, 400.0, 300.0, 200.0), image, 1.0);
        let portrait = place.place(2.0 / 3.0, image);
        assert!(near(portrait.width, 200.0) && near(portrait.height, 300.0));
        assert!(near(portrait.x + portrait.width / 2.0, 450.0) && near(portrait.y + portrait.height / 2.0, 500.0));
    }

    #[test]
    fn a_rectangle_in_a_corner_stays_in_that_corner() {
        let image = (1000.0, 800.0);
        for (r, left, top) in [
            (rect(0.0, 0.0, 300.0, 200.0), true, true),
            (rect(700.0, 0.0, 300.0, 200.0), false, true),
            (rect(0.0, 600.0, 300.0, 200.0), true, false),
            (rect(700.0, 600.0, 300.0, 200.0), false, false),
        ] {
            let place = Placement::of(r, image, 1.0);
            for ratio in [1.0, 9.0 / 16.0, 21.0 / 9.0] {
                let p = place.place(ratio, image);
                assert_eq!(near(p.x, 0.0), left, "{r:?} -> {p:?}");
                assert_eq!(near(p.right(), 1000.0), !left, "{r:?} -> {p:?}");
                assert_eq!(near(p.y, 0.0), top, "{r:?} -> {p:?}");
                assert_eq!(near(p.bottom(), 800.0), !top, "{r:?} -> {p:?}");
            }
        }
    }

    #[test]
    fn a_shape_the_picture_cannot_hold_shrinks_only_as_far_as_needed() {
        let image = (1000.0, 400.0);
        let place = Placement::of(rect(100.0, 50.0, 600.0, 300.0), image, 1.0);
        let tall = place.place(0.5, image);
        assert!(
            near(tall.width, 200.0) && near(tall.height, 400.0),
            "the full height, not a sliver"
        );
    }

    #[test]
    fn going_through_the_ratios_and_back_gives_the_same_rectangle_and_moving_keeps_the_area() {
        let image = (1200.0, 900.0);
        let start = rect(820.0, 40.0, 360.0, 240.0);
        let place = Placement::of(start, image, 1.0);
        let first = place.place(1.5, image);
        for ratio in [1.0, 9.0 / 16.0, 21.0 / 9.0, 0.8] {
            let p = place.place(ratio, image);
            assert!(p.x >= 0.0 && p.right() <= 1200.01 && p.y >= 0.0 && p.bottom() <= 900.01);
        }
        assert_eq!(place.place(1.5, image), first);
        assert!(near(first.x, start.x) && near(first.width, start.width) && near(first.height, start.height));
        let moved = place.moved(rect(840.0, 660.0, 360.0, 240.0), image, 1.0);
        let p = moved.place(16.0 / 9.0, image);
        assert!(near(p.right(), 1200.0) && near(p.bottom(), 900.0));
        assert!(near(p.width * p.height, 360.0 * 240.0));
    }

    #[test]
    fn a_rectangle_follows_a_ratio_up_to_the_rounding() {
        assert!(follows(rect(0.0, 0.0, 1920.0, 1080.0), 16.0 / 9.0, 2.0));
        assert!(follows(rect(0.0, 0.0, 1920.0, 1082.0), 16.0 / 9.0, 2.0));
        assert!(!follows(rect(0.0, 0.0, 1920.0, 1000.0), 16.0 / 9.0, 2.0));
        assert!(!follows(rect(0.0, 0.0, 10.0, 10.0), f64::NAN, 2.0));
    }
}
