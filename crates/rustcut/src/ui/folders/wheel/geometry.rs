//! The folder wheel's size and drawing geometry without Slint (P11.5, user decision Q-FP2): "Automatic" is 40 % of the
//! main window's shorter side, never below a size that is comfortable to hit, nearly the whole window when it is
//! small; no fixed pixels besides that minimum (4K-ready: Slint's logical pixels scale with the screen). The segments
//! are SVG path commands Slint's `Path` draws.

use std::f32::consts::{FRAC_PI_2, TAU};
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

/// The inner (middle) and outer radius of a wheel of `diameter` (the outer edge leaves room for the group marks).
pub fn radii(diameter: f32) -> (f32, f32) {
    let outer = diameter / 2.0 * 0.9;
    (outer * 0.34, outer)
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

/// The gap between two places (radians).
const GAP: f32 = 0.025;

/// The ring segment of place `index` of `n` between `inner` and `outer`, around (c, c), as SVG path commands.
pub fn segment(c: f32, inner: f32, outer: f32, index: usize, n: usize) -> String {
    let step = TAU / count(n.max(1));
    let mid = angle(index, n);
    let (a0, a1) = if n == 1 {
        (0.0, TAU - 0.0001)
    } else {
        (mid - step / 2.0 + GAP, mid + step / 2.0 - GAP)
    };
    ring_part(c, inner, outer, a0, a1)
}

/// A ring part from angle a0 to a1 (clockwise from the top).
pub fn ring_part(c: f32, inner: f32, outer: f32, a0: f32, a1: f32) -> String {
    let large = u8::from(a1 - a0 > std::f32::consts::PI);
    let (x0, y0) = at(c, c, outer, a0);
    let (x1, y1) = at(c, c, outer, a1);
    let (x2, y2) = at(c, c, inner, a1);
    let (x3, y3) = at(c, c, inner, a0);
    let mut d = String::new();
    let _ = write!(
        d,
        "M {x0:.2} {y0:.2} A {outer:.2} {outer:.2} 0 {large} 1 {x1:.2} {y1:.2} L {x2:.2} {y2:.2} \
         A {inner:.2} {inner:.2} 0 {large} 0 {x3:.2} {y3:.2} Z"
    );
    d
}

/// The dwell fill of place `index`: the segment from the inner edge outwards to `progress` (0..1).
pub fn fill(c: f32, inner: f32, outer: f32, index: usize, n: usize, progress: f32) -> String {
    if progress <= 0.0 {
        return String::new();
    }
    segment(c, inner, inner + (outer - inner) * progress.min(1.0), index, n)
}

/// An open arc (a stroke) at `radius` from a0 to a1: the hold ring and the middle's progress.
pub fn arc(c: f32, radius: f32, a0: f32, a1: f32) -> String {
    let a1 = a1.min(a0 + TAU - 0.0001);
    let large = u8::from(a1 - a0 > std::f32::consts::PI);
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
        let (inner, outer) = radii(500.0);
        assert!(inner < outer && outer < 250.0);
        assert!(segment(100.0, 30.0, 90.0, 0, 8).starts_with('M'));
        assert!(fill(100.0, 30.0, 90.0, 0, 8, 0.0).is_empty());
    }
}
