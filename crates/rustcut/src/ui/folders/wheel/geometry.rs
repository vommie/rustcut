//! The folder wheel's size and drawing geometry without Slint (P11.5, user decision Q-FP2): "Automatic" is 40 % of the
//! main window's shorter side, never below a size that is comfortable to hit, nearly the whole window when it is
//! small; no fixed pixels besides that minimum (4K-ready: Slint's logical pixels scale with the screen). The segments
//! are SVG path commands Slint's `Path` draws.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write as _;

use rustcut_core::settings::WheelSize;

/// A count as `f32` (counts here are places and indexes: far below 65 536).
fn count(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// The smallest comfortable diameter (logical pixels).
pub const MIN_DIAMETER: f32 = 300.0;
/// The share of the window a wheel never exceeds.
const MAX_SHARE: f32 = 0.94;

/// Automatic: 40 % of the shorter side from this size on (a large window) ...
const AUTO_LARGE: f32 = 1200.0;
/// ... growing to 90 % at this size and below (a small window: the wheel may cover the panels, not only the video).
const AUTO_SMALL: f32 = 600.0;

/// The wheel's diameter in a main window of `width` x `height` (logical pixels); it is not bound to the video's area.
pub fn diameter(size: WheelSize, width: f32, height: f32) -> f32 {
    let short = width.min(height).max(1.0);
    let share = match size {
        WheelSize::Automatic => {
            let small = ((AUTO_LARGE - short) / (AUTO_LARGE - AUTO_SMALL)).clamp(0.0, 1.0);
            0.5f32.mul_add(small, 0.4)
        }
        WheelSize::Small => 0.4,
        WheelSize::Medium => 0.6,
        WheelSize::Large => 0.8,
    };
    let wanted = (short * share).max(MIN_DIAMETER);
    wanted.min(short * MAX_SHARE)
}

/// The middle's and the places' outer radius of a wheel of `diameter`, the middle in percent of the outer radius (the
/// outer edge leaves room for the group marks).
pub fn radii(diameter: f32, middle_size: u8) -> (f32, f32) {
    let outer = diameter / 2.0 * 0.9;
    (outer * f32::from(middle_size) / 100.0, outer)
}

/// A length given in tenths of a percent of the wheel's diameter, in pixels (P12: the wheel grows with the window, so
/// what is set stays the same look at every size).
pub fn share(diameter: f32, tenths: u16) -> f32 {
    diameter * f32::from(tenths) / 1000.0
}

/// A line's width in tenths of a percent of the diameter, on half pixels and never below one pixel.
pub fn line(diameter: f32, tenths: u16) -> f32 {
    (share(diameter, tenths) * 2.0).round().max(2.0) / 2.0
}

/// The angle (radians, clockwise from the top) of place `index` of `n`.
pub fn angle(index: usize, n: usize) -> f32 {
    count(index) * TAU / count(n.max(1))
}

/// A point at `radius` and `angle` (clockwise from the top) around (cx, cy).
pub fn at(cx: f32, cy: f32, radius: f32, angle: f32) -> (f32, f32) {
    let a = angle - FRAC_PI_2;
    (cx + radius * a.cos(), cy + radius * a.sin())
}

/// A wheel's places as shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub cx: f32,
    pub cy: f32,
    pub inner: f32,
    pub outer: f32,
    /// The gap between two places (the edges run parallel, so it is this wide everywhere).
    pub gap: f32,
    /// How round the corners are.
    pub corner: f32,
}

impl Face {
    /// Half of a place's angle at `radius`.
    fn half(&self, step: f32, radius: f32) -> f32 {
        (step / 2.0 - (self.gap / 2.0 / radius.max(1.0)).min(0.98).asin()).max(0.001)
    }

    /// The same places at `scale` (physical pixels of a picture's mask).
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            cx: self.cx * scale,
            cy: self.cy * scale,
            inner: self.inner * scale,
            outer: self.outer * scale,
            gap: self.gap * scale,
            corner: self.corner * scale,
        }
    }

    /// Place `index` of `n` as SVG path commands.
    pub fn spoke(&self, index: usize, n: usize) -> String {
        let step = TAU / count(n.max(1));
        let mid = angle(index, n);
        let (inner, outer) = (self.inner, self.outer);
        let (ho, hi) = if n <= 1 {
            (PI - 0.0001, PI - 0.0001)
        } else {
            (self.half(step, outer), self.half(step, inner))
        };
        let large_o = u8::from(ho * 2.0 > PI);
        let large_i = u8::from(hi * 2.0 > PI);
        let rc = self
            .corner
            .min((outer - inner) / 2.2)
            .min(outer * ho * 0.9)
            .min(inner * hi * 0.9);
        let p = |r: f32, a: f32| {
            let (x, y) = at(self.cx, self.cy, r, a);
            format!("{x:.2} {y:.2}")
        };
        let mut d = String::new();
        if rc < 0.05 {
            let _ = write!(
                d,
                "M {} A {outer:.2} {outer:.2} 0 {large_o} 1 {} L {} A {inner:.2} {inner:.2} 0 {large_i} 0 {} Z",
                p(outer, mid - ho),
                p(outer, mid + ho),
                p(inner, mid + hi),
                p(inner, mid - hi),
            );
            return d;
        }
        let (d_o, d_i) = (rc / outer, rc / inner);
        let _ = write!(
            d,
            "M {} Q {} {} A {outer:.2} {outer:.2} 0 {large_o} 1 {} Q {} {} L {} Q {} {} \
             A {inner:.2} {inner:.2} 0 {large_i} 0 {} Q {} {} Z",
            p(outer - rc, mid - ho),
            p(outer, mid - ho),
            p(outer, mid - ho + d_o),
            p(outer, mid + ho - d_o),
            p(outer, mid + ho),
            p(outer - rc, mid + ho),
            p(inner + rc, mid + hi),
            p(inner, mid + hi),
            p(inner, mid + hi - d_i),
            p(inner, mid - hi + d_i),
            p(inner, mid - hi),
            p(inner + rc, mid - hi),
        );
        d
    }

    /// The box around place `index` of `n`: left, top, width, height.
    pub fn spoke_box(&self, index: usize, n: usize) -> (f32, f32, f32, f32) {
        let step = TAU / count(n.max(1));
        let mid = angle(index, n);
        let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..=12u8 {
            let a = mid - step / 2.0 + step * f32::from(i) / 12.0;
            for r in [self.inner, self.outer] {
                let (x, y) = at(self.cx, self.cy, r, a);
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
        (left, top, right - left, bottom - top)
    }

    /// The dwell fill of place `index`: the place from the inner edge outwards to `progress` (0..1).
    pub fn fill(&self, index: usize, n: usize, progress: f32) -> String {
        if progress <= 0.0 {
            return String::new();
        }
        Self {
            outer: self.inner + (self.outer - self.inner) * progress.min(1.0),
            corner: 0.0,
            ..*self
        }
        .spoke(index, n)
    }
}

/// An open arc (a stroke) at `radius` from a0 to a1: the hold ring and the middle's progress.
pub fn arc(c: f32, radius: f32, a0: f32, a1: f32) -> String {
    let a1 = a1.min(a0 + TAU - 0.0001);
    let large = u8::from(a1 - a0 > PI);
    let (x0, y0) = at(c, c, radius, a0);
    let (x1, y1) = at(c, c, radius, a1);
    let mut d = String::new();
    let _ = write!(
        d,
        "M {x0:.2} {y0:.2} A {radius:.2} {radius:.2} 0 {large} 1 {x1:.2} {y1:.2}"
    );
    d
}

/// The mark outside a group's place.
pub fn group_mark(c: f32, outer: f32, index: usize, n: usize) -> String {
    let step = TAU / count(n.max(1));
    let mid = angle(index, n);
    arc(
        c,
        outer + outer * 0.05,
        mid - step / 2.0 + 0.12,
        mid + step / 2.0 - 0.12,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_grows_in_small_windows() {
        let auto = |w, h| diameter(WheelSize::Automatic, w, h);
        assert!((auto(3840.0, 2160.0) - 864.0).abs() < 0.5, "40 % of a large window");
        assert!((auto(1066.0, 600.0) - 540.0).abs() < 0.5, "90 % of a small one");
        assert!((auto(1280.0, 900.0) - 0.65 * 900.0).abs() < 0.5, "in between");
        assert!(
            (auto(400.0, 300.0) - 300.0 * MAX_SHARE).abs() < 0.5,
            "never beyond the window"
        );
        assert!(
            (diameter(WheelSize::Large, 1066.0, 625.0) - 500.0).abs() < 0.5,
            "Large: 80 %"
        );
        assert!((diameter(WheelSize::Small, 3840.0, 2160.0) - 864.0).abs() < 0.5);
        assert!(diameter(WheelSize::Small, 1000.0, 700.0) >= MIN_DIAMETER);
        let (inner, outer) = radii(500.0, 34);
        assert!(inner < outer && outer < 250.0);
        assert!((share(520.0, 10) - 5.2).abs() < 0.01);
        assert!((line(520.0, 4) - 2.0).abs() < 0.01, "0.4 % of 520 px");
        assert!((line(100.0, 4) - 1.0).abs() < 0.01, "never below one pixel");
    }

    #[test]
    fn a_gap_is_equally_wide_inside_and_outside() {
        let face = Face {
            cx: 100.0,
            cy: 100.0,
            inner: 30.0,
            outer: 90.0,
            gap: 10.0,
            corner: 0.0,
        };
        let step = TAU / 8.0;
        let width = |r: f32| 2.0 * r * ((step / 2.0 - face.half(step, r)).sin());
        assert!((width(face.outer) - 10.0).abs() < 0.01 && (width(face.inner) - 10.0).abs() < 0.01);
        assert!(face.spoke(0, 8).starts_with('M'));
        assert!(face.fill(0, 8, 0.0).is_empty());
        assert!(Face { corner: 6.0, ..face }.spoke(0, 8).contains('Q'), "round corners");
        let (x, y, w, h) = face.spoke_box(0, 8);
        assert!(x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0 && y + h <= 100.0);
    }
}
