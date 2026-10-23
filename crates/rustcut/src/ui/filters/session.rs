//! The crop being edited on the video (pure, tested with the state): the edit mode with handles, or the quick mode while
//! Ctrl is held; hovering, grabbing and dragging go through the [`CropView`](super::geometry::CropView) geometry.

use rustcut_core::edit::{Crop, Filter, FilterKind};

use super::geometry::Handle;
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
}

impl CropSession {
    /// The handle under the pointer or in the hand (for the pointer shape and the highlight).
    pub fn active_handle(&self) -> Option<Handle> {
        self.drag.map(|d| d.handle).or(self.hover)
    }

    pub const fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

impl FiltersState {
    /// Starts editing the crop on the video (switching the crop on). `false` while the picture is not known.
    pub fn begin_crop(&mut self, mode: CropMode) -> bool {
        if self.crop_view().is_none() {
            return false;
        }
        if let Some(session) = &mut self.crop {
            session.mode = mode;
            return true;
        }
        let original = (self.crop(), self.chain.is_enabled(FilterKind::Crop));
        self.chain.set_enabled(FilterKind::Crop, true);
        self.crop = Some(CropSession {
            mode,
            original,
            hover: None,
            drag: None,
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

    fn handle_at(&self, session: &CropSession, x: f32, y: f32) -> Option<Handle> {
        let view = self.crop_view()?;
        match session.mode {
            CropMode::Edit => view.hit(self.crop(), x, y),
            CropMode::Quick => view.nearest(self.crop(), x, y),
        }
    }

    /// The pointer moved over the video: the hovered handle, or the drag. `true` when the crop or the highlight changed.
    pub fn crop_pointer_moved(&mut self, x: f32, y: f32) -> bool {
        let Some(session) = self.crop else { return false };
        if let Some(drag) = session.drag {
            let Some(view) = self.crop_view() else { return false };
            let ratio = self.aspect.ratio(view.shown_aspect());
            let crop = view.drag(drag.start, drag.handle, x - drag.from.0, y - drag.from.1, ratio);
            let changed = crop != self.crop();
            self.chain.set(Filter::Crop(crop));
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
