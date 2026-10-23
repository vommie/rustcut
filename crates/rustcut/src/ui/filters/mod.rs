//! The Filters card and cropping on the video. The pure parts are `state` (the chain and every card operation, the
//! preview text, the crop session), `geometry` (frame pixels <-> video area through rotation, mirrors, pixel aspect
//! and letterboxing) and `view` (what the card shows). This bridge keeps the state on the UI thread, handles the
//! callbacks (`input.rs`), sends the preview's video filters to the player's command thread, and runs the picture
//! checks (auto-crop, interlace) on the tokio runtime; their results come back through `slint::spawn_local`, checked
//! against the file they were made for.

mod input;
mod tasks;

pub mod geometry;
pub mod session;
pub mod state;
pub mod view;

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use rustcut_core::edit::{CropAspect, FilterKind};
use rustcut_core::media::{DetectOptions, MediaInfo};
use rustcut_core::settings::{ExportPanelState, Settings};
use rustcut_player::VideoFilter;
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use self::state::{CropMode, FiltersState, InterlaceHint, Source};
use crate::ui::player::PlayerBridge;
use crate::ui::status::{self, Level};
use crate::util::model::sync;
use crate::{CropOverlay, FilterRowView, Filters, MainWindow};

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
    detect: DetectOptions,
    state: RefCell<FiltersState>,
    /// The open file and a number that changes with every open (late results of an older file are dropped).
    file: RefCell<Option<PathBuf>>,
    generation: Cell<u64>,
    idet_task: RefCell<Option<tokio::task::AbortHandle>>,
    crop_task: RefCell<Option<tokio::task::AbortHandle>>,
    /// The video filters last sent to the player.
    sent: RefCell<VideoFilter>,
    rows: Rc<VecModel<FilterRowView>>,
    /// Ctrl is held, and where the pointer is over the video.
    ctrl: Cell<bool>,
    hover: Cell<Option<(f32, f32)>>,
    quick_timer: slint::Timer,
}

impl FiltersBridge {
    /// Takes over the Filters card and the crop overlay. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        player: &Rc<PlayerBridge>,
        runtime: tokio::runtime::Handle,
        settings: &Settings,
        export: &ExportPanelState,
    ) -> Rc<Self> {
        let state = FiltersState::new(
            settings.filters.clone(),
            export.preview_filters,
            export.keep_filters,
            export.filters.clone(),
            export.crop_aspect,
        );
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            player: Rc::downgrade(player),
            runtime,
            detect: DetectOptions {
                program: settings.advanced.ffmpeg.clone(),
                ..DetectOptions::default()
            },
            state: RefCell::new(state),
            file: RefCell::new(None),
            generation: Cell::new(0),
            idet_task: RefCell::new(None),
            crop_task: RefCell::new(None),
            sent: RefCell::new(VideoFilter::default()),
            rows: Rc::new(VecModel::default()),
            ctrl: Cell::new(false),
            hover: Cell::new(None),
            quick_timer: slint::Timer::default(),
        });
        let filters = window.global::<Filters<'_>>();
        filters.set_rows(ModelRc::from(Rc::clone(&bridge.rows)));
        filters.set_deinterlacers(strings(view::DEINTERLACERS.iter().map(|d| d.ffmpeg_name())));
        filters.set_algorithms(strings(view::ALGORITHMS.iter().map(|a| a.ffmpeg_flag())));
        filters.set_aspects(strings(CropAspect::ALL.iter().map(|a| a.label())));
        filters.set_interpolations(strings(["Bicubic", "Bilinear", "Linear", "None"].into_iter()));
        input::connect(&bridge, window);
        let this = Rc::downgrade(&bridge);
        crate::ui::media_info::set_probe_hook(move |info| {
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

    /// What `ui-state.toml` keeps of the filters.
    pub fn save_into(&self, export: &mut ExportPanelState) {
        let st = self.state.borrow();
        export.preview_filters = st.preview;
        export.keep_filters = st.keep;
        export.filters = st.kept_chain();
        export.crop_aspect = st.aspect;
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
        self.status(Level::Info, "Filters reset");
    }

    pub fn rotate(&self, clockwise: bool) {
        self.update(|s| s.rotate(clockwise));
    }

    pub fn mirror(&self, horizontal: bool) {
        self.update(|s| s.toggle_mirror(horizontal));
    }

    /// Starts or finishes editing the crop on the video (the button, the key).
    pub fn toggle_crop_edit(&self) {
        let session = self.state.borrow().crop_session().map(|s| s.mode);
        match session {
            Some(CropMode::Edit) => self.end_crop(true),
            Some(CropMode::Quick) | None => {
                if !self.state.borrow_mut().begin_crop(CropMode::Edit) {
                    self.status(Level::Warning, "The crop can be edited once a video is open.");
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

    fn status(&self, level: Level, message: &str) {
        tracing::info!("[Filters] {message}");
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }

    // --- Showing the state ------------------------------------------------------------------------------------------

    /// The card, the overlay and the player's video filters.
    fn publish(&self) {
        self.publish_card();
        self.publish_overlay();
        self.send_preview();
    }

    fn send_preview(&self) {
        let filter = self.state.borrow().preview_filter();
        if *self.sent.borrow() == filter {
            return;
        }
        if let Some(player) = self.player.upgrade() {
            player.set_video_filter(filter.clone());
        }
        *self.sent.borrow_mut() = filter;
    }

    fn publish_card(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let card = view::card(&self.state.borrow());
        let f = window.global::<Filters<'_>>();
        f.set_has_media(card.has_media);
        f.set_preview(card.preview);
        f.set_keep(card.keep);
        f.set_output(card.output.as_str().into());
        f.set_output_warning(card.output_warning);
        sync(
            &self.rows,
            card.rows
                .iter()
                .map(|r| FilterRowView {
                    kind: kind_id(r.kind).into(),
                    title: r.kind.label().into(),
                    tooltip: kind_tooltip(r.kind).into(),
                    enabled: r.enabled,
                    can_up: r.can_up,
                    can_down: r.can_down,
                })
                .collect(),
        );
        f.set_deinterlacer(index(card.deinterlacer));
        f.set_field_rate(card.field_rate);
        f.set_field_rate_supported(card.field_rate_supported);
        f.set_interlace_hint(card.interlace_hint.as_str().into());
        f.set_interlace_level(card.interlace_level as i32);
        f.set_interlace_detail(card.interlace_detail.as_str().into());
        let c = card.crop;
        f.set_crop_edges(strings([c.top, c.right, c.bottom, c.left].iter().map(u32::to_string)));
        f.set_crop_editing(card.crop_editing);
        f.set_can_edit_crop(card.can_edit_crop);
        f.set_auto_crop_running(card.auto_crop_running);
        f.set_aspect(index(card.aspect));
        f.set_quarter_turns(i32::from(card.quarter_turns));
        f.set_mirror_horizontal(card.mirror_horizontal);
        f.set_mirror_vertical(card.mirror_vertical);
        f.set_resize_width(card.resize_width.as_str().into());
        f.set_resize_height(card.resize_height.as_str().into());
        f.set_resize_auto_width(card.resize_auto_width.as_str().into());
        f.set_resize_auto_height(card.resize_auto_height.as_str().into());
        f.set_algorithm(index(card.resize_algorithm));
        let d = card.deshake;
        f.set_deshake_values(strings(
            [
                d.shakiness.to_string(),
                d.accuracy.to_string(),
                d.stepsize.to_string(),
                d.smoothing.to_string(),
                d.zoom.to_string(),
            ]
            .into_iter(),
        ));
        f.set_deshake_optzoom(i32::from(d.optzoom));
        f.set_deshake_borders(i32::from(d.crop == rustcut_core::edit::DeshakeCrop::Keep));
        f.set_deshake_interpolation(input::interpolation_index(d.interpolation));
        f.set_deshake_tripod(d.tripod);
    }

    fn publish_overlay(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let o = window.global::<CropOverlay<'_>>();
        let (Some(session), Some(view)) = (st.crop_session(), st.crop_view()) else {
            o.set_active(false);
            return;
        };
        let p = view.picture();
        let crop = st.crop();
        let k = view.kept_rect(crop);
        o.set_active(true);
        o.set_quick(session.mode == CropMode::Quick);
        o.set_picture_x(p.x);
        o.set_picture_y(p.y);
        o.set_picture_width(p.width);
        o.set_picture_height(p.height);
        o.set_kept_x(k.x);
        o.set_kept_y(k.y);
        o.set_kept_width(k.width);
        o.set_kept_height(k.height);
        o.set_handle(session.active_handle().map_or(-1, input::handle_index));
        o.set_dragging(session.dragging());
        let kept = view.shown_crop(crop).kept(view.shown_size());
        let lock = match st.aspect {
            CropAspect::Free => String::new(),
            aspect => format!(" · {}", aspect.label()),
        };
        o.set_size_text(
            kept.map(|s| format!("{}×{}{lock}", s.width, s.height))
                .unwrap_or_default()
                .into(),
        );
        o.set_hint(
            match session.mode {
                CropMode::Edit => "Drag a corner, an edge or the area · Enter keeps · Esc restores",
                CropMode::Quick => "Ctrl held: drag the nearest edge or corner · Esc restores",
            }
            .into(),
        );
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
        FilterKind::Rotate => "rotate",
        FilterKind::Mirror => "mirror",
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
        FilterKind::Deshake => "Stabilizes shaky footage with vid.stab in two render passes.",
        FilterKind::Rotate => "Turns the picture in quarter turns.",
        FilterKind::Mirror => "Mirrors the picture left and right or top and bottom.",
        FilterKind::Resize => "Scales the picture; an empty side keeps the aspect ratio. The output has square pixels.",
    }
}

fn strings(items: impl Iterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(Into::into).collect::<Vec<SharedString>>()))
}

fn index(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(0)
}
