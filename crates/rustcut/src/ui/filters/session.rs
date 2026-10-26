//! The crop being edited on the video (pure, tested with the state): the edit mode with handles, or the quick mode while
//! Ctrl is held; hovering, grabbing and dragging go through the [`CropView`](super::geometry::CropView) geometry. The
//! ratio button on the crop (P9.14, RustView's crop tool) goes through the presets of Settings > Filters and places
//! each like the crop the user made (`placement.rs`); a drag is free, with Shift it keeps the proportions.

use rustcut_core::edit::{Crop, FREE_LABEL, Filter, FilterKind, RatioChoice};

use super::geometry::{CropView, Handle};
use super::placement::{self, Placement};
use super::state::FiltersState;

/// How the crop is being edited on the video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropMode {
    /// Started with the button or the key: precise handles, the kept area moves; Enter keeps, Escape restores.
    Edit,
    /// While Ctrl is held over the video: the nearest edge or corner follows the pointer, no aiming needed.
    Quick,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CropDrag {
    handle: Handle,
    start: Crop,
    from: (f32, f32),
}

/// A crop being edited on the video.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropSession {
    pub mode: CropMode,
    /// The crop and its on/off state when the session began (Escape restores them).
    original: (Crop, bool),
    hover: Option<Handle>,
    drag: Option<CropDrag>,
    /// What the ratio button keeps: the area and the anchors of the crop the user made.
    placement: Placement,
    /// The crop still has the shape of the chosen ratio (else the button reads "Free").
    follows: bool,
}

impl CropSession {
    /// The handle under the pointer or in the hand (for the pointer shape and the highlight).
    pub fn active_handle(&self) -> Option<Handle> {
        self.drag.map(|d| d.handle).or(self.hover)
    }

    pub const fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub const fn follows(&self) -> bool {
        self.follows
    }
}

/// How far a crop may be from a ratio's shape and still have it: the multiple its size keeps (at least two pixels: the
/// crop is on even pixels).
fn slack(view: &CropView) -> f64 {
    f64::from(view.multiple().max(2)) * view.pixel_width().max(1.0)
}

impl FiltersState {
    /// The display ratio of the chosen ratio (width / height); `None` before the picture is known.
    pub fn chosen_ratio(&self) -> Option<f64> {
        let view = self.crop_view()?;
        self.ratios.value(self.ratio, view.shown_aspect())
    }

    /// The ratio button's text: the chosen ratio while the crop has its shape, else "Free".
    pub fn ratio_label(&self) -> &str {
        if self.crop.is_some_and(|s| s.follows) {
            self.ratios.label(self.ratio)
        } else {
            FREE_LABEL
        }
    }

    /// Whether the crop has the shape of the chosen ratio.
    fn crop_follows(&self, view: &CropView, crop: Crop) -> bool {
        self.chosen_ratio()
            .is_some_and(|r| placement::follows(view.display_rect(crop), r, slack(view)))
    }

    /// Starts editing the crop on the video (switching the crop on). `false` while the picture is not known.
    pub fn begin_crop(&mut self, mode: CropMode) -> bool {
        let Some(view) = self.crop_view() else {
            return false;
        };
        if let Some(session) = &mut self.crop {
            session.mode = mode;
            return true;
        }
        let crop = self.crop();
        let original = (crop, self.chain.is_enabled(FilterKind::Crop));
        self.chain.set_enabled(FilterKind::Crop, true);
        self.crop = Some(CropSession {
            mode,
            original,
            hover: None,
            drag: None,
            placement: Placement::of(view.display_rect(crop), view.display_size(), view.pixel_width()),
            follows: self.crop_follows(&view, crop),
        });
        true
    }

    /// Ends the session: `keep` = the edited crop stays, else the crop from the start comes back.
    /// A session that changed nothing leaves the filter as it was (holding Ctrl alone does not switch the crop on).
    pub fn end_crop(&mut self, keep: bool) {
        if let Some(session) = self.crop.take() {
            let (crop, enabled) = session.original;
            if !keep || self.crop() == crop {
                self.chain.set(Filter::Crop(crop));
                self.chain.set_enabled(FilterKind::Crop, enabled);
            }
        }
    }

    /// The ratio button: the next landscape (left click) or portrait (right click) ratio, placed like the crop the user
    /// made. `false` without a session.
    pub fn cycle_ratio(&mut self, landscape: bool) -> bool {
        let (Some(session), Some(view)) = (self.crop, self.crop_view()) else {
            return false;
        };
        self.ratio = self.ratios.next(self.ratio, landscape);
        let Some(ratio) = self.chosen_ratio() else {
            self.ratio = RatioChoice::Original;
            return false;
        };
        let crop = view.crop_of_display(session.placement.place(ratio, view.display_size()));
        self.chain.set(Filter::Crop(crop));
        if let Some(s) = &mut self.crop {
            s.follows = true;
        }
        true
    }

    fn handle_at(&self, session: &CropSession, x: f32, y: f32) -> Option<Handle> {
        let view = self.crop_view()?;
        match session.mode {
            CropMode::Edit => view.hit(self.crop(), x, y),
            CropMode::Quick => view.nearest(self.crop(), x, y),
        }
    }

    /// Whether (`x`, `y`) lies inside the kept area while the crop is edited (a double click there keeps it).
    pub fn inside_crop(&self, x: f32, y: f32) -> bool {
        self.crop.is_some()
            && self
                .crop_view()
                .is_some_and(|view| view.hit(self.crop(), x, y) == Some(Handle::Inside))
    }

    /// The pointer moved over the video: the hovered handle, or the drag (`shift` keeps the proportions: the chosen
    /// ratio while the crop has it, else the crop's own). `true` when the crop or the highlight changed.
    pub fn crop_pointer_moved(&mut self, x: f32, y: f32, shift: bool) -> bool {
        let Some(session) = self.crop else { return false };
        if let Some(drag) = session.drag {
            let Some(view) = self.crop_view() else { return false };
            let ratio = if !shift {
                None
            } else if session.follows {
                self.chosen_ratio()
            } else {
                Some(view.display_rect(drag.start).ratio()).filter(|r| *r > 0.0)
            };
            let crop = view.drag(drag.start, drag.handle, x - drag.from.0, y - drag.from.1, ratio);
            let changed = crop != self.crop();
            self.chain.set(Filter::Crop(crop));
            let rect = view.display_rect(crop);
            let (size, tolerance) = (view.display_size(), view.pixel_width());
            let follows = self.crop_follows(&view, crop);
            if let Some(s) = &mut self.crop {
                if drag.handle == Handle::Inside {
                    s.placement = s.placement.moved(rect, size, tolerance);
                } else {
                    s.placement = Placement::of(rect, size, tolerance);
                    s.follows = follows;
                }
            }
            return changed;
        }
        let hover = self.handle_at(&session, x, y);
        let changed = hover != session.hover;
        if let Some(s) = &mut self.crop {
            s.hover = hover;
        }
        changed
    }

    /// A press on the video: grabs the handle there. `false` when the press is not for the crop.
    pub fn crop_pointer_down(&mut self, x: f32, y: f32) -> bool {
        let Some(session) = self.crop else { return false };
        let Some(handle) = self.handle_at(&session, x, y) else {
            return false;
        };
        let start = self.crop();
        if let Some(s) = &mut self.crop {
            s.drag = Some(CropDrag {
                handle,
                start,
                from: (x, y),
            });
            s.hover = Some(handle);
        }
        true
    }

    /// The press ended. `true` when a drag ended.
    pub fn crop_pointer_up(&mut self) -> bool {
        match &mut self.crop {
            Some(s) if s.drag.is_some() => {
                s.drag = None;
                true
            }
            _ => false,
        }
    }

    /// The pointer left the video: no highlight unless a drag goes on.
    pub fn crop_pointer_left(&mut self) -> bool {
        match &mut self.crop {
            Some(s) if s.drag.is_none() && s.hover.is_some() => {
                s.hover = None;
                true
            }
            _ => false,
        }
    }
}
