//! Where a tooltip bubble goes (user decision Q-H3): below its element and centered on it when it can be; near the
//! window's left or right edge it shifts so it stays inside, and its arrow keeps pointing at the element. Without
//! room below it goes above (the arrow then points down), without room above and below beside the element (the arrow
//! points sideways), and as a last resort it lies over the element inside the window, without an arrow. A row of a
//! menu asks for `beside` first, so the bubble never covers the other rows. Pure, so every edge case is tested;
//! `TooltipLayer` calls it through `TooltipLogic.place` in a binding, so the bubble is placed again whenever its size,
//! the window or the element's rectangle changes.

/// Kept free at the window's edges.
pub const MARGIN: f32 = 6.0;
/// Between the element and the tip of the arrow.
pub const GAP: f32 = 2.0;
/// How far the arrow sticks out of the bubble (`tooltip_layer.slint` draws it).
pub const ARROW: f32 = 7.0;
/// Half the width of the arrow's base.
pub const ARROW_HALF: f32 = 7.0;
/// The bubble's corner radius: the arrow never sits on a corner.
pub const RADIUS: f32 = 6.0;

/// A rectangle in the layer's coordinates (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.width
    }

    fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Which side of the element the bubble is on (its arrow is on the opposite edge of the bubble).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
    Right,
    Left,
    /// Nowhere fits: inside the window over the element, without an arrow.
    Over,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The bubble's top left corner.
    pub x: f32,
    pub y: f32,
    pub side: Side,
    /// The arrow's center along the bubble's edge that faces the element: from the bubble's left for `Below`/`Above`,
    /// from its top for `Right`/`Left`.
    pub arrow: f32,
}

/// The place of a `width` x `height` bubble for `anchor` in an `area_width` x `area_height` window; `beside` tries the
/// right and the left side before below and above. On whole pixels, so the arrow's edges meet the border line exactly
/// (review 15 #10, P9.2).
pub fn place(anchor: Rect, width: f32, height: f32, area_width: f32, area_height: f32, beside: bool) -> Placement {
    let p = place_exact(anchor, width, height, area_width, area_height, beside);
    Placement {
        x: p.x.round(),
        y: p.y.round(),
        arrow: p.arrow.round(),
        ..p
    }
}

fn place_exact(anchor: Rect, width: f32, height: f32, area_width: f32, area_height: f32, beside: bool) -> Placement {
    let width = finite(width).max(0.0);
    let height = finite(height).max(0.0);
    let area_width = finite(area_width).max(0.0);
    let area_height = finite(area_height).max(0.0);
    let anchor = visible_part(anchor, area_width, area_height);
    let reach = GAP + ARROW;
    let room_below = area_height - MARGIN - anchor.bottom();
    let room_above = anchor.y - MARGIN;
    let room_right = area_width - MARGIN - anchor.right();
    let room_left = anchor.x - MARGIN;
    let center_x = anchor.x + anchor.width / 2.0;
    let center_y = anchor.y + anchor.height / 2.0;
    let across = |side, y: f32| {
        let x = clamp_span(center_x - width / 2.0, width, area_width);
        Placement {
            x,
            y,
            side,
            arrow: arrow_at(center_x - x, width),
        }
    };
    let to_side = |side, x: f32| {
        let y = clamp_span(center_y - height / 2.0, height, area_height);
        Placement {
            x,
            y,
            side,
            arrow: arrow_at(center_y - y, height),
        }
    };
    let fits = |side| match side {
        Side::Below => room_below >= height + reach,
        Side::Above => room_above >= height + reach,
        Side::Right => room_right >= width + reach,
        Side::Left => room_left >= width + reach,
        Side::Over => true,
    };
    let order = if beside {
        [Side::Right, Side::Left, Side::Below, Side::Above]
    } else {
        [Side::Below, Side::Above, Side::Right, Side::Left]
    };
    match order.into_iter().find(|side| fits(*side)) {
        Some(Side::Below) => across(Side::Below, anchor.bottom() + reach),
        Some(Side::Above) => across(Side::Above, anchor.y - reach - height),
        Some(Side::Right) => to_side(Side::Right, anchor.right() + reach),
        Some(Side::Left) => to_side(Side::Left, anchor.x - reach - width),
        Some(Side::Over) | None => {
            let want = if room_below >= room_above {
                anchor.bottom() + reach
            } else {
                anchor.y - reach - height
            };
            Placement {
                x: clamp_span(center_x - width / 2.0, width, area_width),
                y: clamp_span(want, height, area_height),
                side: Side::Over,
                arrow: width / 2.0,
            }
        }
    }
}

/// `want` moved so a span of `size` stays inside `0..room` with the margin; a span too big for that is centered (and
/// starts at 0 when it is bigger than the room itself).
fn clamp_span(want: f32, size: f32, room: f32) -> f32 {
    let lo = MARGIN;
    let hi = room - MARGIN - size;
    if hi < lo {
        ((room - size) / 2.0).max(0.0)
    } else {
        want.clamp(lo, hi)
    }
}

/// The arrow's center at `want` along an edge of `size`, kept off the rounded corners.
fn arrow_at(want: f32, size: f32) -> f32 {
    let lo = RADIUS + ARROW_HALF;
    let hi = size - RADIUS - ARROW_HALF;
    if hi < lo { size / 2.0 } else { want.clamp(lo, hi) }
}

/// The part of `anchor` inside the window (an element scrolled half out points at its visible half); an element
/// completely outside shrinks to the nearest edge.
fn visible_part(anchor: Rect, area_width: f32, area_height: f32) -> Rect {
    let x0 = finite(anchor.x).clamp(0.0, area_width);
    let y0 = finite(anchor.y).clamp(0.0, area_height);
    let x1 = finite(anchor.right()).clamp(x0, area_width);
    let y1 = finite(anchor.bottom()).clamp(y0, area_height);
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// Layout passes can hand over NaN or infinities for an element that is not laid out yet.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
