//! What the timeline draws, in track pixels: ruler ticks, section blocks, the current section's marks and range, the
//! overview window. Built from the [`Viewport`] and the [`SectionsState`]; Slint places exactly these numbers.

use rustcut_core::time::Timestamp;

use super::ticks::{Tick, ticks};
use super::viewport::Viewport;
use crate::ui::sections::SectionsState;

/// Blocks and ranges are clipped to the track plus this margin (their rounded ends stay outside).
const CLIP: f32 = 3.0;
/// A block shows its number from this width on.
const LABEL_MIN_WIDTH: f32 = 14.0;

#[derive(Debug, Clone, PartialEq)]
pub struct BlockView {
    pub x: f32,
    pub width: f32,
    /// The section's number in the list, when it fits.
    pub label: String,
    pub selected: bool,
    pub overlapping: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimelineView {
    /// A video with a known duration is shown.
    pub active: bool,
    pub ticks: Vec<Tick>,
    pub blocks: Vec<BlockView>,
    /// Where the start and end marks are, when visible.
    pub mark_start: Option<f32>,
    pub mark_end: Option<f32>,
    /// The current section between the marks (x, width), when both are set and part of it is visible.
    pub range: Option<(f32, f32)>,
    pub zoomed: bool,
    /// The visible window on the overview strip (start, width as fractions of the duration).
    pub window: (f32, f32),
}

/// The view of the whole timeline except the playhead and the hover marker (they change more often).
pub fn build(vp: &Viewport, sections: &SectionsState) -> TimelineView {
    if vp.is_empty() {
        return TimelineView::default();
    }
    let blocks = sections
        .rows()
        .into_iter()
        .filter_map(|row| {
            let (x, width) = clip(vp, row.start, row.end)?;
            Some(BlockView {
                x,
                width,
                label: if width >= LABEL_MIN_WIDTH {
                    row.number.to_string()
                } else {
                    String::new()
                },
                selected: row.selected,
                overlapping: row.overlapping,
            })
        })
        .collect();
    let (start, end) = sections.marks();
    let range = match (start, end) {
        (Some(a), Some(b)) if a < b => clip(vp, a, b),
        _ => None,
    };
    TimelineView {
        active: true,
        ticks: ticks(vp),
        blocks,
        mark_start: start.and_then(|t| visible_x(vp, t)),
        mark_end: end.and_then(|t| visible_x(vp, t)),
        range,
        zoomed: vp.is_zoomed(),
        window: vp.window_fractions(),
    }
}

/// The x of `t` when it is on the track.
pub fn visible_x(vp: &Viewport, t: Timestamp) -> Option<f32> {
    let x = vp.x_of(t);
    (!vp.is_empty() && (-1.0..=vp.width() + 1.0).contains(&x)).then_some(x)
}

/// The hover marker: where the pointer is and the time there (`H:MM:SS.mmm`).
pub fn hover(vp: &Viewport, x: f32) -> Option<(f32, String)> {
    if vp.is_empty() {
        return None;
    }
    let x = x.clamp(0.0, vp.width());
    let t = vp.time_at(x);
    Some((vp.x_of(t), t.to_string()))
}

/// `[a, b)` as a clipped (x, width) on the track; `None` when it is not visible.
fn clip(vp: &Viewport, a: Timestamp, b: Timestamp) -> Option<(f32, f32)> {
    let (x0, x1) = (vp.x_of(a), vp.x_of(b));
    if x1 < -CLIP || x0 > vp.width() + CLIP {
        return None;
    }
    let (x0, x1) = (x0.max(-CLIP), x1.min(vp.width() + CLIP));
    Some((x0, (x1 - x0).max(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sections::Bound;

    fn secs(s: i64) -> Timestamp {
        Timestamp::from_secs(s)
    }

    fn sections() -> SectionsState {
        let mut s = SectionsState::default();
        s.set_duration(Some(secs(100)));
        for (a, b) in [(10, 20), (50, 51), (90, 100)] {
            s.set_mark(Bound::Start, secs(a));
            s.set_mark(Bound::End, secs(b));
            s.add_current().unwrap();
        }
        s.set_mark(Bound::Start, secs(30));
        s.set_mark(Bound::End, secs(40));
        s
    }

    #[test]
    fn blocks_marks_and_range_on_the_whole_video() {
        let vp = Viewport::new(secs(100), 1000.0, 1.0);
        let view = build(&vp, &sections());
        assert!(view.active && !view.zoomed);
        assert_eq!(view.blocks.len(), 3);
        assert_eq!((view.blocks[0].x, view.blocks[0].width), (100.0, 100.0));
        assert_eq!(view.blocks[0].label, "1");
        // A 10 px block is too narrow for its number.
        assert_eq!(view.blocks[1].label, "");
        assert!(view.blocks[2].selected);
        assert_eq!((view.mark_start, view.mark_end), (Some(300.0), Some(400.0)));
        assert_eq!(view.range, Some((300.0, 100.0)));
        assert!(!view.ticks.is_empty());
    }

    #[test]
    fn zoomed_views_clip_and_drop_what_is_outside() {
        let mut vp = Viewport::new(secs(100), 1000.0, 10.0);
        vp.zoom_to(10.0, 0.0);
        vp.scroll_by(1500.0); // 100 px per second: 15 s .. 25 s
        let view = build(&vp, &sections());
        assert!(view.zoomed);
        assert_eq!(view.blocks.len(), 1);
        assert_eq!((view.blocks[0].x, view.blocks[0].width), (-CLIP, 500.0 + CLIP));
        assert_eq!((view.mark_start, view.mark_end, view.range), (None, None, None));
        assert_eq!(view.window, (0.15, 0.1));
    }

    #[test]
    fn hover_text_and_empty_views() {
        let vp = Viewport::new(secs(100), 1000.0, 1.0);
        assert_eq!(hover(&vp, 123.4), Some((123.4, "0:00:12.340".to_owned())));
        assert_eq!(hover(&vp, -50.0), Some((0.0, "0:00:00.000".to_owned())));
        let empty = Viewport::new(Timestamp::ZERO, 1000.0, 1.0);
        assert_eq!(build(&empty, &sections()), TimelineView::default());
        assert_eq!(hover(&empty, 10.0), None);
        assert_eq!(visible_x(&vp, secs(150)), None);
    }
}
