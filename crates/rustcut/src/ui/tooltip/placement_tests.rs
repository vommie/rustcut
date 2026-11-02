//! Tests of the bubble placement (`placement.rs`): every side, the shifts at the window's edges, the arrow, the last
//! resort, odd input, and properties over random rectangles.

use proptest::prelude::*;

use super::placement::{ARROW, ARROW_HALF, GAP, MARGIN, Placement, RADIUS, Rect, Side, clip_rows, place};

const W: f32 = 1280.0;
const H: f32 = 800.0;

fn button(x: f32, y: f32) -> Rect {
    Rect {
        x,
        y,
        width: 26.0,
        height: 24.0,
    }
}

fn center_x(r: Rect) -> f32 {
    r.x + r.width / 2.0
}

/// Where the arrow's tip points in window coordinates.
fn arrow_x(p: &Placement) -> f32 {
    p.x + p.arrow
}

#[test]
fn a_bubble_with_room_is_centered_below_its_element() {
    let b = button(600.0, 100.0);
    let p = place(b, 120.0, 40.0, W, H, false);
    assert_eq!(p.side, Side::Below);
    assert!((p.x + 60.0 - center_x(b)).abs() < 0.01, "centered: {p:?}");
    assert!((p.y - (b.y + b.height + GAP + ARROW)).abs() < 0.01);
    assert!(
        (arrow_x(&p) - center_x(b)).abs() < 0.01,
        "the arrow points at the middle"
    );
}

#[test]
fn near_the_left_edge_the_bubble_starts_at_the_margin_and_the_arrow_still_points_at_the_element() {
    let b = button(8.0, 100.0);
    let p = place(b, 200.0, 40.0, W, H, false);
    assert_eq!(p.side, Side::Below);
    assert!((p.x - MARGIN).abs() < 0.01, "{p:?}");
    assert!((arrow_x(&p) - center_x(b)).abs() < 0.01, "{p:?}");
}

#[test]
fn near_the_right_edge_the_bubble_ends_at_the_margin() {
    let b = button(W - 30.0, 100.0);
    let p = place(b, 200.0, 40.0, W, H, false);
    assert!((p.x + 200.0 - (W - MARGIN)).abs() < 0.01, "{p:?}");
    // The arrow keeps off the rounded corner, so it points a pixel left of the middle, still into the button.
    assert!(arrow_x(&p) > b.x && arrow_x(&p) < b.x + b.width, "{p:?}");
}

#[test]
fn the_arrow_stays_off_the_rounded_corners() {
    // An element in the window's corner: its middle is left of where the bubble can start.
    let b = Rect {
        x: 0.0,
        y: 50.0,
        width: 8.0,
        height: 20.0,
    };
    let p = place(b, 200.0, 40.0, W, H, false);
    assert!((p.arrow - (RADIUS + ARROW_HALF)).abs() < 0.01, "{p:?}");
}

#[test]
fn without_room_below_the_bubble_goes_above_with_the_arrow_on_its_bottom_edge() {
    // The transport bar at the window's bottom.
    let b = button(40.0, H - 34.0);
    let p = place(b, 160.0, 50.0, W, H, false);
    assert_eq!(p.side, Side::Above);
    assert!((p.y + 50.0 - (b.y - GAP - ARROW)).abs() < 0.01, "{p:?}");
    assert!((arrow_x(&p) - center_x(b)).abs() < 0.01);
}

#[test]
fn a_tall_bubble_in_a_short_window_goes_beside_its_element() {
    let b = button(300.0, 140.0);
    let p = place(b, 200.0, 250.0, 900.0, 300.0, false);
    assert_eq!(p.side, Side::Right);
    assert!((p.x - (b.x + b.width + GAP + ARROW)).abs() < 0.01, "{p:?}");
    assert!(
        (p.y + p.arrow - (b.y + b.height / 2.0)).abs() < 0.01,
        "the arrow points at the middle: {p:?}"
    );
}

#[test]
fn beside_goes_left_when_the_right_is_too_narrow() {
    let b = button(700.0, 140.0);
    let p = place(b, 200.0, 250.0, 760.0, 300.0, false);
    assert_eq!(p.side, Side::Left);
    assert!((p.x + 200.0 - (b.x - GAP - ARROW)).abs() < 0.01, "{p:?}");
}

#[test]
fn when_nothing_fits_the_bubble_lies_inside_the_window_without_an_arrow() {
    let b = button(150.0, 120.0);
    let p = place(b, 280.0, 230.0, 300.0, 250.0, false);
    assert_eq!(p.side, Side::Over);
    assert!(p.x >= 0.0 && p.x + 280.0 <= 300.0, "{p:?}");
    assert!(p.y >= 0.0 && p.y + 230.0 <= 250.0, "{p:?}");
}

#[test]
fn a_bubble_bigger_than_the_window_starts_at_its_corner() {
    let p = place(button(10.0, 10.0), 500.0, 400.0, 300.0, 200.0, false);
    assert_eq!(p.side, Side::Over);
    assert!(p.x.abs() < 0.01 && p.y.abs() < 0.01, "{p:?}");
}

#[test]
fn an_element_half_outside_the_window_points_at_its_visible_part() {
    // A row scrolled half out of the right edge: the visible part is 1260..1280.
    let b = Rect {
        x: W - 20.0,
        y: 100.0,
        width: 200.0,
        height: 24.0,
    };
    let p = place(b, 100.0, 30.0, W, H, false);
    assert_eq!(p.side, Side::Below);
    assert!(arrow_x(&p) >= W - 20.0, "the arrow points into the visible part: {p:?}");
    assert!(p.x + 100.0 <= W - MARGIN + 0.01);
}

#[test]
fn odd_numbers_give_a_place_inside_the_window() {
    let b = Rect {
        x: f32::NAN,
        y: f32::INFINITY,
        width: -5.0,
        height: f32::NAN,
    };
    let p = place(b, f32::NAN, 30.0, W, H, false);
    assert!(p.x.is_finite() && p.y.is_finite() && p.arrow.is_finite(), "{p:?}");
    assert!(p.y >= 0.0 && p.y + 30.0 <= H, "{p:?}");
}

#[test]
fn a_bubble_that_grows_while_shown_flips_above_once_it_no_longer_fits_below() {
    let b = button(400.0, 600.0);
    assert_eq!(place(b, 200.0, 100.0, W, H, false).side, Side::Below);
    assert_eq!(place(b, 200.0, 200.0, W, H, false).side, Side::Above);
}

#[test]
fn a_menu_row_gets_its_bubble_beside_the_menu() {
    let row = Rect {
        x: 170.0,
        y: 736.0,
        width: 227.0,
        height: 28.0,
    };
    let p = place(row, 300.0, 44.0, W, H, true);
    assert_eq!(p.side, Side::Right);
    assert!((p.x - (row.x + row.width + GAP + ARROW)).abs() < 0.01, "{p:?}");
    assert!(p.y + 44.0 <= H - MARGIN + 0.01, "kept inside at the bottom: {p:?}");
    assert!(
        (p.y + p.arrow - (row.y + row.height / 2.0)).abs() < 0.01,
        "the arrow points at the row: {p:?}"
    );
}

#[test]
fn a_menu_row_at_the_right_edge_gets_its_bubble_on_the_left() {
    let row = Rect {
        x: 1000.0,
        y: 300.0,
        width: 227.0,
        height: 28.0,
    };
    assert_eq!(place(row, 300.0, 44.0, W, H, true).side, Side::Left);
    // No room on either side of a row as wide as the window: below as usual.
    let wide = Rect {
        x: 20.0,
        width: 1240.0,
        ..row
    };
    assert_eq!(place(wide, 300.0, 44.0, W, H, true).side, Side::Below);
}

proptest! {
    #[test]
    fn a_bubble_that_fits_the_window_never_leaves_it(
        ax in -100.0f32..1400.0, ay in -100.0f32..900.0, aw in 0.0f32..300.0, ah in 0.0f32..60.0,
        w in 0.0f32..400.0, h in 0.0f32..300.0, beside in any::<bool>(),
    ) {
        let p = place(Rect { x: ax, y: ay, width: aw, height: ah }, w, h, W, H, beside);
        // Whole pixels may move it by half a pixel into the margin.
        prop_assert!(p.x >= MARGIN - 0.51 && p.x + w <= W - MARGIN + 0.51, "{:?}", p);
        prop_assert!(p.y >= MARGIN - 0.51 && p.y + h <= H - MARGIN + 0.51, "{:?}", p);
    }

    #[test]
    fn a_bubble_and_its_arrow_stand_on_whole_pixels(
        ax in -100.0f32..1400.0, ay in -100.0f32..900.0, aw in 0.0f32..300.0, ah in 0.0f32..60.0,
        w in 0.0f32..400.0, h in 0.0f32..300.0, beside in any::<bool>(),
    ) {
        // Review 15 #10 (P9.2): the arrow's edges meet the border line only on whole pixels.
        let p = place(Rect { x: ax, y: ay, width: aw, height: ah }, w, h, W, H, beside);
        prop_assert!(p.x.fract() == 0.0 && p.y.fract() == 0.0 && p.arrow.fract() == 0.0, "{:?}", p);
    }

    #[test]
    fn a_bubble_with_an_arrow_never_covers_its_element(
        ax in 0.0f32..1250.0, ay in 0.0f32..780.0, aw in 1.0f32..30.0, ah in 1.0f32..20.0,
        w in 10.0f32..400.0, h in 10.0f32..300.0, beside in any::<bool>(),
    ) {
        let a = Rect { x: ax, y: ay, width: aw, height: ah };
        let p = place(a, w, h, W, H, beside);
        let overlaps = p.x < a.x + a.width && a.x < p.x + w && p.y < a.y + a.height && a.y < p.y + h;
        prop_assert!(p.side == Side::Over || !overlaps, "{:?}", p);
        if p.side != Side::Over {
            let edge = if matches!(p.side, Side::Below | Side::Above) { w } else { h };
            prop_assert!(p.arrow >= (RADIUS + ARROW_HALF).min(edge / 2.0) - 0.51, "{:?}", p);
            prop_assert!(p.arrow <= edge - (RADIUS + ARROW_HALF).min(edge / 2.0) + 0.51, "{:?}", p);
        }
    }
}

#[test]
fn a_row_scrolled_out_points_at_the_lists_edge() {
    let list = Rect {
        x: 10.0,
        y: 100.0,
        width: 200.0,
        height: 300.0,
    };
    let row = |y| Rect {
        x: 10.0,
        y,
        width: 200.0,
        height: 26.0,
    };
    assert_eq!(clip_rows(row(150.0), list), row(150.0), "inside: unchanged");
    assert_eq!(
        clip_rows(row(90.0), list),
        Rect {
            y: 100.0,
            height: 16.0,
            ..row(0.0)
        },
        "half out: its visible half"
    );
    assert_eq!(
        clip_rows(row(20.0), list),
        Rect {
            y: 100.0,
            height: 0.0,
            ..row(0.0)
        },
        "above: the top edge"
    );
    assert_eq!(
        clip_rows(row(500.0), list),
        Rect {
            y: 400.0,
            height: 0.0,
            ..row(0.0)
        },
        "below: the bottom edge"
    );
    let p = place(clip_rows(row(500.0), list), 300.0, 200.0, 1200.0, 800.0, true);
    assert_eq!(p.side, Side::Right);
    assert_eq!(p.y + p.arrow, 400.0, "the arrow points at the bottom edge");
}
