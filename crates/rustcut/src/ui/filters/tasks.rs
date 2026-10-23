//! The picture checks on the tokio runtime: auto-crop (black borders sampled over the video) and the interlace check.
//! Each runs as a task whose ffmpeg is killed when the task is aborted (another file, quit); the result comes back to
//! the UI thread through `slint::spawn_local` and is dropped when another file was opened meanwhile.

use std::rc::Weak;

use rustcut_core::edit::{FilterKind, LavfiTarget};
use rustcut_core::media::{detect_crop, detect_interlace};

use super::FiltersBridge;
use super::state::{Edge, InterlaceHint, Source};
use crate::ui::status::Level;

impl FiltersBridge {
    /// Starts the interlace check of the open file.
    pub(super) fn check_interlace(&self, source: Source) {
        let Some(file) = self.file.borrow().clone() else { return };
        self.state.borrow_mut().interlace = InterlaceHint::Checking;
        let options = self.detect.clone();
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
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no interlace check");
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
            self.status(Level::Warning, "Black borders can be detected once a video is open.");
            return;
        };
        self.state.borrow_mut().auto_crop_running = true;
        self.publish_card();
        let options = self.detect.clone();
        let task = self
            .runtime
            .spawn(async move { detect_crop(&file, frame, duration, &prefix, &options).await });
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
                    bridge.status(Level::Warning, "Only black frames were found; nothing to crop.");
                }
                Ok(found) if found.crop.is_identity() => {
                    bridge.publish_card();
                    bridge.status(Level::Info, "No black borders found.");
                }
                Ok(found) => {
                    tracing::info!(crop = ?found.crop, samples = found.samples_used, "[Filters] Black borders");
                    bridge.update(|s| s.set_crop(found.crop));
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
                    bridge.status(
                        Level::Success,
                        &format!("Black borders cropped: {} pixels", parts.join(", ")),
                    );
                }
                Err(e) => {
                    bridge.publish_card();
                    bridge.status(Level::Error, &format!("Black borders not detected: {e}"));
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Filters] The event loop is gone; no auto-crop");
        }
    }
}
