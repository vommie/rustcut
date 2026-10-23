//! Bridge between the player (`rustcut-player`) and the main window: the video surface, opening files, player events.
//!
//! Player events arrive on mpv's event thread and reach the UI through [`SlintSink`] (the dispatcher's sink). They move onto
//! the full [`UiDispatcher`](crate::ui::dispatcher::UiDispatcher) with the tokio runtime in M1.3.

use std::path::Path;

use az::CheckedAs as _;
use rustcut_player::{Player, PlayerError, PlayerEvent, PlayerOptions, SurfaceError, SurfaceHooks, attach_surface};
use slint::ComponentHandle as _;

use crate::MainWindow;
use crate::ui::dispatcher::{SlintSink, UiSink as _};

/// Errors while starting the player.
#[derive(Debug, thiserror::Error)]
pub enum PlayerBridgeError {
    #[error(transparent)]
    Player(#[from] PlayerError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
}

/// The running player of the main window. Dropping it stops mpv.
pub struct PlayerBridge {
    player: Player,
}

impl PlayerBridge {
    /// Starts mpv and attaches its video surface to `window`.
    pub fn start(window: &MainWindow) -> Result<Self, PlayerBridgeError> {
        let events_window = window.as_weak();
        let player = Player::new(&PlayerOptions::default(), move |event| {
            on_event(&event);
            let window = events_window.clone();
            SlintSink.run_on_ui(Box::new(move || {
                if let Some(window) = window.upgrade() {
                    apply_event(&window, &event);
                }
            }));
        })?;

        let size_window = window.as_weak();
        let frame_window = window.as_weak();
        attach_surface(
            window,
            player.handle(),
            SurfaceHooks {
                target_size: Box::new(move || size_window.upgrade().map_or_else(Default::default, |w| video_size(&w))),
                present: Box::new(move |image| {
                    if let Some(w) = frame_window.upgrade() {
                        w.set_video_frame(image);
                    }
                }),
            },
        )?;
        Ok(Self { player })
    }

    /// Opens `path` in the player (it waits for the video surface if the window has not rendered yet).
    pub fn open(&self, path: &Path) {
        if let Err(e) = self.player.open(path) {
            tracing::error!(error = %e, path = %path.display(), "[Player] The file could not be opened");
        }
    }
}

/// The video area's size in physical pixels.
fn video_size(window: &MainWindow) -> slint::PhysicalSize {
    let scale = window.window().scale_factor();
    let px = |logical: f32| (logical * scale).round().checked_as::<u32>().unwrap_or(0);
    slint::PhysicalSize::new(px(window.get_video_width()), px(window.get_video_height()))
}

/// Logs a player event (on mpv's event thread).
fn on_event(event: &PlayerEvent) {
    match event {
        PlayerEvent::Error(message) => tracing::warn!(message = %message, "[Player] Playback error"),
        PlayerEvent::Ended(reason) => tracing::info!(reason = ?reason, "[Player] Playback ended"),
        other => tracing::debug!(event = ?other, "[Player] Event"),
    }
}

/// Shows a player event in the window (UI thread).
fn apply_event(window: &MainWindow, event: &PlayerEvent) {
    match event {
        PlayerEvent::FileLoaded => window.set_has_video(true),
        PlayerEvent::Ended(rustcut_player::EndReason::Error) | PlayerEvent::Shutdown => window.set_has_video(false),
        _ => {}
    }
}
