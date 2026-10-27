//! The callbacks of the Filters card and of the crop overlay, and the Ctrl key over the video.

use std::rc::{Rc, Weak};

use rustcut_core::edit::{DeshakeCrop, DeshakeInterpolation, DeshakeParams};
use rustcut_core::media::CropSearch;
use slint::ComponentHandle as _;

use super::geometry::Handle;
use super::state::{CropMode, CropSession, Edge, FiltersState};
use super::{FiltersBridge, QUICK_DELAY, kind_from_id, view};
use crate::util::amount::{limit, parse_hundredths};
use crate::{CropOverlay, Filters, MainWindow};

/// The interpolations in the order of the card's list.
const INTERPOLATIONS: [DeshakeInterpolation; 4] = [
    DeshakeInterpolation::Bicubic,
    DeshakeInterpolation::Bilinear,
    DeshakeInterpolation::Linear,
    DeshakeInterpolation::No,
];
/// Pixels per step of a resize field (a common multiple of the codecs' block sizes).
const RESIZE_STEP: i32 = 16;
/// One step of each deshake number (shakiness, accuracy, search step, smoothing, zoom, max shift, max angle, zoom
/// speed in hundredths), as in Settings > Filters.
const DESHAKE_STEPS: [i64; 8] = [1, 1, 1, 5, 1, 10, 1, 5];

pub(super) fn interpolation_index(i: DeshakeInterpolation) -> i32 {
    INTERPOLATIONS
        .iter()
        .position(|x| *x == i)
        .and_then(|p| i32::try_from(p).ok())
        .unwrap_or(0)
}

/// The overlay's number of a handle.
pub(super) const fn handle_index(handle: Handle) -> i32 {
    match handle {
        Handle::TopLeft => 0,
        Handle::Top => 1,
        Handle::TopRight => 2,
        Handle::Right => 3,
        Handle::BottomRight => 4,
        Handle::Bottom => 5,
        Handle::BottomLeft => 6,
        Handle::Left => 7,
        Handle::Inside => 8,
    }
}

/// A number of the auto-crop search that the Crop menu types and steps.
#[derive(Debug, Clone, Copy)]
enum Search {
    Similarity,
    Retries,
}

impl Search {
    const fn range(self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::Similarity => 1..=100,
            Self::Retries => 0..=CropSearch::MAX_RETRIES,
        }
    }

    const fn get(self, search: &CropSearch) -> u8 {
        match self {
            Self::Similarity => search.similarity,
            Self::Retries => search.retries,
        }
    }

    const fn set(self, search: &mut CropSearch, value: u8) {
        match self {
            Self::Similarity => search.similarity = value,
            Self::Retries => search.retries = value,
        }
    }
}

/// A typed whole number; empty = `empty`.
fn number(text: &str, empty: Option<i64>) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return empty;
    }
    text.parse().ok()
}

/// Runs `f` with the bridge while it exists.
fn with<A>(this: &Weak<FiltersBridge>, f: impl Fn(&FiltersBridge, A) + 'static) -> impl Fn(A) + 'static {
    let this = Weak::clone(this);
    move |a| {
        if let Some(bridge) = this.upgrade() {
            f(&bridge, a);
        }
    }
}

/// A state change from a callback, then the new state shown.
fn change<A>(this: &Weak<FiltersBridge>, f: impl Fn(&mut FiltersState, A) + 'static) -> impl Fn(A) + 'static {
    with(this, move |bridge, a| bridge.update(|s| f(s, a)))
}

pub(super) fn connect(bridge: &Rc<FiltersBridge>, window: &MainWindow) {
    let this = Rc::downgrade(bridge);
    let f = window.global::<Filters<'_>>();
    f.on_toggle(change(&this, |s, id: slint::SharedString| {
        if let Some(kind) = kind_from_id(&id) {
            s.toggle(kind);
        }
    }));
    let reset = with(&this, |b, ()| b.reset());
    f.on_reset(move || reset(()));
    f.on_set_preview(change(&this, |s, on| s.preview = on));
    f.on_set_keep(change(&this, |s, on| s.keep = on));
    f.on_set_deinterlacer(change(&this, |s, i: i32| {
        if let Some(d) = usize::try_from(i).ok().and_then(|i| view::DEINTERLACERS.get(i)) {
            s.set_deinterlacer(*d);
        }
    }));
    f.on_set_field_rate(change(&this, FiltersState::set_field_rate));
    f.on_set_field_order(change(&this, |s, i: i32| {
        if let Some(order) = usize::try_from(i).ok().and_then(|i| view::FIELD_ORDERS.get(i)) {
            s.set_field_order(*order);
        }
    }));
    f.on_set_only_interlaced(change(&this, FiltersState::set_only_interlaced));
    let pick = with(&this, |b, ()| b.start_pick());
    f.on_pick_border_color(move || pick(()));
    let clear = change(&this, |s, ()| s.set_search(|search| search.color = None));
    f.on_clear_border_color(move || clear(()));
    // The color tolerance (percent) and the retries: typed or stepped, inside their ranges.
    for (which, setter) in [(0, Search::Similarity), (1, Search::Retries)] {
        let typed = Weak::clone(&this);
        let edit = move |text: slint::SharedString| -> bool {
            let Some(bridge) = typed.upgrade() else { return false };
            let text = text.trim().trim_end_matches('%').trim_end();
            let Some(value) = text.parse::<u8>().ok().filter(|v| setter.range().contains(v)) else {
                return false;
            };
            bridge.update(|s| s.set_search(|search| setter.set(search, value)));
            true
        };
        let step = change(&this, move |s, steps: i32| {
            s.set_search(|search| {
                let value = i32::from(setter.get(search)).saturating_add(steps);
                let (lo, hi) = (*setter.range().start(), *setter.range().end());
                setter.set(
                    search,
                    u8::try_from(value.clamp(i32::from(lo), i32::from(hi))).unwrap_or(lo),
                );
            });
        });
        if which == 0 {
            f.on_edit_search_similarity(edit);
            f.on_step_search_similarity(step);
        } else {
            f.on_edit_search_retries(edit);
            f.on_step_search_retries(step);
        }
    }
    f.on_set_search_limit(change(&this, |s, v: i32| {
        s.set_search(|search| search.limit = u8::try_from(v.clamp(0, 255)).unwrap_or(search.limit));
    }));
    f.on_set_search_sides(change(&this, |s, i: i32| {
        if let Some(sides) = usize::try_from(i).ok().and_then(|i| view::SIDES.get(i)) {
            s.set_search(|search| search.sides = *sides);
        }
    }));
    let edit_samples = Weak::clone(&this);
    f.on_edit_search_samples(move |text| {
        let Some(bridge) = edit_samples.upgrade() else {
            return false;
        };
        let Some(value) = number(&text, None)
            .and_then(|v| u8::try_from(v).ok())
            .filter(|v| (1..=CropSearch::MAX_SAMPLES).contains(v))
        else {
            return false;
        };
        bridge.update(|s| s.set_search(|search| search.samples = value));
        true
    });
    f.on_step_search_samples(change(&this, |s, steps: i32| {
        s.set_search(|search| {
            let value = i32::from(search.samples).saturating_add(steps);
            search.samples = u8::try_from(value.clamp(1, i32::from(CropSearch::MAX_SAMPLES))).unwrap_or(search.samples);
        });
    }));
    f.on_set_search_sections_only(change(&this, |s, on: bool| {
        s.set_search(|search| search.sections_only = on);
    }));
    let edit_crop = Weak::clone(&this);
    f.on_edit_crop(move |edge, text| {
        let (Some(bridge), Some(edge)) = (edit_crop.upgrade(), Edge::from_index(edge)) else {
            return false;
        };
        let Some(value) = number(&text, Some(0)).and_then(|v| u32::try_from(v).ok()) else {
            return false;
        };
        bridge.update(|s| s.set_shown_edge(edge, value));
        true
    });
    let step_crop = change(&this, |s, (edge, steps): (i32, i32)| {
        if let Some(edge) = Edge::from_index(edge) {
            s.step_shown_edge(edge, steps);
        }
    });
    f.on_step_crop(move |edge, steps| step_crop((edge, steps)));
    let crop_edit = with(&this, |b, ()| b.toggle_crop_edit());
    f.on_crop_edit(move || crop_edit(()));
    let auto_crop = with(&this, |b, ()| b.auto_crop());
    f.on_auto_crop(move || auto_crop(()));
    f.on_rotate(change(&this, FiltersState::turn));
    f.on_mirror(change(&this, FiltersState::flip));
    let edit_resize = Weak::clone(&this);
    f.on_edit_resize(move |which, text| {
        let Some(bridge) = edit_resize.upgrade() else {
            return false;
        };
        let value = match number(&text, None) {
            None if text.trim().is_empty() => None,
            Some(v) => match u32::try_from(v) {
                Ok(v) => Some(v),
                Err(_) => return false,
            },
            None => return false,
        };
        bridge.update(|s| s.set_resize_side(u8::from(which != 0), value));
        true
    });
    let step_resize = change(&this, |s, (which, steps): (i32, i32)| {
        let (width, height, _) = s.resize();
        let input = s.resize_input();
        let current = if which == 0 {
            width.or(input.map(|i| i.width))
        } else {
            height.or(input.map(|i| i.height))
        };
        if let Some(current) = current {
            let value = current.saturating_add_signed(steps.saturating_mul(RESIZE_STEP));
            s.set_resize_side(u8::from(which != 0), Some(value));
        }
    });
    f.on_step_resize(move |which, steps| step_resize((which, steps)));
    f.on_set_algorithm(change(&this, |s, i: i32| {
        if let Some(a) = usize::try_from(i).ok().and_then(|i| view::ALGORITHMS.get(i)) {
            s.set_resize_algorithm(*a);
        }
    }));
    let edit_deshake = Weak::clone(&this);
    f.on_edit_deshake(move |param, text| {
        let Some(bridge) = edit_deshake.upgrade() else {
            return false;
        };
        let Some(params) = typed_deshake(bridge.state.borrow().deshake(), param, &text) else {
            return false;
        };
        bridge.update(|s| s.set_deshake(params));
        true
    });
    let step_deshake = change(&this, |s, (param, steps): (i32, i32)| {
        let current = deshake_value(s, param);
        let step = usize::try_from(param)
            .ok()
            .and_then(|i| DESHAKE_STEPS.get(i))
            .copied()
            .unwrap_or(1);
        set_deshake_value(s, param, current.saturating_add(i64::from(steps).saturating_mul(step)));
    });
    f.on_step_deshake(move |param, steps| step_deshake((param, steps)));
    let choice = change(&this, |s, (param, value): (i32, i32)| {
        let mut p = s.deshake();
        match param {
            0 => p.optzoom = u8::try_from(value).unwrap_or(0),
            1 => {
                p.crop = if value == 1 {
                    DeshakeCrop::Keep
                } else {
                    DeshakeCrop::Black
                };
            }
            _ => {
                if let Some(i) = usize::try_from(value).ok().and_then(|i| INTERPOLATIONS.get(i)) {
                    p.interpolation = *i;
                }
            }
        }
        s.set_deshake(p);
    });
    f.on_set_deshake_choice(move |param, value| choice((param, value)));
    f.on_set_tripod(change(&this, |s, on| {
        let mut p = s.deshake();
        p.tripod = on;
        s.set_deshake(p);
    }));
    f.on_set_sharpen(change(&this, |s, on| {
        let mut p = s.deshake();
        p.sharpen = on;
        s.set_deshake(p);
    }));

    let o = window.global::<CropOverlay<'_>>();
    let pointer = with(&this, |b, (kind, x, y, control, shift): (i32, f32, f32, bool, bool)| {
        b.pointer(kind, x, y, control, shift);
    });
    o.on_pointer(move |kind, x, y, control, shift| pointer((kind, x, y, control, shift)));
    let cycle = with(&this, |b, landscape: bool| {
        if b.state.borrow_mut().cycle_ratio(landscape) {
            b.publish();
        }
    });
    o.on_cycle_ratio(cycle);
    let double = with(&this, |b, (x, y): (f32, f32)| {
        if b.state.borrow().inside_crop(x, y) {
            b.end_crop(true);
        }
    });
    o.on_double_clicked(move |x, y| double((x, y)));
    let left = with(&this, |b, ()| b.pointer_left());
    o.on_left(move || left(()));
    let resized = with(&this, |b, (w, h): (f32, f32)| {
        b.state.borrow_mut().set_view(w, h);
        b.publish_overlay();
    });
    o.on_resized(move |w, h| resized((w, h)));
}

/// One deshake number (a step starts a limit that is off at 0).
fn deshake_value(s: &FiltersState, param: i32) -> i64 {
    let p = s.deshake();
    match param {
        0 => i64::from(p.shakiness),
        1 => i64::from(p.accuracy),
        2 => i64::from(p.stepsize),
        3 => i64::from(p.smoothing),
        5 => i64::from(p.max_shift.unwrap_or(0)),
        6 => i64::from(p.max_angle.unwrap_or(0)),
        7 => i64::from(p.zoom_speed),
        _ => i64::from(p.zoom),
    }
}

/// Sets one deshake number, limited to its range (the parameters are sanitized as a whole afterwards).
fn set_deshake_value(s: &mut FiltersState, param: i32, value: i64) {
    let mut p = s.deshake();
    let small = |v: i64| u8::try_from(v.clamp(0, 255)).unwrap_or(0);
    let wide = |v: i64, max: u16| u16::try_from(v.clamp(0, i64::from(max))).unwrap_or(0);
    match param {
        0 => p.shakiness = small(value),
        1 => p.accuracy = small(value),
        2 => p.stepsize = small(value),
        3 => p.smoothing = wide(value, 1000),
        5 => p.max_shift = Some(wide(value, DeshakeParams::MAX_SHIFT)),
        6 => p.max_angle = Some(small(value.min(i64::from(DeshakeParams::MAX_ANGLE)))),
        7 => p.zoom_speed = wide(value, DeshakeParams::ZOOM_SPEED_MAX),
        _ => p.zoom = i8::try_from(value.clamp(-100, 100)).unwrap_or(0),
    }
    s.set_deshake(p);
}

/// The deshake parameters with one typed value (`None` = refused: not a number, or out of range for the limits and
/// the zoom speed). Max shift and max angle take an empty text as "no limit"; the zoom speed is a decimal percent.
pub(super) fn typed_deshake(current: DeshakeParams, param: i32, text: &str) -> Option<DeshakeParams> {
    let mut p = current;
    match param {
        5 => p.max_shift = limit(text, DeshakeParams::MAX_SHIFT, "px").ok()?,
        6 => {
            p.max_angle = match limit(text, u16::from(DeshakeParams::MAX_ANGLE), "°").ok()? {
                Some(v) => Some(u8::try_from(v).ok()?),
                None => None,
            };
        }
        7 => p.zoom_speed = parse_hundredths(text, DeshakeParams::ZOOM_SPEED_MAX)?,
        _ => {
            let value = number(text, None)?;
            let small = |v: i64| u8::try_from(v.clamp(0, 255)).ok();
            match param {
                0 => p.shakiness = small(value)?,
                1 => p.accuracy = small(value)?,
                2 => p.stepsize = small(value)?,
                3 => p.smoothing = u16::try_from(value.clamp(0, 1000)).ok()?,
                _ => p.zoom = i8::try_from(value.clamp(-100, 100)).ok()?,
            }
        }
    }
    Some(p)
}

impl FiltersBridge {
    /// The pointer over the video (`kind`: 0 down, 1 up, 2 move), with Ctrl's and Shift's state.
    fn pointer(&self, kind: i32, x: f32, y: f32, control: bool, shift: bool) {
        self.hover.set(Some((x, y)));
        if self
            .state
            .borrow()
            .crop_session()
            .is_some_and(|s| s.mode == CropMode::Pick)
        {
            if kind == 0 {
                self.pick_at(x, y);
            }
            return;
        }
        match kind {
            0 => {
                if control && self.state.borrow().crop_session().is_none() {
                    self.begin_quick();
                }
                if self.state.borrow_mut().crop_pointer_down(x, y) {
                    self.publish_overlay();
                }
            }
            1 => {
                if self.state.borrow_mut().crop_pointer_up() {
                    if self.quick() && !control {
                        self.end_crop(true);
                    } else {
                        self.publish();
                    }
                }
            }
            _ => {
                let (session, dragging) = {
                    let st = self.state.borrow();
                    (
                        st.crop_session().map(|s| s.mode),
                        st.crop_session().is_some_and(CropSession::dragging),
                    )
                };
                match session {
                    None if control => self.begin_quick(),
                    Some(CropMode::Quick) if !control && !dragging => {
                        self.end_crop(true);
                        return;
                    }
                    _ => {}
                }
                if self.state.borrow_mut().crop_pointer_moved(x, y, shift) {
                    if dragging {
                        self.publish_card();
                    }
                    self.publish_overlay();
                }
            }
        }
    }

    fn pointer_left(&self) {
        self.hover.set(None);
        self.quick_timer.stop();
        let dragging = self.state.borrow().crop_session().is_some_and(CropSession::dragging);
        if self.quick() && !dragging {
            self.end_crop(true);
        } else if self.state.borrow_mut().crop_pointer_left() {
            self.publish_overlay();
        }
    }

    fn quick(&self) -> bool {
        self.state
            .borrow()
            .crop_session()
            .is_some_and(|s| s.mode == CropMode::Quick)
    }

    /// Starts the quick crop (Ctrl held over the video) unless a session runs or the picture is not known.
    fn begin_quick(&self) {
        if self.suspended.get() {
            return;
        }
        let started = {
            let mut st = self.state.borrow_mut();
            st.crop_session().is_none() && st.begin_crop(CropMode::Quick)
        };
        if started {
            if let Some((x, y)) = self.hover.get() {
                self.state.borrow_mut().crop_pointer_moved(x, y, false);
            }
            self.publish();
        }
    }

    /// Ctrl went down or up (from the key scope). Held over the video for a moment it starts the quick crop; released,
    /// it ends the quick crop (after a drag in progress).
    pub fn control_changed(&self, down: bool) {
        self.ctrl.set(down);
        if down {
            if self.hover.get().is_some() && self.state.borrow().crop_session().is_none() {
                let this = Weak::clone(&self.me);
                self.quick_timer
                    .start(slint::TimerMode::SingleShot, QUICK_DELAY, move || {
                        if let Some(bridge) = this.upgrade()
                            && bridge.ctrl.get()
                            && bridge.hover.get().is_some()
                        {
                            bridge.begin_quick();
                        }
                    });
            }
            return;
        }
        self.quick_timer.stop();
        let dragging = self.state.borrow().crop_session().is_some_and(CropSession::dragging);
        if self.quick() && !dragging {
            self.end_crop(true);
        }
    }

    /// Another key was pressed: a Ctrl held for a shortcut (Ctrl+O) does not start the quick crop.
    pub fn other_key(&self) {
        self.quick_timer.stop();
    }
}
