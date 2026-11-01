//! The picture checks on the tokio runtime: auto-crop (black borders sampled over the video) and the interlace check.
//! Each runs as a task whose ffmpeg is killed when the task is aborted (another file, quit); the result comes back to
//! the UI thread through `slint::spawn_local` and is dropped when another file was opened meanwhile. Auto-crop shows in
//! the status bar's activity segment and on its buttons (P12 4.1): the throbber with its count, the result, a glow.

use std::rc::{Rc, Weak};
use std::time::Duration;

use rustcut_core::edit::{FilterKind, LavfiTarget};
use rustcut_core::media::{CropProgress, crop_sample_times, detect_crop_watched, detect_interlace, pick_color};
use slint::ComponentHandle as _;

use super::FiltersBridge;
use super::history_view::edges_text;
use super::state::{CropMode, InterlaceHint, Source};
use crate::Filters;
use crate::ui::notices::{Kind, Level, Notice};
use crate::ui::status::activity;

/// The status bar's segment of auto-crop.
const ACTIVITY: &str = "auto-crop";
/// The full glow on the buttons before it fades out.
const FLASH_TIME: Duration = Duration::from_millis(120);

/// How a run ended, for the buttons' glow.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Cropped = 1,
    Nothing = 2,
    Failed = 3,
}

/// What the status bar says while the samples are read.
fn waiting_text(progress: CropProgress) -> String {
    let CropProgress { done, total, attempt } = progress;
    let tries = if attempt > 1 {
        format!(" (try {attempt})")
    } else {
        String::new()
    };
    format!("Waiting for Auto-Crop (ffmpeg)... {done}/{total}{tries}")
}

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

    /// Auto-crop's segment shows `text`, its buttons wait, and a run that is older is dropped from now on.
    fn crop_work_started(&self, text: &str) {
        self.crop_run.set(self.crop_run.get().wrapping_add(1));
        self.state.borrow_mut().auto_crop_running = true;
        let me = Weak::clone(&self.me);
        let cancel: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(bridge) = me.upgrade() {
                bridge.cancel_auto_crop();
            }
        });
        activity::begin(ACTIVITY, text, Some(cancel));
        self.publish_card();
    }

    /// Nothing runs any more; a result showing in the segment stays for its time.
    pub(super) fn crop_work_ended(&self) {
        self.crop_run.set(self.crop_run.get().wrapping_add(1));
        self.state.borrow_mut().auto_crop_running = false;
        activity::finish(ACTIVITY);
    }

    /// The segment's ×, a loaded job: ffmpeg stops, the crop stays as it is. `false` when nothing ran.
    pub(super) fn cancel_auto_crop(&self) -> bool {
        if !self.state.borrow().auto_crop_running {
            return false;
        }
        if let Some(task) = self.crop_task.borrow_mut().take() {
            task.abort();
        }
        self.crop_work_ended();
        self.publish_card();
        true
    }

    /// The user changed the crop while auto-crop runs: that change wins.
    pub(super) fn user_took_over(&self) {
        if self.cancel_auto_crop() {
            Self::tell(
                Notice::new(Kind::AutoCrop, "Canceled")
                    .detail("you changed the crop")
                    .level(Level::Info)
                    .activity(ACTIVITY),
            );
        }
    }

    /// Reports a run's end: the message, the segment's end and the glow.
    fn crop_result(&self, notice: Notice, outcome: Outcome) {
        self.crop_work_ended();
        self.publish_card();
        Self::tell(notice.activity(ACTIVITY));
        let Some(window) = self.window.upgrade() else { return };
        let filters = window.global::<Filters<'_>>();
        filters.set_crop_flash_level(outcome as i32);
        filters.set_crop_flash(true);
        let window = window.as_weak();
        self.flash_timer
            .start(slint::TimerMode::SingleShot, FLASH_TIME, move || {
                if let Some(window) = window.upgrade() {
                    window.global::<Filters<'_>>().set_crop_flash(false);
                }
            });
    }

    /// The right click on auto-crop: starts the pipette, or ends it while it waits for a click.
    pub(super) fn toggle_pick(&self) {
        let picking = self
            .state
            .borrow()
            .crop_session()
            .is_some_and(|s| s.mode == CropMode::Pick);
        if picking {
            self.update(|s| s.end_crop(false));
        } else {
            self.start_pick();
        }
    }

    /// The Crop menu's "pick the border color": the whole frame shows and the next click on the video picks.
    pub fn start_pick(&self) {
        if self.suspended.get() || self.state.borrow().auto_crop_running {
            return;
        }
        if self.state.borrow().crop_session().is_some() {
            self.end_crop(true);
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
        self.crop_work_started("Reading the color (ffmpeg)...");
        let options = self.detect.borrow().clone();
        let task = self
            .runtime
            .spawn(async move { pick_color(&file, at, &prefix, point, &options).await });
        *self.crop_task.borrow_mut() = Some(task.abort_handle());
        let run = self.crop_run.get();
        let bridge = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.crop_run.get() != run {
                return;
            }
            match result {
                Ok(rgb) => {
                    tracing::info!(color = ?rgb, "[Filters] Border color");
                    // The badge shows at once; the segment goes straight on with the search.
                    bridge.state.borrow_mut().auto_crop_running = false;
                    bridge.update(|s| s.set_search(|search| search.color = Some(rgb)));
                    bridge.auto_crop();
                }
                Err(e) => {
                    bridge.crop_result(
                        Notice::new(Kind::AutoCropFailed, "The color could not be read").detail(e.to_string()),
                        Outcome::Failed,
                    );
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
        let total = times.len();
        self.crop_work_started(&waiting_text(CropProgress {
            done: 0,
            total,
            attempt: 1,
        }));
        let options = self.detect.borrow().clone();
        let (sender, mut progress) = tokio::sync::mpsc::unbounded_channel();
        let task = self.runtime.spawn(async move {
            let tell = move |update: CropProgress| {
                // Closed only when the UI gave up on this run.
                let _ignored = sender.send(update);
            };
            detect_crop_watched(&file, frame, &times, &prefix, &search, &options, &tell).await
        });
        *self.crop_task.borrow_mut() = Some(task.abort_handle());
        let run = self.crop_run.get();
        let bridge = Weak::clone(&self.me);
        let counter = Weak::clone(&self.me);
        let counting = slint::spawn_local(async move {
            while let Some(update) = progress.recv().await {
                let Some(bridge) = counter.upgrade() else { return };
                if bridge.crop_run.get() != run {
                    return;
                }
                activity::update(ACTIVITY, waiting_text(update));
            }
        });
        if let Err(e) = counting {
            tracing::warn!(error = %e, "[Filters] The event loop is gone; no auto-crop count");
        }
        let spawned = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.crop_run.get() != run {
                return;
            }
            match result {
                Ok(found) if found.samples_used == 0 => bridge.crop_result(
                    Notice::new(Kind::AutoCrop, "Nothing to crop")
                        .detail("Only black frames were found.")
                        .level(Level::Warning),
                    Outcome::Nothing,
                ),
                Ok(found) if found.crop.is_identity() => {
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
                    bridge.crop_result(
                        Notice::new(Kind::AutoCrop, format!("{what} found{tries}")).level(Level::Warning),
                        Outcome::Nothing,
                    );
                }
                Ok(found) => {
                    tracing::info!(crop = ?found.crop, samples = found.samples_used, "[Filters] Black borders");
                    bridge.update(|s| {
                        let fitted = s.fitted_crop(found.crop);
                        s.set_crop(fitted);
                        s.switch_on(FilterKind::Crop);
                        s.crop_replaced();
                    });
                    bridge.remember_crop();
                    let (edges, size) = {
                        let st = bridge.state.borrow();
                        (edges_text(st.shown_crop()), st.shown_kept().unwrap_or_default())
                    };
                    let title = if found.search.color.is_some() {
                        "Cropped to the picked color"
                    } else {
                        "Cropped"
                    };
                    let retried = match (found.tries, found.search.color) {
                        (1, _) => String::new(),
                        (n, Some(_)) => format!(" (try {n}: tolerance {} %)", found.search.similarity),
                        (n, None) => format!(" (try {n}: black up to {})", found.search.limit),
                    };
                    bridge.crop_result(
                        Notice::new(Kind::AutoCrop, title).detail(format!("{edges} → {size}{retried}")),
                        Outcome::Cropped,
                    );
                }
                Err(e) => bridge.crop_result(
                    Notice::new(Kind::AutoCropFailed, "Auto-crop failed").detail(e.to_string()),
                    Outcome::Failed,
                ),
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no auto-crop");
        }
    }
}
