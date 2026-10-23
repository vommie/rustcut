//! What the pointer is over on the track, and where a dragged edge snaps to. Uses the same [`Viewport`] as the drawing.

use rustcut_core::edit::SectionId;
use rustcut_core::time::{FrameRate, Timestamp};

use super::viewport::Viewport;
use crate::ui::sections::{Bound, Edge, EdgeOwner, SectionsState};

/// An edge within this many pixels of the pointer can be grabbed.
pub const GRAB: f32 = 6.0;
/// A dragged edge within this many pixels of the playhead or another edge jumps onto it.
pub const SNAP: f32 = 8.0;

/// Which part of the timeline a y position is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// The time ruler: pressing scrubs, edges cannot be grabbed there.
    Ruler,
    /// The section track.
    Track,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Edge(Edge),
    /// The body of a listed section.
    Section(SectionId),
    Empty,
}

/// What is under `x` in `area`: the nearest grabbable edge first (the marks before the selected section before the
/// others, then the later list entry, which is drawn on top), else the body of a section (the selected one first).
pub fn hit(vp: &Viewport, sections: &SectionsState, area: Area, x: f32) -> Hit {
    if vp.is_empty() || area == Area::Ruler {
        return Hit::Empty;
    }
    let mut best: Option<(f32, u8, Edge)> = None;
    let mut consider = |t: Option<Timestamp>, edge: Edge, rank: u8| {
        let Some(t) = t else { return };
        let distance = (vp.x_of(t) - x).abs();
        if distance > GRAB {
            return;
        }
        let better = best.is_none_or(|(d, r, _)| distance < d - 0.01 || ((distance - d).abs() <= 0.01 && rank < r));
        if better {
            best = Some((distance, rank, edge));
        }
    };
    let (start, end) = sections.marks();
    for (t, bound) in [(start, Bound::Start), (end, Bound::End)] {
        consider(
            t,
            Edge {
                owner: EdgeOwner::Marks,
                bound,
            },
            0,
        );
    }
    let rows = sections.rows();
    let count = rows.len();
    for (i, row) in rows.iter().enumerate() {
        // Selected first, then the later rows (drawn on top of earlier ones).
        let rank = if row.selected {
            1
        } else {
            u8::try_from(count - i).map_or(u8::MAX, |r| r.saturating_add(1))
        };
        let owner = EdgeOwner::Section(row.id);
        consider(
            Some(row.start),
            Edge {
                owner,
                bound: Bound::Start,
            },
            rank,
        );
        consider(
            Some(row.end),
            Edge {
                owner,
                bound: Bound::End,
            },
            rank,
        );
    }
    if let Some((_, _, edge)) = best {
        return Hit::Edge(edge);
    }
    let t = vp.time_at(x);
    let inside = |r: &&crate::ui::sections::SectionRow| r.start <= t && t < r.end;
    rows.iter()
        .filter(inside)
        .find(|r| r.selected)
        .or_else(|| rows.iter().rev().find(inside))
        .map_or(Hit::Empty, |r| Hit::Section(r.id))
}

/// Where an edge dragged to `t` lands: onto the playhead, a mark or another section's edge within [`SNAP`] pixels
/// (`snap` off: Shift held), else onto the nearest frame start of `rate` (else the nearest millisecond).
pub fn snap(
    vp: &Viewport,
    sections: &SectionsState,
    dragged: Edge,
    t: Timestamp,
    playhead: Option<Timestamp>,
    rate: Option<FrameRate>,
    snap_to_edges: bool,
) -> Timestamp {
    if snap_to_edges {
        let x = vp.x_of(t);
        let mut targets: Vec<Timestamp> = playhead.into_iter().collect();
        let (start, end) = sections.marks();
        for (mark, bound) in [(start, Bound::Start), (end, Bound::End)] {
            if dragged
                != (Edge {
                    owner: EdgeOwner::Marks,
                    bound,
                })
            {
                targets.extend(mark);
            }
        }
        for row in sections.rows() {
            if dragged.owner != EdgeOwner::Section(row.id) {
                targets.extend([row.start, row.end]);
            }
        }
        let nearest = targets
            .into_iter()
            .map(|target| ((vp.x_of(target) - x).abs(), target))
            .filter(|(d, _)| *d <= SNAP)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, target)) = nearest {
            return target;
        }
    }
    nearest_frame(t, rate)
}

/// The start of the frame nearest to `t`, or the nearest millisecond without a frame rate.
pub fn nearest_frame(t: Timestamp, rate: Option<FrameRate>) -> Timestamp {
    let Some(rate) = rate else {
        let ms = Timestamp::MICROS_PER_MILLI;
        return Timestamp::from_micros((t.as_micros() + ms / 2).div_euclid(ms) * ms);
    };
    let down = rate.snap(t);
    let up = rate.step(t, 1);
    if t - down <= up - t { down } else { up }
}

/// The smallest length a dragged section keeps: one frame, or a millisecond without a frame rate.
pub fn min_gap(rate: Option<FrameRate>) -> Timestamp {
    rate.map_or(Timestamp::from_millis(1), FrameRate::frame_duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: i64) -> Timestamp {
        Timestamp::from_secs(s)
    }

    /// 100 s on 1000 px (10 px per second), marks at 10-20, sections 30-40 and 35-60 (the second selected).
    fn setup() -> (Viewport, SectionsState) {
        let mut s = SectionsState::default();
        s.set_duration(Some(secs(100)));
        for (a, b) in [(30, 40), (35, 60)] {
            s.set_mark(Bound::Start, secs(a));
            s.set_mark(Bound::End, secs(b));
            s.add_current().unwrap();
        }
        s.set_mark(Bound::Start, secs(10));
        s.set_mark(Bound::End, secs(20));
        (Viewport::new(secs(100), 1000.0, 1.0), s)
    }

    #[test]
    fn edges_bodies_and_empty_space() {
        let (vp, s) = setup();
        let first = s.id_at(0).unwrap();
        let second = s.id_at(1).unwrap();
        let edge = |owner, bound| Hit::Edge(Edge { owner, bound });
        assert_eq!(hit(&vp, &s, Area::Track, 103.0), edge(EdgeOwner::Marks, Bound::Start));
        assert_eq!(hit(&vp, &s, Area::Track, 198.0), edge(EdgeOwner::Marks, Bound::End));
        assert_eq!(hit(&vp, &s, Area::Ruler, 103.0), Hit::Empty);
        assert_eq!(hit(&vp, &s, Area::Track, 150.0), Hit::Empty);
        assert_eq!(
            hit(&vp, &s, Area::Track, 302.0),
            edge(EdgeOwner::Section(first), Bound::Start)
        );
        assert_eq!(
            hit(&vp, &s, Area::Track, 401.0),
            edge(EdgeOwner::Section(first), Bound::End)
        );
        // 35-40 is in both sections: the selected (second) one wins.
        assert_eq!(hit(&vp, &s, Area::Track, 380.0), Hit::Section(second));
        assert_eq!(hit(&vp, &s, Area::Track, 320.0), Hit::Section(first));
        assert_eq!(hit(&vp, &s, Area::Track, 700.0), Hit::Empty);
        assert_eq!(
            hit(&Viewport::new(Timestamp::ZERO, 1000.0, 1.0), &s, Area::Track, 100.0),
            Hit::Empty
        );
    }

    #[test]
    fn the_nearest_edge_wins_and_ties_go_to_the_marks() {
        let (vp, mut s) = setup();
        // A section edge exactly on the start mark.
        s.set_mark(Bound::Start, secs(30));
        assert_eq!(
            hit(&vp, &s, Area::Track, 300.0),
            Hit::Edge(Edge {
                owner: EdgeOwner::Marks,
                bound: Bound::Start
            })
        );
        // Between two close edges the nearer one is taken.
        let second = s.id_at(1).unwrap();
        assert_eq!(
            hit(&vp, &s, Area::Track, 353.0),
            Hit::Edge(Edge {
                owner: EdgeOwner::Section(second),
                bound: Bound::Start
            })
        );
    }

    #[test]
    fn dragged_edges_snap_to_neighbors_then_to_frames() {
        let (vp, s) = setup();
        let first = s.id_at(0).unwrap();
        let dragged = Edge {
            owner: EdgeOwner::Section(first),
            bound: Bound::End,
        };
        let rate = FrameRate::new(25, 1);
        // 5 px from the other section's start: snaps onto it.
        assert_eq!(
            snap(&vp, &s, dragged, Timestamp::from_millis(35_500), None, rate, true),
            secs(35)
        );
        // Onto the playhead.
        assert_eq!(
            snap(
                &vp,
                &s,
                dragged,
                Timestamp::from_millis(45_300),
                Some(secs(45)),
                rate,
                true
            ),
            secs(45)
        );
        // Shift held: only the frame grid (25 fps = 40 ms frames).
        assert_eq!(
            snap(&vp, &s, dragged, Timestamp::from_millis(35_515), None, rate, false),
            Timestamp::from_millis(35_520)
        );
        // Its own section's start does not attract it.
        assert_eq!(
            snap(&vp, &s, dragged, Timestamp::from_millis(30_400), None, rate, true),
            Timestamp::from_millis(30_400)
        );
    }

    #[test]
    fn frame_snapping() {
        let ntsc = FrameRate::new(30_000, 1_001);
        assert_eq!(
            nearest_frame(Timestamp::from_micros(50_000), ntsc),
            Timestamp::from_micros(33_367)
        );
        assert_eq!(
            nearest_frame(Timestamp::from_micros(51_000), ntsc),
            Timestamp::from_micros(66_734)
        );
        assert_eq!(
            nearest_frame(Timestamp::from_micros(40_000), ntsc),
            Timestamp::from_micros(33_367)
        );
        assert_eq!(
            nearest_frame(Timestamp::from_micros(1_499), None),
            Timestamp::from_millis(1)
        );
        assert_eq!(
            nearest_frame(Timestamp::from_micros(1_500), None),
            Timestamp::from_millis(2)
        );
        assert_eq!(min_gap(FrameRate::new(25, 1)), Timestamp::from_millis(40));
        assert_eq!(min_gap(None), Timestamp::from_millis(1));
    }
}
