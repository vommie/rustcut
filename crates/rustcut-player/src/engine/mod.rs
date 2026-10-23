//! The mpv core: options, commands and the event thread.
//!
//! [`Player`] is owned by the app (UI thread); [`PlayerHandle`] is the cheap, thread-safe part the video surface needs (the
//! mpv handle for the render context, and the "surface ready" state). A file opened before the surface exists is loaded as
//! soon as the render context is there: with `vo=libmpv` mpv can only show video while a render context exists.

mod events;
mod log;
mod options;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use libmpv2::Mpv;
use parking_lot::Mutex;

pub use events::{EndReason, PlayerEvent};
pub use options::{PlayerOptions, PreviewQuality};

/// Errors of the mpv core.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("libmpv failed: {0}")]
    Mpv(#[from] libmpv2::Error),
    #[error("mpv cannot open paths that are not valid UTF-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
    #[error("the mpv event thread could not be started: {0}")]
    Thread(#[from] std::io::Error),
}

/// What the video surface and the player share: whether a render context exists, and the file waiting for it.
#[derive(Debug, Default)]
struct SurfaceState {
    ready: bool,
    pending: Option<PathBuf>,
    quality: PreviewQuality,
}

/// The thread-safe part of the player that the video surface uses.
#[derive(Clone)]
pub struct PlayerHandle {
    mpv: Arc<Mpv>,
    surface: Arc<Mutex<SurfaceState>>,
}

impl PlayerHandle {
    /// The mpv core, for creating the render context.
    pub(crate) fn mpv(&self) -> &Arc<Mpv> {
        &self.mpv
    }

    /// A render context exists now on a renderer called `gl_renderer`: the preview quality is settled and a file opened
    /// before is loaded.
    pub(crate) fn surface_ready(&self, gl_renderer: &str) {
        let (pending, quality) = {
            let mut state = self.surface.lock();
            state.ready = true;
            (state.pending.take(), state.quality)
        };
        if quality == PreviewQuality::Auto && options::is_software_renderer(gl_renderer) {
            match self.mpv.command("apply-profile", &[options::FAST_PROFILE]) {
                Ok(()) => tracing::info!("[Player] Software OpenGL: fast preview scaling"),
                Err(e) => tracing::warn!(error = %e, "[Player] The fast preview profile could not be applied"),
            }
        }
        if let Some(path) = pending {
            tracing::info!(path = %path.display(), "[Player] Video surface ready, loading the waiting file");
            if let Err(e) = load(&self.mpv, &path) {
                tracing::error!(error = %e, path = %path.display(), "[Player] Loading the file failed");
            }
        }
    }

    /// The render context is gone (window hidden or closed); later files wait for the next one.
    pub(crate) fn surface_lost(&self) {
        self.surface.lock().ready = false;
    }

    /// Reads an mpv property as text, `None` when it is not available (no file, no video).
    pub fn property_text(&self, name: &str) -> Option<String> {
        self.mpv.get_property::<String>(name).ok()
    }

    /// Reads a numeric mpv property, `None` when it is not available.
    pub fn property_f64(&self, name: &str) -> Option<f64> {
        self.mpv.get_property::<f64>(name).ok()
    }

    /// Reads an integer mpv property, `None` when it is not available.
    pub fn property_i64(&self, name: &str) -> Option<i64> {
        self.mpv.get_property::<i64>(name).ok()
    }
}

/// The mpv core with its event thread. Dropping it quits mpv and joins the thread.
pub struct Player {
    handle: PlayerHandle,
    events: Option<JoinHandle<()>>,
}

impl Player {
    /// Creates the mpv core with `options` and starts the event thread, which hands every event to `on_event` (on that
    /// thread: forward it, never block).
    pub fn new(options: &PlayerOptions, on_event: impl Fn(PlayerEvent) + Send + 'static) -> Result<Self, PlayerError> {
        let mpv = Mpv::with_initializer(|init| options.apply(&init))?;
        let client = mpv.create_client(Some("rustcut_events"))?;
        client.disable_deprecated_events()?;
        log::request_log_messages(&client, options.log_level);
        let events = std::thread::Builder::new()
            .name("rustcut-mpv-events".into())
            .spawn(move || events::run(&client, &on_event))?;
        tracing::info!(options = ?options, "[Player] mpv core started");
        Ok(Self {
            handle: PlayerHandle {
                mpv: Arc::new(mpv),
                surface: Arc::new(Mutex::new(SurfaceState {
                    quality: options.preview_quality,
                    ..SurfaceState::default()
                })),
            },
            events: Some(events),
        })
    }

    /// The part the video surface needs.
    pub fn handle(&self) -> PlayerHandle {
        self.handle.clone()
    }

    /// Opens `path` (replacing the current file). Before the video surface exists the file waits for it.
    pub fn open(&self, path: &Path) -> Result<(), PlayerError> {
        {
            let mut state = self.handle.surface.lock();
            if !state.ready {
                tracing::info!(path = %path.display(), "[Player] Waiting for the video surface before loading");
                state.pending = Some(path.to_path_buf());
                return Ok(());
            }
        }
        load(&self.handle.mpv, path)
    }

    /// Pauses or resumes playback.
    pub fn set_paused(&self, paused: bool) -> Result<(), PlayerError> {
        self.handle.mpv.set_property("pause", paused)?;
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // `quit` shuts the core down: every client receives MPV_EVENT_SHUTDOWN, so the event thread ends.
        match self.handle.mpv.command("quit", &[]) {
            Ok(()) => {
                if let Some(thread) = self.events.take()
                    && thread.join().is_err()
                {
                    tracing::error!("[Player] The mpv event thread panicked");
                }
            }
            Err(e) => tracing::warn!(error = %e, "[Player] mpv did not accept quit; the event thread is left to end"),
        }
        tracing::info!("[Player] mpv core stopped");
    }
}

fn load(mpv: &Mpv, path: &Path) -> Result<(), PlayerError> {
    let text = path
        .to_str()
        .ok_or_else(|| PlayerError::NonUtf8Path(path.to_path_buf()))?;
    tracing::info!(path = %path.display(), "[Player] Loading");
    mpv.command("loadfile", &[text, "replace"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn quiet_options() -> PlayerOptions {
        PlayerOptions {
            log_level: "fatal",
            ..PlayerOptions::default()
        }
    }

    #[test]
    fn the_core_starts_and_stops_without_hanging() {
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (event_tx, event_rx) = mpsc::channel();
            let player = Player::new(&quiet_options(), move |event| {
                event_tx.send(event).ok();
            })
            .unwrap();
            drop(player);
            let events: Vec<_> = event_rx.try_iter().collect();
            done_tx.send(events).unwrap();
        });
        let events = done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Player::drop hung");
        assert_eq!(events.last(), Some(&PlayerEvent::Shutdown));
    }

    #[test]
    fn a_file_opened_before_the_surface_waits_for_it() {
        let player = Player::new(&quiet_options(), |_| {}).unwrap();
        player.open(Path::new("/nonexistent/video.mkv")).unwrap();
        let state = player.handle.surface.lock();
        assert!(!state.ready);
        assert_eq!(state.pending.as_deref(), Some(Path::new("/nonexistent/video.mkv")));
    }

    #[test]
    fn the_surface_takes_the_waiting_file() {
        let player = Player::new(&quiet_options(), |_| {}).unwrap();
        player.open(Path::new("/nonexistent/video.mkv")).unwrap();
        player.handle().surface_ready("AMD Radeon (radeonsi)");
        assert!(player.handle.surface.lock().pending.is_none());
        player.handle().surface_lost();
        assert!(!player.handle.surface.lock().ready);
    }
}
