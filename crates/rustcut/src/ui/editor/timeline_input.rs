//! The pointer on the timeline: pressing seeks (and selects a section), dragging scrubs or moves an edge (snapping to the
//! playhead, the marks and other edges, else to frames; Shift snaps to frames only), the wheel zooms around the pointer
//! (Shift or a horizontal wheel scrolls), the overview strip moves the visible window.

use std::rc::Rc;

use slint::ComponentHandle as _;

use super::{Drag, Editor};
use crate::MainWindow;
use crate::Timeline;
use crate::ui::sections::EdgeOwner;
use crate::ui::timeline::hit::{self, Area, Hit};

/// One mouse wheel notch in Slint's `delta-y` (see `player::WHEEL_NOTCH`).
const WHEEL_NOTCH: f32 = 120.0;
/// Zoom factor per wheel notch.
const WHEEL_ZOOM: f64 = 1.25;

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
    tl.on_pointer_moved(move |x, ruler, pressed, shift| {
        if let Some(editor) = this.upgrade() {
            editor.pointer_moved(x, area(ruler), pressed, shift);
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
            {
                let mut st = editor.state.borrow_mut();
                st.hover = None;
                st.edge_hovered = st.drag.is_some_and(|d| matches!(d, Drag::Edge { .. }));
            }
            editor.publish_hover();
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
            match hit::hit(&st.viewport, &st.sections, area, x) {
                Hit::Edge(edge) => {
                    let at = st.sections.edge_time(edge);
                    let offset = at.map_or(0.0, |t| x - st.viewport.x_of(t));
                    st.drag = Some(Drag::Edge { edge, offset });
                    if let EdgeOwner::Section(id) = edge.owner {
                        st.sections.select(Some(id));
                    }
                    at
                }
                Hit::Section(id) => {
                    st.sections.select(Some(id));
                    st.drag = Some(Drag::Scrub);
                    Some(st.viewport.time_at(x))
                }
                Hit::Empty => {
                    st.drag = Some(Drag::Scrub);
                    Some(st.viewport.time_at(x))
                }
            }
        };
        if let Some(t) = target {
            self.with_player(|p| p.seek_to(t, false));
        }
        self.publish();
    }

    fn pointer_moved(&self, x: f32, area: Area, pressed: bool, shift: bool) {
        let rate = self.frame_rate();
        let gap = hit::min_gap(rate);
        let (seek, layout_changed) = {
            let mut st = self.state.borrow_mut();
            st.hover = Some(x);
            let drag = st.drag.filter(|_| pressed);
            st.edge_hovered = match drag {
                Some(Drag::Edge { .. }) => true,
                Some(Drag::Scrub) => false,
                None => matches!(hit::hit(&st.viewport, &st.sections, area, x), Hit::Edge(_)),
            };
            match drag {
                Some(Drag::Scrub) => (Some(st.viewport.time_at(x)), false),
                Some(Drag::Edge { edge, offset }) => {
                    let raw = st.viewport.time_at(x - offset);
                    let playhead = Some(st.playhead);
                    let t = hit::snap(&st.viewport, &st.sections, edge, raw, playhead, rate, !shift);
                    st.sections.drag_edge(edge, t, gap);
                    (st.sections.edge_time(edge), true)
                }
                None => (None, false),
            }
        };
        if let Some(t) = seek {
            self.with_player(|p| p.seek_to(t, false));
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
            match st.drag.take() {
                Some(Drag::Scrub) => Some(st.viewport.time_at(x)),
                Some(Drag::Edge { edge, .. }) => st.sections.edge_time(edge),
                None => None,
            }
        };
        // The exact frame where the drag ended.
        if let Some(t) = target {
            self.with_player(|p| p.seek_to(t, true));
        }
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
}
