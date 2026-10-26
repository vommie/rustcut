//! Where the crop is on the video, and how a drag changes it: the pure geometry between the frame the crop filter gets
//! (pixels) and the video area (logical pixels), through the rotations and mirrors after the crop, the pixel aspect ratio
//! and mpv's letterboxing. Slint draws exactly these numbers and sends the pointer back in video-area coordinates.
//!
//! Drags are computed in the *shown* orientation (where a handle on screen is an edge of the shown picture) from the
//! crop at the start of the drag, then turned back into the crop filter's orientation.

use rustcut_core::edit::{Crop, Orientation, Size};

use super::placement::DisplayRect;

/// A handle within this many logical pixels of the pointer can be grabbed.
pub const GRAB: f32 = 8.0;
/// Fewer shown pixels than this never stay in a direction (unless the frame itself is smaller).
pub const MIN_KEPT: u32 = 16;
/// With Ctrl held, the corner wins when the pointer is this close to the nearest edge of the other direction.
pub const CORNER_ZONE: f32 = 32.0;

/// A rectangle in logical pixels of the video area.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

/// What the pointer holds or would hold, as seen on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    /// The kept area: moves the crop without changing its size.
    Inside,
}

impl Handle {
    /// Which shown edges the handle moves: (top, right, bottom, left).
    const fn edges(self) -> (bool, bool, bool, bool) {
        match self {
            Self::TopLeft => (true, false, false, true),
            Self::Top => (true, false, false, false),
            Self::TopRight => (true, true, false, false),
            Self::Right => (false, true, false, false),
            Self::BottomRight => (false, true, true, false),
            Self::Bottom => (false, false, true, false),
            Self::BottomLeft => (false, false, true, true),
            Self::Left => (false, false, false, true),
            Self::Inside => (true, true, true, true),
        }
    }

    pub const fn is_corner(self) -> bool {
        matches!(
            self,
            Self::TopLeft | Self::TopRight | Self::BottomRight | Self::BottomLeft
        )
    }
}

/// The video area showing the crop filter's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropView {
    /// The frame the crop filter gets.
    frame: Size,
    /// The rotations and mirrors after the crop.
    orientation: Orientation,
    /// The frame as shown (after `orientation`).
    shown: Size,
    /// The pixel aspect (width / height) of the shown frame.
    sar: f64,
    /// Where the shown frame is in the video area (letterboxed like mpv).
    picture: Rect,
    /// Dragged and placed crops keep a width and height that are multiples of this (2 = even, P9.14).
    multiple: u32,
}

impl CropView {
    /// `frame` with sample aspect `sar` (pixel width / height), turned by `orientation`, fit into a `width` x `height`
    /// video area and centered (mpv's `keepaspect`). `None` while anything is empty.
    pub fn new(frame: Size, sar: f64, orientation: Orientation, width: f32, height: f32) -> Option<Self> {
        if frame.width == 0 || frame.height == 0 || !(width > 0.0 && height > 0.0) {
            return None;
        }
        let shown = orientation.size(frame);
        let sar = orientation.sample_aspect(if sar.is_finite() && sar > 0.0 { sar } else { 1.0 });
        let display_w = f64::from(shown.width) * sar;
        let display_h = f64::from(shown.height);
        let scale = (f64::from(width) / display_w).min(f64::from(height) / display_h);
        #[expect(clippy::cast_possible_truncation, reason = "view sizes are far inside f32")]
        let (w, h) = ((display_w * scale) as f32, (display_h * scale) as f32);
        Some(Self {
            frame,
            orientation,
            shown,
            sar,
            picture: Rect {
                x: (width - w) / 2.0,
                y: (height - h) / 2.0,
                width: w,
                height: h,
            },
            multiple: 2,
        })
    }

    /// Dragged and placed crops keep a width and height that are multiples of `n` (2, 4, 8, 16).
    #[must_use]
    pub fn with_multiple(mut self, n: u32) -> Self {
        self.multiple = n.max(2);
        self
    }

    /// The multiple the crop's size keeps.
    pub const fn multiple(&self) -> u32 {
        self.multiple
    }

    /// Where the whole (uncropped) frame is shown.
    pub const fn picture(&self) -> Rect {
        self.picture
    }

    /// The frame size as shown (width and height swapped after a quarter turn).
    pub const fn shown_size(&self) -> Size {
        self.shown
    }

    /// The display aspect ratio of the shown frame.
    pub fn shown_aspect(&self) -> f64 {
        f64::from(self.shown.width) * self.sar / f64::from(self.shown.height.max(1))
    }

    /// The frame the crop filter gets.
    pub const fn frame(&self) -> Size {
        self.frame
    }

    /// The shown frame in display units (a pixel is `sar` wide): what a ratio of the crop refers to.
    pub fn display_size(&self) -> (f64, f64) {
        (f64::from(self.shown.width) * self.sar, f64::from(self.shown.height))
    }

    /// One shown pixel's width in display units.
    pub const fn pixel_width(&self) -> f64 {
        self.sar
    }

    /// The pixel of the crop filter's frame under (`x`, `y`) of the video area; `None` outside the picture.
    pub fn frame_point(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let p = self.picture;
        if !p.contains(x, y) {
            return None;
        }
        let (sx, sy) = self.scale();
        let pixel = |offset: f32, scale: f32, size: u32| -> u32 {
            let v = (f64::from(offset) / f64::from(scale.max(f32::EPSILON))).floor();
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to the frame first"
            )]
            let v = v.clamp(0.0, f64::from(size.saturating_sub(1))) as u32;
            v
        };
        let (px_x, px_y) = (
            pixel(x - p.x, sx, self.shown.width),
            pixel(y - p.y, sy, self.shown.height),
        );
        // A 1x1 crop of the shown frame, turned back: its left and top edges are the pixel in the crop's frame.
        let shown = Crop::new(
            px_y,
            self.shown.width.saturating_sub(px_x + 1),
            self.shown.height.saturating_sub(px_y + 1),
            px_x,
        );
        let frame = self.frame_crop(shown);
        Some((frame.left, frame.top))
    }

    /// The kept area of `crop` in display units of the shown frame.
    pub fn display_rect(&self, crop: Crop) -> DisplayRect {
        let c = self.shown_crop(crop);
        let (w, h) = (f64::from(self.shown.width), f64::from(self.shown.height));
        let (l, t) = (f64::from(c.left), f64::from(c.top));
        let (r, b) = (w - f64::from(c.right), h - f64::from(c.bottom));
        DisplayRect {
            x: l * self.sar,
            y: t,
            width: (r - l) * self.sar,
            height: b - t,
        }
    }

    /// The crop that keeps `rect` (display units of the shown frame), on even pixels like a drag.
    pub fn crop_of_display(&self, rect: DisplayRect) -> Crop {
        let (l, r) = even_span(rect.x / self.sar, rect.right() / self.sar, self.shown.width);
        let (t, b) = even_span(rect.y, rect.bottom(), self.shown.height);
        let (l, r) = fit_span(l, r, self.shown.width, self.multiple, true, true);
        let (t, b) = fit_span(t, b, self.shown.height, self.multiple, true, true);
        let shown = Crop::new(
            t,
            self.shown.width.saturating_sub(r),
            self.shown.height.saturating_sub(b),
            l,
        )
        .with_even_size(self.shown);
        self.frame_crop(shown)
    }

    /// The crop as it looks on screen (edges of the shown frame).
    pub fn shown_crop(&self, crop: Crop) -> Crop {
        self.orientation.map_crop(crop)
    }

    /// A crop of the shown frame back in the crop filter's orientation.
    pub fn frame_crop(&self, shown: Crop) -> Crop {
        self.orientation.inverse().map_crop(shown)
    }

    fn scale(&self) -> (f32, f32) {
        (
            self.picture.width / px(self.shown.width).max(1.0),
            self.picture.height / px(self.shown.height).max(1.0),
        )
    }

    /// Where the kept area of `crop` is in the video area.
    pub fn kept_rect(&self, crop: Crop) -> Rect {
        let c = self.shown_crop(crop);
        let (sx, sy) = self.scale();
        let left = self.picture.x + px(c.left) * sx;
        let top = self.picture.y + px(c.top) * sy;
        let right = self.picture.right() - px(c.right) * sx;
        let bottom = self.picture.bottom() - px(c.bottom) * sy;
        Rect {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        }
    }

    /// The handle under `(x, y)`: a corner or an edge of the kept area within [`GRAB`] pixels, else the area itself;
    /// `None` outside.
    pub fn hit(&self, crop: Crop, x: f32, y: f32) -> Option<Handle> {
        let r = self.kept_rect(crop);
        let near = |a: f32, b: f32| (a - b).abs() <= GRAB;
        let within_x = x >= r.x - GRAB && x <= r.right() + GRAB;
        let within_y = y >= r.y - GRAB && y <= r.bottom() + GRAB;
        if !(within_x && within_y) {
            return None;
        }
        let (top, bottom, left, right) = (near(y, r.y), near(y, r.bottom()), near(x, r.x), near(x, r.right()));
        Some(match (top, right, bottom, left) {
            (true, _, _, true) => Handle::TopLeft,
            (true, true, _, _) => Handle::TopRight,
            (_, true, true, _) => Handle::BottomRight,
            (_, _, true, true) => Handle::BottomLeft,
            (true, ..) => Handle::Top,
            (_, true, ..) => Handle::Right,
            (_, _, true, _) => Handle::Bottom,
            (.., true) => Handle::Left,
            _ if r.contains(x, y) => Handle::Inside,
            _ => return None,
        })
    }

    /// With Ctrl held the pointer needs no aim: the edge of the kept area nearest to it anywhere over the picture, or a
    /// corner when the nearest edge of the other direction is within [`CORNER_ZONE`]. `None` off the picture.
    pub fn nearest(&self, crop: Crop, x: f32, y: f32) -> Option<Handle> {
        let p = self.picture;
        let grace = GRAB * 2.0;
        if x < p.x - grace || x > p.right() + grace || y < p.y - grace || y > p.bottom() + grace {
            return None;
        }
        let r = self.kept_rect(crop);
        let (d_top, d_bottom) = ((y - r.y).abs(), (y - r.bottom()).abs());
        let (d_left, d_right) = ((x - r.x).abs(), (x - r.right()).abs());
        let (vertical, dv) = if d_top <= d_bottom {
            (Handle::Top, d_top)
        } else {
            (Handle::Bottom, d_bottom)
        };
        let (horizontal, dh) = if d_left <= d_right {
            (Handle::Left, d_left)
        } else {
            (Handle::Right, d_right)
        };
        let corner = |v: Handle, h: Handle| match (v, h) {
            (Handle::Top, Handle::Left) => Handle::TopLeft,
            (Handle::Top, _) => Handle::TopRight,
            (_, Handle::Left) => Handle::BottomLeft,
            _ => Handle::BottomRight,
        };
        Some(if dv.max(dh) <= CORNER_ZONE {
            corner(vertical, horizontal)
        } else if dv <= dh {
            vertical
        } else {
            horizontal
        })
    }

    /// The crop after dragging `handle` by `(dx, dy)` logical pixels from where it was at `start`. Edges land on even
    /// shown pixels, at least [`MIN_KEPT`] pixels stay, and `aspect` (display width / height) is kept when given.
    pub fn drag(&self, start: Crop, handle: Handle, dx: f32, dy: f32, aspect: Option<f64>) -> Crop {
        let (sx, sy) = self.scale();
        let (w, h) = (f64::from(self.shown.width), f64::from(self.shown.height));
        let c = self.shown_crop(start);
        // Edges as coordinates of the shown frame: left, top, right, bottom.
        let (mut l, mut t) = (f64::from(c.left), f64::from(c.top));
        let (mut r, mut b) = (w - f64::from(c.right), h - f64::from(c.bottom));
        let start_edges = (l, t, r, b);
        let (dxp, dyp) = (f64::from(dx / sx), f64::from(dy / sy));
        let min_w = f64::from(MIN_KEPT.min(self.shown.width));
        let min_h = f64::from(MIN_KEPT.min(self.shown.height));
        if handle == Handle::Inside {
            let shift_x = dxp.max(-l).min(w - r);
            let shift_y = dyp.max(-t).min(h - b);
            (l, r, t, b) = (l + shift_x, r + shift_x, t + shift_y, b + shift_y);
        } else {
            let (top, right, bottom, left) = handle.edges();
            // `min` before `max`: a crop typed smaller than the minimum must not invert the bounds.
            if left {
                l = (l + dxp).min(r - min_w).max(0.0);
            }
            if right {
                r = (r + dxp).max(l + min_w).min(w);
            }
            if top {
                t = (t + dyp).min(b - min_h).max(0.0);
            }
            if bottom {
                b = (b + dyp).max(t + min_h).min(h);
            }
            if let Some(ratio) = aspect.filter(|a| a.is_finite() && *a > 0.0) {
                (l, t, r, b) = self.keep_aspect(handle, (l, t, r, b), ratio);
            }
        }
        let (l, r) = even_span(l, r, self.shown.width);
        let (t, b) = even_span(t, b, self.shown.height);
        // The edges that moved (Shift moves the other axis too) take up the difference to the multiple; a moved area
        // keeps its size.
        let (l, r, t, b) = if handle == Handle::Inside {
            (l, r, t, b)
        } else {
            let moved = |now: u32, then: f64| (f64::from(now) - then).abs() >= 1.0;
            let (l0, t0, r0, b0) = start_edges;
            let (l, r) = fit_span(l, r, self.shown.width, self.multiple, moved(l, l0), moved(r, r0));
            let (t, b) = fit_span(t, b, self.shown.height, self.multiple, moved(t, t0), moved(b, b0));
            (l, r, t, b)
        };
        let shown = Crop::new(
            t,
            self.shown.width.saturating_sub(r),
            self.shown.height.saturating_sub(b),
            l,
        )
        .with_even_size(self.shown);
        self.frame_crop(shown)
    }

    /// Moves the free edges of a dragged box so its display shape is `ratio`, inside the frame. A corner keeps the
    /// opposite corner and takes the larger of the two sizes the pointer suggests; an edge keeps its opposite edge and
    /// grows the other direction around its middle.
    fn keep_aspect(&self, handle: Handle, (l, t, r, b): (f64, f64, f64, f64), ratio: f64) -> (f64, f64, f64, f64) {
        let (w, h) = (f64::from(self.shown.width), f64::from(self.shown.height));
        // Pixel width per pixel height of the shown frame, so ratio = kw * px / kh.
        let px = self.sar;
        let (kw, kh) = (r - l, b - t);
        let (top, right, bottom, left) = handle.edges();
        if handle.is_corner() {
            let (mut nw, mut nh) = if kw * px / ratio >= kh {
                (kw, kw * px / ratio)
            } else {
                (kh * ratio / px, kh)
            };
            // Room from the fixed corner to the frame edges.
            let room_w = if left { r } else { w - l };
            let room_h = if top { b } else { h - t };
            let fit = (room_w / nw).min(room_h / nh).min(1.0);
            (nw, nh) = (nw * fit, nh * fit);
            let (l2, r2) = if left { (r - nw, r) } else { (l, l + nw) };
            let (t2, b2) = if top { (b - nh, b) } else { (t, t + nh) };
            (l2, t2, r2, b2)
        } else if top || bottom {
            let mut nw = kh * ratio / px;
            let mut nh = kh;
            if nw > w {
                (nw, nh) = (w, w * px / ratio);
            }
            let cx = f64::midpoint(l, r);
            let l2 = (cx - nw / 2.0).min(w - nw).max(0.0);
            let (t2, b2) = if top { (b - nh, b) } else { (t, t + nh) };
            (l2, t2, l2 + nw, b2)
        } else if left || right {
            let mut nh = kw * px / ratio;
            let mut nw = kw;
            if nh > h {
                (nw, nh) = (h * ratio / px, h);
            }
            let cy = f64::midpoint(t, b);
            let t2 = (cy - nh / 2.0).min(h - nh).max(0.0);
            let (l2, r2) = if left { (r - nw, r) } else { (l, l + nw) };
            (l2, t2, r2, t2 + nh)
        } else {
            (l, t, r, b)
        }
    }

    /// The largest crop of display shape `ratio` centered on the kept area of `crop` (choosing an aspect ratio).
    pub fn fit_aspect(&self, crop: Crop, ratio: f64) -> Crop {
        if !(ratio.is_finite() && ratio > 0.0) {
            return crop;
        }
        let (w, h) = (f64::from(self.shown.width), f64::from(self.shown.height));
        let px = self.sar;
        let c = self.shown_crop(crop);
        let (l, t) = (f64::from(c.left), f64::from(c.top));
        let (r, b) = (w - f64::from(c.right), h - f64::from(c.bottom));
        let (kw, kh) = (r - l, b - t);
        let (nw, nh) = if kw * px / ratio <= kh {
            (kw, kw * px / ratio)
        } else {
            (kh * ratio / px, kh)
        };
        let (cx, cy) = (f64::midpoint(l, r), f64::midpoint(t, b));
        let (left, right) = even_span(cx - nw / 2.0, cx + nw / 2.0, self.shown.width);
        let (top, bottom) = even_span(cy - nh / 2.0, cy + nh / 2.0, self.shown.height);
        let shown = Crop::new(
            top,
            self.shown.width.saturating_sub(right),
            self.shown.height.saturating_sub(bottom),
            left,
        )
        .with_even_size(self.shown);
        self.frame_crop(shown)
    }
}

/// A pixel count as a logical length factor (frame sizes are far below f32's exact integer range).
#[expect(clippy::cast_precision_loss, reason = "frame sizes stay below 2^24")]
fn px(n: u32) -> f32 {
    n as f32
}

/// The even span `lo..hi` of a `size`-pixel side with its length moved to the nearest multiple of `n` by the edges that
/// move (`moves_lo`, `moves_hi`; both: shared, the start staying even); growing only where there is room, else
/// shrinking. A side shorter than `n` and `n` = 2 leave the span as it is.
fn fit_span(lo: u32, hi: u32, size: u32, n: u32, moves_lo: bool, moves_hi: bool) -> (u32, u32) {
    let kept = hi.saturating_sub(lo);
    if n <= 2 || kept < n || size < n || !(moves_lo || moves_hi) {
        return (lo, hi);
    }
    let down = kept / n * n;
    let up = down + n;
    let room_lo = if moves_lo { lo } else { 0 };
    let room_hi = if moves_hi { size.saturating_sub(hi) } else { 0 };
    let target = if kept - down > n / 2 && up - kept <= room_lo + room_hi {
        up
    } else {
        down
    };
    if target == kept {
        return (lo, hi);
    }
    // The low edge takes an even share; the high edge the rest.
    let share = |d: u32, room: u32| -> u32 {
        if moves_lo && moves_hi {
            ((d / 2) & !1).min(room & !1)
        } else if moves_lo {
            d
        } else {
            0
        }
    };
    if target > kept {
        let d = target - kept;
        let first = share(d, room_lo);
        let (lo2, hi2) = (lo - first.min(lo), hi + (d - first));
        if hi2 <= size {
            return (lo2, hi2);
        }
        let shrink = kept - down;
        let first = share(shrink, shrink);
        return (lo + first, hi - (shrink - first));
    }
    let d = kept - target;
    let first = share(d, d);
    (lo + first, hi - (d - first))
}

/// The span `lo..hi` of a `size`-pixel side on even pixels, at least [`MIN_KEPT`] (an even number) long or the whole
/// side when it is shorter. The start stays even; the end may be `size` itself.
fn even_span(lo: f64, hi: f64, size: u32) -> (u32, u32) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=u32::MAX first"
    )]
    let even = |v: f64| ((v / 2.0).round() * 2.0).clamp(0.0, f64::from(u32::MAX)) as u32;
    let min = MIN_KEPT.min(size & !1);
    let lo = even(lo).min(size & !1);
    let hi = even(hi).min(size).max(lo);
    if hi - lo >= min {
        (lo, hi)
    } else if lo + min <= size {
        (lo, lo + min)
    } else {
        let lo = (size - min) & !1;
        (lo, lo + min)
    }
}
