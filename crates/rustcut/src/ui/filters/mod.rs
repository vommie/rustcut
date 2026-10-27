//! The Filters card, its options menus and cropping on the video. The pure parts are `state` (the chain and every card
//! operation, the preview text, the crop session), `options` (what each menu shows, reset, compared with and saved as
//! the defaults), `geometry` (frame pixels <-> video area through rotation, mirrors, pixel aspect and letterboxing) and
//! `view` (what the card and the menus show). This bridge keeps the state on the UI thread, handles the callbacks
//! (`input.rs`, `menu.rs`, `drag.rs`), shows the state (`publish.rs`), sends the preview's video filters to the
//! player's command thread, and runs the picture checks (auto-crop, interlace) on the tokio runtime; their results come
//! back through `slint::spawn_local`, checked against the file they were made for.

mod drag;
mod input;
mod menu;
mod options;
mod placement;
mod publish;
mod tasks;

pub mod geometry;
pub mod session;
pub mod state;
pub mod view;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_menu;
#[cfg(test)]
mod tests_ratio;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use rustcut_core::edit::{CropAspect, FilterKind};
use rustcut_core::media::{DetectOptions, MediaInfo};
use rustcut_core::settings::{ExportPanelState, Settings};
use rustcut_core::time::Timestamp;
use rustcut_player::VideoFilter;
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use self::state::{CropMode, FiltersState, InterlaceHint, Kept, Source};
use crate::app::SettingsWriter;
use crate::ui::menu::ContextMenuBridge;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::player::PlayerBridge;
use crate::{FilterRowView, Filters, MainWindow};

/// How long Ctrl must be held over the video before the quick crop shows (a shortcut like Ctrl+O is faster than this).
const QUICK_DELAY: Duration = Duration::from_millis(220);

/// The keys of the filter actions, for the card's tooltips.
#[derive(Debug, Clone, Default)]
pub struct FilterKeys {
    pub preview: String,
    pub keep: String,
    pub reset: String,
    pub crop_edit: String,
    pub auto_crop: String,
    pub rotate_left: String,
    pub rotate_right: String,
    pub mirror_horizontal: String,
    pub mirror_vertical: String,
}

pub struct FiltersBridge {
    /// The bridge itself, for results that come back later.
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    player: Weak<PlayerBridge>,
    runtime: tokio::runtime::Handle,
    detect: RefCell<DetectOptions>,
    state: RefCell<FiltersState>,
    /// The open file and a number that changes with every open (late results of an older file are dropped).
    file: RefCell<Option<PathBuf>>,
    generation: Cell<u64>,
    idet_task: RefCell<Option<tokio::task::AbortHandle>>,
    crop_task: RefCell<Option<tokio::task::AbortHandle>>,
    /// The video filters last sent to the player.
    sent: RefCell<VideoFilter>,
    rows: Rc<VecModel<FilterRowView>>,
    /// The player shows a probe render (the filters are in its pixels already): no preview filters, no crop on it.
    suspended: Cell<bool>,
    /// Ctrl is held, and where the pointer is over the video.
    ctrl: Cell<bool>,
    hover: Cell<Option<(f32, f32)>>,
    quick_timer: slint::Timer,
    /// The sections' time ranges (from the editor), for auto-crop.
    sections: RefCell<Option<SectionsSource>>,
    /// "Save as default" of the menus writes `[filters]` (Settings > Filters).
    writer: SettingsWriter,
    /// The rows' context menu and the resize ratio menu.
    menu: Weak<ContextMenuBridge>,
    /// A row being dragged (P9.4), and the rows' shifts meanwhile.
    drag: RefCell<Option<drag::FiltersDrag>>,
    drag_shifts: Rc<VecModel<f32>>,
    /// The filter whose options menu is open, and whether menus stay open while the pointer works elsewhere (kept).
    menu_kind: Cell<Option<FilterKind>>,
    menu_pinned: Cell<bool>,
}

/// What the bridge needs besides the window (the app owns them).
pub struct Parts<'a> {
    pub player: &'a Rc<PlayerBridge>,
    pub runtime: tokio::runtime::Handle,
    pub settings: &'a Settings,
    pub export: &'a ExportPanelState,
    pub writer: SettingsWriter,
    pub menu: &'a Rc<ContextMenuBridge>,
}

/// Where the bridge reads the sections' time ranges.
type SectionsSource = Box<dyn Fn() -> Vec<(Timestamp, Timestamp)>>;

impl FiltersBridge {
    /// Takes over the Filters card and the crop overlay. The app owns the only strong reference.
    pub fn install(window: &MainWindow, parts: Parts<'_>) -> Rc<Self> {
        let Parts {
            player,
            runtime,
            settings,
            export,
            writer,
            menu,
        } = parts;
        let state = FiltersState::new(
            settings.filters.clone(),
            export.preview_filters,
            export.keep_filters,
            Kept {
                chain: export.filters.clone(),
                search: export.crop_search,
            },
            export.crop_ratio.as_deref().or(export.crop_aspect.ratio_label()),
        );
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            player: Rc::downgrade(player),
            runtime,
            detect: RefCell::new(DetectOptions {
                program: settings.advanced.ffmpeg.clone(),
                ..DetectOptions::default()
            }),
            state: RefCell::new(state),
            file: RefCell::new(None),
            generation: Cell::new(0),
            idet_task: RefCell::new(None),
            crop_task: RefCell::new(None),
            sent: RefCell::new(VideoFilter::default()),
            rows: Rc::new(VecModel::default()),
            suspended: Cell::new(false),
            ctrl: Cell::new(false),
            hover: Cell::new(None),
            quick_timer: slint::Timer::default(),
            sections: RefCell::new(None),
            writer,
            menu: Rc::downgrade(menu),
            drag: RefCell::new(None),
            drag_shifts: Rc::new(VecModel::default()),
            menu_kind: Cell::new(None),
            menu_pinned: Cell::new(export.filter_menu_pinned),
        });
        let filters = window.global::<Filters<'_>>();
        filters.set_rows(ModelRc::from(Rc::clone(&bridge.rows)));
        filters.set_tips(publish::tips());
        filters.set_deinterlacers(strings(view::DEINTERLACERS.iter().map(|d| d.ffmpeg_name())));
        filters.set_algorithms(strings(view::ALGORITHMS.iter().map(|a| a.ffmpeg_flag())));
        filters.set_interpolations(strings(["Bicubic", "Bilinear", "Linear", "None"].into_iter()));
        input::connect(&bridge, window);
        menu::connect(&bridge, window);
        drag::connect(&bridge, window);
        let this = Rc::downgrade(&bridge);
        crate::ui::media_info::add_probe_hook(move |info| {
            if let Some(bridge) = this.upgrade() {
                bridge.probed(info);
            }
        });
        bridge.publish();
        bridge
    }

    /// A video is about to open (before the player loads it): the crop session ends, the checks of the previous file
    /// stop, and the filters start over unless "keep" is on. The preview waits for the probe (it needs the rotation).
    pub fn file_opening(&self, path: &Path) {
        self.generation.set(self.generation.get().wrapping_add(1));
        *self.file.borrow_mut() = Some(path.to_path_buf());
        for task in [&self.idet_task, &self.crop_task] {
            if let Some(task) = task.borrow_mut().take() {
                task.abort();
            }
        }
        self.quick_timer.stop();
        self.state.borrow_mut().file_opened();
        self.publish();
    }

    /// The Media Info probe has a result for the open file (UI thread; called again with the frame timing).
    fn probed(&self, info: Option<&Arc<MediaInfo>>) {
        if self.state.borrow().source().is_some() {
            return;
        }
        let Some(info) = info else { return };
        let Some((_, video)) = info.primary_video() else {
            self.state.borrow_mut().interlace = InterlaceHint::None;
            self.publish();
            return;
        };
        let Some((width, height)) = video.oriented_size() else {
            return;
        };
        let turned = (video.rotation / 90) % 2 == 1;
        let sar = video.sample_aspect.map_or(1.0, rustcut_core::media::Ratio::as_f64);
        let source = Source {
            size: rustcut_core::edit::Size::new(width, height),
            sar: if turned && sar > 0.0 { 1.0 / sar } else { sar },
            rotation: video.rotation,
            duration: info.duration(),
        };
        tracing::info!(size = ?source.size, sar = source.sar, rotation = source.rotation, "[Filters] Source known");
        self.state.borrow_mut().source_known(source);
        self.check_interlace(source);
        self.publish();
    }

    /// Where auto-crop reads the sections (the editor, connected after both exist).
    pub fn set_sections_source(&self, source: impl Fn() -> Vec<(Timestamp, Timestamp)> + 'static) {
        *self.sections.borrow_mut() = Some(Box::new(source));
    }

    /// The ffmpeg that auto-crop and the interlace check run (changed in the Settings window).
    pub fn set_ffmpeg(&self, program: Option<PathBuf>) {
        self.detect.borrow_mut().program = program;
    }

    /// The deinterlacer and deshake defaults changed in the Settings window: fresh chains take them.
    pub fn set_defaults(&self, defaults: rustcut_core::settings::FilterDefaults) {
        self.state.borrow_mut().set_defaults(defaults);
        // The header's final size follows the final size rule (Q-P9s).
        self.publish_card();
    }

    /// What `ui-state.toml` keeps of the filters.
    pub fn save_into(&self, export: &mut ExportPanelState) {
        let st = self.state.borrow();
        export.preview_filters = st.preview;
        export.keep_filters = st.keep;
        let kept = st.kept();
        export.filters = kept.chain;
        export.crop_search = kept.search;
        let label = st.ratio_choice_label();
        export.crop_aspect = CropAspect::from_ratio_label(label);
        export.crop_ratio = Some(label.to_owned());
        export.filter_menu_pinned = self.menu_pinned.get();
    }

    /// Shows the keys of the filter actions in the card's tooltips.
    pub fn show_keys(&self, keys: &FilterKeys) {
        let Some(window) = self.window.upgrade() else { return };
        let f = window.global::<Filters<'_>>();
        f.set_key_preview(keys.preview.as_str().into());
        f.set_key_keep(keys.keep.as_str().into());
        f.set_key_reset(keys.reset.as_str().into());
        f.set_key_crop_edit(keys.crop_edit.as_str().into());
        f.set_key_auto_crop(keys.auto_crop.as_str().into());
        f.set_key_rotate_left(keys.rotate_left.as_str().into());
        f.set_key_rotate_right(keys.rotate_right.as_str().into());
        f.set_key_mirror_horizontal(keys.mirror_horizontal.as_str().into());
        f.set_key_mirror_vertical(keys.mirror_vertical.as_str().into());
    }

    // --- Actions (card buttons, keys, menu) ---------------------------------------------------------------------------

    /// Changes the state and shows the result.
    fn update(&self, change: impl FnOnce(&mut FiltersState)) {
        change(&mut self.state.borrow_mut());
        self.publish();
    }

    pub fn toggle_preview(&self) {
        self.update(|s| s.preview = !s.preview);
    }

    pub fn toggle_keep(&self) {
        self.update(|s| s.keep = !s.keep);
    }

    pub fn reset(&self) {
        self.update(FiltersState::reset);
        Self::tell(Notice::new(Kind::FiltersReset, "Filters reset"));
    }

    /// The rotate keys: turn the picture a quarter (Orientation switches on).
    pub fn turn(&self, clockwise: bool) {
        self.update(|s| s.turn(clockwise));
    }

    /// The mirror keys: mirror the picture (Orientation switches on).
    pub fn flip(&self, horizontal: bool) {
        self.update(|s| s.flip(horizontal));
    }

    /// Starts or finishes editing the crop on the video (the button, the key).
    pub fn toggle_crop_edit(&self) {
        if self.suspended.get() {
            Self::tell(
                Notice::new(Kind::ProbeLocked, "The probe render is playing").detail("Esc returns to the source."),
            );
            return;
        }
        let session = self.state.borrow().crop_session().map(|s| s.mode);
        match session {
            Some(CropMode::Edit) => self.end_crop(true),
            Some(CropMode::Quick | CropMode::Pick) | None => {
                // Without a video the card is closed (the state manager, M6.5e) and its key does nothing.
                if !self.state.borrow_mut().begin_crop(CropMode::Edit) {
                    tracing::debug!("[Filters] No video: the crop is not edited");
                }
                self.publish();
            }
        }
    }

    fn end_crop(&self, keep: bool) {
        self.update(|s| s.end_crop(keep));
    }

    /// A key while the crop is edited: Enter keeps it, Escape restores the crop from before. `true` = taken.
    pub fn crop_key(&self, text: &str) -> bool {
        if self.state.borrow().crop_session().is_none() {
            return false;
        }
        let key = |k: slint::platform::Key| text == SharedString::from(k).as_str();
        if key(slint::platform::Key::Return) {
            self.end_crop(true);
            true
        } else if key(slint::platform::Key::Escape) {
            self.end_crop(false);
            true
        } else {
            false
        }
    }

    fn tell(notice: Notice) {
        if notice.level >= Level::Warning {
            tracing::warn!("[Filters] {}", notice.text());
        } else {
            tracing::info!("[Filters] {}", notice.text());
        }
        notices::post(notice);
    }

    // --- Showing the state ------------------------------------------------------------------------------------------

    /// The card, the overlay and the player's video filters.
    fn publish(&self) {
        self.publish_card();
        self.publish_overlay();
        self.send_preview();
    }

    /// The chain a job (and the probe render) renders.
    pub fn chain(&self) -> rustcut_core::edit::FilterChain {
        self.state.borrow().chain().clone()
    }

    /// The final size rule of Settings > Filters (Q-P9s): a saved job and a probe take it.
    pub fn final_multiple(&self) -> u8 {
        self.state.borrow().defaults().final_multiple
    }

    /// The chain of a job loaded into the editor, or of a draft put back (after its video opened).
    pub fn load_chain(&self, chain: rustcut_core::edit::FilterChain) {
        if self.state.borrow().crop_session().is_some() {
            self.end_crop(true);
        }
        self.state.borrow_mut().load_chain(chain);
        self.publish();
    }

    /// The player shows a probe render (`true`) or the source again: the preview filters are taken off meanwhile and a
    /// crop being edited is kept as it is.
    pub fn suspend_preview(&self, suspended: bool) {
        self.suspended.set(suspended);
        if suspended && self.state.borrow().crop_session().is_some() {
            self.end_crop(true);
            return;
        }
        self.publish();
    }

    fn send_preview(&self) {
        let filter = if self.suspended.get() {
            VideoFilter::default()
        } else {
            self.state.borrow().preview_filter()
        };
        if *self.sent.borrow() == filter {
            return;
        }
        if let Some(player) = self.player.upgrade() {
            player.set_video_filter(filter.clone());
        }
        *self.sent.borrow_mut() = filter;
    }
}

impl Drop for FiltersBridge {
    fn drop(&mut self) {
        for task in [self.idet_task.get_mut(), self.crop_task.get_mut()] {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }
}

/// The card's id of a filter kind (its `config` name).
pub fn kind_id(kind: FilterKind) -> &'static str {
    match kind {
        FilterKind::Deinterlace => "deinterlace",
        FilterKind::Crop => "crop",
        FilterKind::Deshake => "deshake",
        FilterKind::Orientation => "orientation",
        FilterKind::Resize => "resize",
    }
}

pub fn kind_from_id(id: &str) -> Option<FilterKind> {
    FilterKind::DEFAULT_ORDER.into_iter().find(|k| kind_id(*k) == id)
}

/// What a filter does (the tooltip of its title and switch).
const fn kind_tooltip(kind: FilterKind) -> &'static str {
    match kind {
        FilterKind::Deinterlace => {
            "Turns interlaced fields into whole frames (TV and camcorder recordings with combing)."
        }
        FilterKind::Crop => "Cuts pixels off the edges. The fields are the edges as seen in the output.",
        FilterKind::Deshake => {
            "Stabilizes shaky footage with vid.stab in two render passes. Render only: the preview does not show it."
        }
        FilterKind::Orientation => "Turns and mirrors the picture (all eight ways a picture can lie).",
        FilterKind::Resize => "Scales the picture; an empty side keeps the aspect ratio. The output has square pixels.",
    }
}

fn strings(items: impl Iterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(Into::into).collect::<Vec<SharedString>>()))
}

fn index(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(0)
}
