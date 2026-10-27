//! The pointer on the timeline: pressing seeks, dragging scrubs, moves an edge or (P9.5, D-P9b) moves a section's body
//! whole; a press on a section that does not move selects it and seeks there. Dragged edges and sections snap to the
//! playhead where the drag started, the marks and other edges, else to frames (Shift: frames only); Ctrl moves a tenth
//! as far for fine work, and a readout on the ruler shows the dragged range's start, end and length. The wheel zooms
//! around the pointer (Shift or a horizontal wheel scrolls), the overview strip moves the visible window.

use std::rc::Rc;

use rustcut_core::edit::{Section, SectionId};
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

use super::Editor;
use crate::MainWindow;
use crate::Timeline;
use crate::ui::sections::{Edge, EdgeOwner};
use crate::ui::timeline::hit::{self, Area, Hit};
use crate::ui::timeline::view;

/// One mouse wheel notch in Slint's `delta-y` (see `player::WHEEL_NOTCH`).
const WHEEL_NOTCH: f32 = 120.0;
/// Zoom factor per wheel notch.
const WHEEL_ZOOM: f64 = 1.25;
/// How far a pointer moves with Ctrl held, relative to the hand (D-P9b).
const FINE: f32 = 0.1;
/// A press on a section's body becomes a move after this many pixels.
const MOVE_AFTER: f32 = 3.0;

/// The pointer's x as a drag uses it: with Ctrl held every move counts a tenth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Fine {
    last: f32,
    x: f32,
}

impl Fine {
    const fn new(x: f32) -> Self {
        Self { last: x, x }
    }

    /// The pointer moved to `x`: where the drag is now.
    fn follow(&mut self, x: f32, slow: bool) -> f32 {
        let dx = x - self.last;
        self.last = x;
        self.x += if slow { dx * FINE } else { dx };
        self.x
    }
}

/// A press on the timeline that is still held; `anchor` is the playhead when it started (a snap target that does not
/// follow the drag).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Drag {
    /// Scrubbing: the playhead follows the pointer.
    Scrub { fine: Fine },
    /// Moving an edge; `offset` is where the pointer grabbed it, relative to the edge.
    Edge {
        edge: Edge,
        offset: f32,
        fine: Fine,
        anchor: Timestamp,
    },
    /// Pressed on a section's body: it moves once the pointer moved (`moved`), else the release selects and seeks.
    Body {
        id: SectionId,
        /// The pointer's time minus the section's start at the press.
        grab: Timestamp,
        press: f32,
        moved: bool,
        fine: Fine,
        anchor: Timestamp,
    },
}

impl Drag {
    /// What the readout on the ruler shows: the dragged section's or the marks' range.
    const fn shown(&self) -> Option<EdgeOwner> {
        match self {
            Self::Edge { edge, .. } => Some(edge.owner),
            Self::Body { id, moved: true, .. } => Some(EdgeOwner::Section(*id)),
            Self::Scrub { .. } | Self::Body { .. } => None,
        }
    }
}

/// The pointer's shape over the timeline (`timeline_view.slint` maps the number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cursor {
    Default,
    /// On an edge that can be dragged.
    Resize,
    /// On a section that can be moved.
    Grab,
    /// A section is being moved.
    Grabbing,
}

impl Cursor {
    const fn number(self) -> i32 {
        match self {
            Self::Default => 0,
            Self::Resize => 1,
            Self::Grab => 2,
            Self::Grabbing => 3,
        }
    }
}

pub(super) fn connect(editor: &Rc<Editor>, window: &MainWindow) {
    let tl = window.global::<Timeline<'_>>();
    let this = Rc::downgrade(editor);
    tl.on_resized(move |width| {
        if let Some(editor) = this.upgrade() {
            editor.state.borrow_mut().viewport.set_width(width);
            editor.publish();
        }
    });
    let this = Rc::downgrade(editor);
    tl.on_pointer_down(move |x, ruler| {
        if let Some(editor) = this.upgrade() {
            editor.pointer_down(x, area(ruler));
        }
    });
    let this = Rc::downgrade(editor);
    tl.on_pointer_moved(move |x, ruler, pressed, shift, control| {
        if let Some(editor) = this.upgrade() {
            editor.pointer_moved(x, area(ruler), pressed, shift, control);
        }
    });
    let this = Rc::downgrade(editor);
    tl.on_pointer_up(move |x| {
        if let Some(editor) = this.upgrade() {
            editor.pointer_up(x);
        }
    });
    let this = Rc::downgrade(editor);
    tl.on_pointer_left(move || {
        if let Some(editor) = this.upgrade() {
            let changed = {
                let mut st = editor.state.borrow_mut();
                st.hover = None;
                if st.drag.is_none() {
                    st.cursor = Cursor::Default;
                }
                st.hovered.take().is_some() && st.drag.is_none()
            };
            if changed {
                editor.publish();
            } else {
                editor.publish_hover();
            }
        }
    });
    let this = Rc::downgrade(editor);
    tl.on_wheel(move |x, dx, dy, _control, shift| this.upgrade().is_some_and(|editor| editor.wheel(x, dx, dy, shift)));
    let this = Rc::downgrade(editor);
    tl.on_overview(move |fraction| {
        if let Some(editor) = this.upgrade() {
            editor.state.borrow_mut().viewport.center_on_fraction(fraction);
            editor.publish();
        }
    });
}

fn area(ruler: bool) -> Area {
    if ruler { Area::Ruler } else { Area::Track }
}

impl Editor {
    fn pointer_down(&self, x: f32, area: Area) {
        let target = {
            let mut st = self.state.borrow_mut();
            if !st.has_video || st.viewport.is_empty() {
                return;
            }
            st.sent = None;
            let anchor = st.playhead;
            match hit::hit(&st.viewport, &st.sections, area, x) {
                Hit::Edge(edge) => {
                    let at = st.sections.edge_time(edge);
                    let offset = at.map_or(0.0, |t| x - st.viewport.x_of(t));
                    st.drag = Some(Drag::Edge {
                        edge,
                        offset,
                        fine: Fine::new(x),
                        anchor,
                    });
                    if let EdgeOwner::Section(id) = edge.owner
                        && !st.sections.is_selected(id)
                    {
                        st.sections.select(Some(id));
                    }
                    at
                }
                Hit::Section(id) => {
                    let start = st.sections.list().get(id).map_or(Timestamp::ZERO, Section::start);
                    st.drag = Some(Drag::Body {
                        id,
                        grab: st.viewport.time_at(x) - start,
                        press: x,
                        moved: false,
                        fine: Fine::new(x),
                        anchor,
                    });
                    if !st.sections.is_selected(id) {
                        st.sections.select(Some(id));
                    }
                    None
                }
                Hit::Empty => {
                    st.drag = Some(Drag::Scrub { fine: Fine::new(x) });
                    Some(st.viewport.time_at(x))
                }
            }
        };
        if let Some(t) = target {
            self.pointer_seek(t);
        }
        self.publish();
    }

    /// A seek of the timeline's pointer: always to the exact frame (as PyCutRv and mpv's `hr-seek=yes` do; a keyframe
    /// first made a press or a drag's end jump twice), and never twice to the same time in one press. A drag sends
    /// many; the player's command thread runs only the newest of a burst.
    fn pointer_seek(&self, t: Timestamp) {
        if self.state.borrow_mut().sent.replace(t) == Some(t) {
            return;
        }
        self.with_player(|p| p.seek_to(t, true));
    }

    fn pointer_moved(&self, x: f32, area: Area, pressed: bool, shift: bool, control: bool) {
        let rate = self.frame_rate();
        let gap = hit::min_gap(rate);
        let (seek, layout_changed) = {
            let mut guard = self.state.borrow_mut();
            let st = &mut *guard;
            st.hover = Some(x);
            let mut drag = st.drag.filter(|_| pressed);
            let result = match &mut drag {
                Some(Drag::Scrub { fine }) => {
                    st.cursor = Cursor::Default;
                    (Some(st.viewport.time_at(fine.follow(x, control))), false)
                }
                Some(Drag::Edge {
                    edge,
                    offset,
                    fine,
                    anchor,
                }) => {
                    st.cursor = Cursor::Resize;
                    let raw = st.viewport.time_at(fine.follow(x, control) - *offset);
                    let t = hit::snap(&st.viewport, &st.sections, *edge, raw, Some(*anchor), rate, !shift);
                    st.sections.drag_edge(*edge, t, gap);
                    (st.sections.edge_time(*edge), true)
                }
                Some(Drag::Body {
                    id,
                    grab,
                    press,
                    moved,
                    fine,
                    anchor,
                }) => {
                    let at = fine.follow(x, control);
                    *moved = *moved || (x - *press).abs() > MOVE_AFTER;
                    if *moved {
                        st.cursor = Cursor::Grabbing;
                        let length = st.sections.list().get(*id).map(Section::duration);
                        if let Some(length) = length {
                            let raw = st.viewport.time_at(at) - *grab;
                            let (vp, s) = (&st.viewport, &st.sections);
                            let start = hit::snap_body(vp, s, *id, raw, length, Some(*anchor), rate, !shift);
                            st.sections.move_section(*id, start);
                        }
                        (st.sections.list().get(*id).map(Section::start), true)
                    } else {
                        (None, false)
                    }
                }
                None => {
                    let found = hit::hit(&st.viewport, &st.sections, area, x);
                    let (cursor, hovered) = match found {
                        Hit::Edge(edge) => (
                            Cursor::Resize,
                            match edge.owner {
                                EdgeOwner::Section(id) => Some(id),
                                EdgeOwner::Marks => None,
                            },
                        ),
                        Hit::Section(id) => (Cursor::Grab, Some(id)),
                        Hit::Empty => (Cursor::Default, None),
                    };
                    st.cursor = cursor;
                    let changed = st.hovered != hovered;
                    st.hovered = hovered;
                    (None, changed)
                }
            };
            if drag.is_some() {
                st.drag = drag;
            }
            result
        };
        if let Some(t) = seek {
            self.pointer_seek(t);
        }
        if layout_changed {
            self.publish();
        } else {
            self.publish_hover();
        }
    }

    fn pointer_up(&self, x: f32) {
        let target = {
            let mut st = self.state.borrow_mut();
            st.cursor = Cursor::Default;
            match st.drag.take() {
                Some(Drag::Scrub { mut fine }) => Some(st.viewport.time_at(fine.follow(x, false))),
                Some(Drag::Edge { edge, .. }) => st.sections.edge_time(edge),
                Some(Drag::Body { id, moved: true, .. }) => {
                    st.cursor = Cursor::Grab;
                    st.sections.list().get(id).map(Section::start)
                }
                // A click on a section: it is selected, the playhead goes where it was pressed.
                Some(Drag::Body { press, .. }) => Some(st.viewport.time_at(press)),
                None => None,
            }
        };
        // Where the drag ended, unless the player is already there.
        if let Some(t) = target {
            self.pointer_seek(t);
        }
        self.state.borrow_mut().sent = None;
        self.publish();
    }

    fn wheel(&self, x: f32, dx: f32, dy: f32, shift: bool) -> bool {
        {
            let mut st = self.state.borrow_mut();
            if !st.has_video || st.viewport.is_empty() {
                return false;
            }
            if shift || dx != 0.0 {
                let amount = if dx == 0.0 { dy } else { dx };
                st.viewport.scroll_by(-amount);
            } else if dy != 0.0 {
                st.viewport.zoom_by(WHEEL_ZOOM.powf(f64::from(dy / WHEEL_NOTCH)), x);
            } else {
                return false;
            }
        }
        self.publish();
        true
    }

    /// The hover marker, the readout of a dragged range and the pointer's shape.
    pub(super) fn publish_hover(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let tl = window.global::<Timeline<'_>>();
        let readout = st
            .drag
            .and_then(|d| d.shown())
            .filter(|_| st.has_video)
            .and_then(|owner| match owner {
                EdgeOwner::Section(id) => st
                    .sections
                    .list()
                    .get(id)
                    .and_then(|s| view::readout(&st.viewport, s.start(), Some(s.end()))),
                EdgeOwner::Marks => {
                    let (start, end) = st.sections.marks();
                    match (start, end) {
                        (Some(a), b) => view::readout(&st.viewport, a, b),
                        (None, Some(b)) => view::readout(&st.viewport, b, None),
                        (None, None) => None,
                    }
                }
            });
        tl.set_readout_shown(readout.is_some());
        if let Some((x, text)) = &readout {
            tl.set_readout_x(*x);
            tl.set_readout_text(text.as_str().into());
        }
        let hover = st
            .hover
            .filter(|_| st.has_video && readout.is_none())
            .and_then(|x| view::hover(&st.viewport, x));
        tl.set_hover_shown(hover.is_some());
        if let Some((x, text)) = hover {
            tl.set_hover_x(x);
            tl.set_hover_text(text.into());
        }
        tl.set_cursor(st.cursor.number());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_moves_a_tenth_as_far() {
        let mut fine = Fine::new(100.0);
        assert!((fine.follow(110.0, false) - 110.0).abs() < 1e-4);
        assert!(
            (fine.follow(160.0, true) - 115.0).abs() < 1e-4,
            "50 px with Ctrl count 5"
        );
        assert!(
            (fine.follow(170.0, false) - 125.0).abs() < 1e-4,
            "without Ctrl again at full speed"
        );
    }
}
