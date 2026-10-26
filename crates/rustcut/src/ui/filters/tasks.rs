//! The picture checks on the tokio runtime: auto-crop (black borders sampled over the video) and the interlace check.
//! Each runs as a task whose ffmpeg is killed when the task is aborted (another file, quit); the result comes back to
//! the UI thread through `slint::spawn_local` and is dropped when another file was opened meanwhile.

use std::rc::Weak;

use rustcut_core::edit::{FilterKind, LavfiTarget};
use rustcut_core::media::{crop_sample_times, detect_crop_retrying, detect_interlace, pick_color};

use super::FiltersBridge;
use super::state::{CropMode, Edge, InterlaceHint, Source};
use crate::ui::notices::{Kind, Level, Notice};

impl FiltersBridge {
    /// Starts the interlace check of the open file.
    pub(super) fn check_interlace(&self, source: Source) {
        let Some(file) = self.file.borrow().clone() else { return };
        self.state.borrow_mut().interlace = InterlaceHint::Checking;
        let options = self.detect.borrow().clone();
        let task = self
            .runtime
            .spawn(async move { detect_interlace(&file, source.duration, &options).await });
        *self.idet_task.borrow_mut() = Some(task.abort_handle());
        let generation = self.generation.get();
        let bridge = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.generation.get() != generation {
                return;
            }
            let hint = match result {
                Ok(counts) => {
                    tracing::info!(counts = ?counts, verdict = ?counts.verdict(), "[Filters] Interlace check");
                    InterlaceHint::Known(counts.verdict(), counts)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "[Filters] Interlace check failed");
                    InterlaceHint::Failed
                }
            };
            bridge.state.borrow_mut().interlace = hint;
            bridge.publish_card();
            bridge.tell_interlaced(hint);
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no interlace check");
        }
    }

    /// The optional message about an interlaced video (Q-N10: by default only the card's note says it): while
    /// Deinterlace is off, with a button that switches it on.
    fn tell_interlaced(&self, hint: InterlaceHint) {
        let InterlaceHint::Known(rustcut_core::media::Interlacing::Interlaced { top_field_first }, _) = hint else {
            return;
        };
        if self.state.borrow().chain.is_enabled(FilterKind::Deinterlace) {
            return;
        }
        let bridge = Weak::clone(&self.me);
        crate::ui::notices::post(
            Notice::new(Kind::Interlaced, "The video is interlaced")
                .detail(format!(
                    "{} field first: switch Deinterlace on, or the output shows combing on movement.",
                    if top_field_first { "Top" } else { "Bottom" }
                ))
                .action("Switch Deinterlace on", move || {
                    if let Some(bridge) = bridge.upgrade() {
                        bridge.update(|s| s.switch_on(FilterKind::Deinterlace));
                    }
                }),
        );
    }

    /// The Crop menu's "pick the border color": the whole frame shows and the next click on the video picks.
    pub fn start_pick(&self) {
        if self.suspended.get() {
            return;
        }
        if self.state.borrow().crop_session().is_some() {
            self.state.borrow_mut().end_crop(true);
        }
        if self.state.borrow_mut().begin_crop(CropMode::Pick) {
            tracing::debug!("[Filters] Picking a border color");
            // An open menu that is not pinned would take the click on the video (a press outside closes it).
            if !self.menu_pinned.get() {
                self.menu_kind.set(None);
            }
        }
        self.publish();
    }

    /// A click while picking: the color of the frame's pixel there (read by ffmpeg at the playhead), then auto-crop
    /// with it (an action: the crop switches on).
    pub(super) fn pick_at(&self, x: f32, y: f32) {
        let point = self.state.borrow().crop_view().and_then(|view| view.frame_point(x, y));
        self.state.borrow_mut().end_crop(false);
        self.publish();
        let Some(point) = point else { return };
        let job = {
            let st = self.state.borrow();
            let prefix = st.chain().before(FilterKind::Crop).to_lavfi(LavfiTarget::Preview);
            match (self.file.borrow().clone(), prefix) {
                (Some(file), Ok(prefix)) => Some((file, prefix)),
                _ => None,
            }
        };
        let Some((file, prefix)) = job else { return };
        let at = self
            .player
            .upgrade()
            .map_or(rustcut_core::time::Timestamp::ZERO, |p| p.position());
        tracing::info!(x = point.0, y = point.1, at = %at, "[Filters] Border color picked at");
        let options = self.detect.borrow().clone();
        let task = self
            .runtime
            .spawn(async move { pick_color(&file, at, &prefix, point, &options).await });
        let generation = self.generation.get();
        let bridge = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.generation.get() != generation {
                return;
            }
            match result {
                Ok(rgb) => {
                    tracing::info!(color = ?rgb, "[Filters] Border color");
                    bridge.update(|s| s.set_search(|search| search.color = Some(rgb)));
                    bridge.auto_crop();
                }
                Err(e) => {
                    Self::tell(Notice::new(Kind::AutoCropFailed, "The color could not be read").detail(e.to_string()));
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no color pick");
        }
    }

    /// Detects the black borders over several points of the video and crops them (the button, the key).
    pub fn auto_crop(&self) {
        let job = {
            let st = self.state.borrow();
            if st.auto_crop_running {
                return;
            }
            let prefix = st.chain().before(FilterKind::Crop).to_lavfi(LavfiTarget::Preview);
            match (self.file.borrow().clone(), st.source(), st.crop_frame(), prefix) {
                (Some(file), Some(source), Some(frame), Ok(prefix)) => Some((file, source.duration, frame, prefix)),
                _ => None,
            }
        };
        let Some((file, duration, frame, prefix)) = job else {
            // Without a video the card is closed (the state manager, M6.5e) and its key does nothing.
            tracing::debug!("[Filters] No video: no black border detection");
            return;
        };
        let search = self.state.borrow().search;
        let ranges = if search.sections_only {
            self.sections
                .borrow()
                .as_ref()
                .map(|source| source())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let times = crop_sample_times(duration, usize::from(search.samples), &ranges);
        tracing::info!(
            samples = times.len(),
            in_sections = !ranges.is_empty(),
            limit = search.limit,
            sides = ?search.sides,
            "[Filters] Auto-crop"
        );
        self.state.borrow_mut().auto_crop_running = true;
        self.publish_card();
        let options = self.detect.borrow().clone();
        let task = self
            .runtime
            .spawn(async move { detect_crop_retrying(&file, frame, &times, &prefix, &search, &options).await });
        *self.crop_task.borrow_mut() = Some(task.abort_handle());
        let generation = self.generation.get();
        let bridge = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.generation.get() != generation {
                return;
            }
            bridge.state.borrow_mut().auto_crop_running = false;
            match result {
                Ok(found) if found.samples_used == 0 => {
                    bridge.publish_card();
                    Self::tell(
                        Notice::new(Kind::AutoCrop, "Nothing to crop")
                            .detail("Only black frames were found.")
                            .level(Level::Warning),
                    );
                }
                Ok(found) if found.crop.is_identity() => {
                    bridge.publish_card();
                    let what = if found.search.color.is_some() {
                        "No borders of that color"
                    } else {
                        "No black borders"
                    };
                    let tries = if found.tries > 1 {
                        format!(" in {} tries", found.tries)
                    } else {
                        String::new()
                    };
                    Self::tell(Notice::new(Kind::AutoCrop, format!("{what} found{tries}")).level(Level::Info));
                }
                Ok(found) => {
                    tracing::info!(crop = ?found.crop, samples = found.samples_used, "[Filters] Black borders");
                    bridge.update(|s| {
                        let fitted = s.fitted_crop(found.crop);
                        s.set_crop(fitted);
                        s.switch_on(FilterKind::Crop);
                    });
                    let shown = bridge.state.borrow().shown_crop();
                    let parts: Vec<String> = [
                        (Edge::Top, "top"),
                        (Edge::Right, "right"),
                        (Edge::Bottom, "bottom"),
                        (Edge::Left, "left"),
                    ]
                    .iter()
                    .filter_map(|(edge, name)| {
                        let v = match edge {
                            Edge::Top => shown.top,
                            Edge::Right => shown.right,
                            Edge::Bottom => shown.bottom,
                            Edge::Left => shown.left,
                        };
                        (v > 0).then(|| format!("{v} {name}"))
                    })
                    .collect();
                    let title = if found.search.color.is_some() {
                        "Borders of the picked color cropped"
                    } else {
                        "Black borders cropped"
                    };
                    let retried = match (found.tries, found.search.color) {
                        (1, _) => String::new(),
                        (n, Some(_)) => format!(" (try {n}: tolerance {} %)", found.search.similarity),
                        (n, None) => format!(" (try {n}: black up to {})", found.search.limit),
                    };
                    Self::tell(
                        Notice::new(Kind::AutoCrop, title).detail(format!("{} pixels{retried}", parts.join(", "))),
                    );
                }
                Err(e) => {
                    bridge.publish_card();
                    Self::tell(Notice::new(Kind::AutoCropFailed, "Black borders not detected").detail(e.to_string()));
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no auto-crop");
        }
    }
}
