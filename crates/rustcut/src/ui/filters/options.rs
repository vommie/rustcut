//! The options of each filter's menu (P9.13 step 3, Q-P9.13), pure and tested: what a menu shows of its filter set back to
//! the defaults of Settings > Filters (the menu's held reset), compared with them (reset and save have nothing to do
//! while they match) and written into them (save as default); the auto-crop search of the Crop menu; a drop of drag &
//! drop. A menu's options are the values its card row does not show: the crop's edges and the resize's size stay.

use rustcut_core::edit::{Crop, Filter, FilterKind};
use rustcut_core::media::CropSearch;
use rustcut_core::settings::FilterDefaults;

use super::state::FiltersState;

impl FiltersState {
    /// Changes how auto-crop searches (the Crop menu), limited to what auto-crop accepts.
    pub fn set_search(&mut self, change: impl FnOnce(&mut CropSearch)) {
        let mut search = self.search;
        change(&mut search);
        self.search = search.sanitized();
    }

    /// `crop` (in the crop filter's orientation) with an even kept size (an auto-crop result; it only shrinks, so a
    /// found border never shows again). The final size rule acts after every filter (Q-P9s), not here.
    pub fn fitted_crop(&self, crop: Crop) -> Crop {
        match self.crop_frame() {
            Some(frame) => crop.fitted_to_multiple(frame, 2),
            None => crop,
        }
    }

    /// A drop of drag & drop: `kind` before the filter at `before` (an index of the order before the move); `false`
    /// when the order stays.
    pub fn drop_filter(&mut self, kind: FilterKind, before: usize) -> bool {
        self.chain.move_to(kind, before)
    }

    /// Whether the options `kind`'s menu shows equal the defaults of Settings > Filters (Orientation has no menu).
    pub fn options_are_default(&self, kind: FilterKind) -> bool {
        let defaults = self.defaults().sanitized();
        match kind {
            FilterKind::Deinterlace => self.deinterlace() == defaults.deinterlace(),
            // The picked color belongs to the video, never to the defaults.
            FilterKind::Crop => {
                CropSearch {
                    color: None,
                    ..self.search
                } == defaults.crop_search
            }
            FilterKind::Deshake => self.deshake() == defaults.deshake,
            FilterKind::Resize => self.resize().2 == defaults.resize_algorithm,
            FilterKind::Orientation => true,
        }
    }

    /// The options of `kind`'s menu back to the defaults; the filter stays on or off and keeps the values of its row
    /// (the crop's edges, the resize's size).
    pub fn reset_options(&mut self, kind: FilterKind) {
        let defaults = self.defaults().sanitized();
        match kind {
            FilterKind::Deinterlace => self.chain.set(defaults.deinterlace()),
            FilterKind::Crop => self.search = defaults.crop_search,
            FilterKind::Deshake => self.chain.set(Filter::Deshake(defaults.deshake)),
            FilterKind::Resize => self.set_resize_algorithm(defaults.resize_algorithm),
            FilterKind::Orientation => {}
        }
    }

    /// The defaults with the options of `kind`'s menu put in (save as default: Settings > Filters takes them).
    pub fn options_as_defaults(&self, kind: FilterKind) -> FilterDefaults {
        let mut defaults = self.defaults().clone();
        match kind {
            FilterKind::Deinterlace => {
                if let Filter::Deinterlace {
                    deinterlacer,
                    rate,
                    field_order,
                    only_interlaced,
                } = self.deinterlace()
                {
                    defaults.deinterlacer = deinterlacer;
                    defaults.deinterlace_rate = rate;
                    defaults.field_order = field_order;
                    defaults.only_interlaced = only_interlaced;
                }
            }
            FilterKind::Crop => {
                defaults.crop_search = CropSearch {
                    color: None,
                    ..self.search
                };
            }
            FilterKind::Deshake => defaults.deshake = self.deshake(),
            FilterKind::Resize => defaults.resize_algorithm = self.resize().2,
            FilterKind::Orientation => {}
        }
        defaults.sanitized()
    }
}
