//! The mpv core: options, commands and the event thread.
//!
//! [`Player`] is owned by the app (UI thread); [`PlayerHandle`] is the cheap, thread-safe part the video surface needs (the
//! mpv handle for the render context and a way to report the surface's state). Every command goes to the player's command
//! thread (`commands.rs`), because the UI thread renders the video and must not call libmpv's synchronous API. A file
//! opened before the surface exists is loaded as soon as the render context is there: with `vo=libmpv` mpv can only show
//! video while a render context exists. When the surface goes away (the window closes), mpv stops the video and ends a
//! file without audio with "video output initialization failed"; the event thread knows from [`PlayerHandle`] that the
//! surface is gone and reports that end as [`EndReason::SurfaceLost`], not as a failure.

mod audio;
mod commands;
mod events;
mod log;
mod options;
mod video;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use libmpv2::Mpv;

pub use audio::AudioOptions;
use commands::{Command, Session};
pub use events::{EndReason, PlayerEvent, Property};
pub use options::{PlayerOptions, PreviewQuality};
pub use video::VideoFilter;

/// The loudest volume in percent: no amplification above the source (user review 2026-10-27; it was 150).
pub const MAX_VOLUME: f64 = 100.0;

/// Errors of the mpv core.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("libmpv failed: {0}")]
    Mpv(#[from] libmpv2::Error),
    #[error("mpv cannot open paths that are not valid UTF-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
    #[error("a player thread could not be started: {0}")]
    Thread(#[from] std::io::Error),
    #[error("the player's command thread is gone")]
    Stopped,
}

/// How a file is opened.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OpenOptions {
    /// Start position in seconds.
    pub start: Option<f64>,
    /// `Some(true)` plays at once, `Some(false)` opens paused, `None` keeps the current pause state.
    pub play: Option<bool>,
}

/// The thread-safe part of the player that the video surface uses.
#[derive(Clone)]
pub struct PlayerHandle {
    mpv: Arc<Mpv>,
    commands: Sender<Command>,
    /// A render context exists: set once it was created, cleared before it is freed; read by the event thread.
    surface: Arc<AtomicBool>,
}

impl PlayerHandle {
    /// The mpv core, for creating the render context.
    pub(crate) fn mpv(&self) -> &Arc<Mpv> {
        &self.mpv
    }

    /// A render context exists now on a renderer called `gl_renderer`: the preview quality is settled and a file opened
    /// before is loaded (on the command thread).
    pub(crate) fn surface_ready(&self, gl_renderer: &str) {
        self.surface.store(true, Ordering::Release);
        let software = options::is_software_renderer(gl_renderer);
        self.send(Command::SurfaceReady { software });
    }

    /// The render context goes away (window hidden or closed); later files wait for the next one. Called before the
    /// context is freed, so the end of file that freeing it causes is known to come from here.
    pub(crate) fn surface_lost(&self) {
        self.surface.store(false, Ordering::Release);
        self.send(Command::SurfaceLost);
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::debug!("[Player] Command thread is gone; command dropped");
        }
    }
}

/// The mpv core with its event and command threads. Dropping it quits mpv and joins both threads.
pub struct Player {
    handle: PlayerHandle,
    events: Option<JoinHandle<()>>,
    commands: Option<JoinHandle<()>>,
}

impl Player {
    /// Creates the mpv core with `options` and starts its threads. Every event goes to `on_event` on the event thread:
    /// forward it, never block.
    pub fn new(options: &PlayerOptions, on_event: impl Fn(PlayerEvent) + Send + 'static) -> Result<Self, PlayerError> {
        let mpv = Arc::new(Mpv::with_initializer(|init| options.apply(&init))?);
        events::silence(&mpv);
        let client = mpv.create_client(Some("rustcut_events"))?;
        client.disable_deprecated_events()?;
        log::request_log_messages(&client, options.log_level);
        events::observe(&client);

        let (tx, rx) = mpsc::channel();
        let surface = Arc::new(AtomicBool::new(false));
        let session = Session::new(options.preview_quality, options.audio);
        let command_mpv = Arc::clone(&mpv);
        let commands = std::thread::Builder::new()
            .name("rustcut-mpv-commands".into())
            .spawn(move || commands::run(&command_mpv, &rx, session))?;
        let event_tx = tx.clone();
        let event_surface = Arc::clone(&surface);
        let events = std::thread::Builder::new()
            .name("rustcut-mpv-events".into())
            .spawn(move || events::run(&client, &on_event, &event_tx, &event_surface))?;
        tracing::info!(options = ?options, "[Player] mpv core started");
        Ok(Self {
            handle: PlayerHandle {
                mpv,
                commands: tx,
                surface,
            },
            events: Some(events),
            commands: Some(commands),
        })
    }

    /// The part the video surface needs.
    pub fn handle(&self) -> PlayerHandle {
        self.handle.clone()
    }

    /// Opens `path` (replacing the current file). Before the video surface exists the file waits for it.
    pub fn open(&self, path: &Path, options: OpenOptions) -> Result<(), PlayerError> {
        if path.to_str().is_none() {
            return Err(PlayerError::NonUtf8Path(path.to_path_buf()));
        }
        self.command(Command::Open {
            path: path.to_path_buf(),
            start: options.start,
            play: options.play,
        })
    }

    /// Pauses or resumes playback.
    pub fn set_paused(&self, paused: bool) -> Result<(), PlayerError> {
        self.command(Command::SetPaused(paused))
    }

    /// Pauses or resumes; at the end of the file playback starts over.
    pub fn toggle_pause(&self) -> Result<(), PlayerError> {
        self.command(Command::TogglePause)
    }

    /// Jumps to `seconds`; `exact` = to the precise frame (otherwise the nearest keyframe, for dragging).
    pub fn seek(&self, seconds: f64, exact: bool) -> Result<(), PlayerError> {
        self.command(Command::Seek { seconds, exact })
    }

    /// Playback speed (1 = normal; mpv keeps the pitch of the audio).
    pub fn set_speed(&self, speed: f64) -> Result<(), PlayerError> {
        self.command(Command::SetSpeed(speed))
    }

    /// Plays the file in a loop (`true`) or once; applies to the files opened after it as well.
    pub fn set_loop(&self, on: bool) -> Result<(), PlayerError> {
        self.command(Command::SetLoop(on))
    }

    /// Shows the next (or, with `back`, the previous) frame and pauses.
    pub fn frame_step(&self, back: bool) -> Result<(), PlayerError> {
        self.command(Command::FrameStep { back })
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), PlayerError> {
        self.command(Command::SetMuted(muted))
    }

    /// Volume in percent (0 to `MAX_VOLUME`).
    pub fn set_volume(&self, percent: f64) -> Result<(), PlayerError> {
        self.command(Command::SetVolume(percent))
    }

    /// Changes the preview's video filters (a burst of changes is coalesced; an unchanged filter is not set again).
    pub fn set_video_filter(&self, filter: VideoFilter) -> Result<(), PlayerError> {
        self.command(Command::SetVideoFilter(filter))
    }

    /// Changes the preview's audio filters.
    pub fn set_audio(&self, audio: AudioOptions) -> Result<(), PlayerError> {
        self.command(Command::SetAudio(audio))
    }

    /// Changes the hardware decoding (`hwdec`); mpv reinitializes the decoder of a playing file.
    pub fn set_hwdec(&self, hwdec: &str) -> Result<(), PlayerError> {
        self.command(Command::SetHwdec(hwdec.to_owned()))
    }

    /// Changes the color around the video (`#RRGGBB`).
    pub fn set_background(&self, color: &str) -> Result<(), PlayerError> {
        self.command(Command::SetBackground(color.to_owned()))
    }

    fn command(&self, command: Command) -> Result<(), PlayerError> {
        self.handle
            .commands
            .send(command)
            .map_err(|_closed| PlayerError::Stopped)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // `quit` shuts the core down: every client receives MPV_EVENT_SHUTDOWN, so the event thread ends. The command
        // thread sends it and ends; if that thread is gone, quit is sent from here (the render context is gone by now).
        let sent = self.handle.commands.send(Command::Quit).is_ok();
        let joined = self.commands.take().is_some_and(|t| t.join().is_ok());
        if !(sent && joined) {
            tracing::warn!("[Player] The command thread was gone; quitting mpv directly");
            if let Err(e) = self.handle.mpv.command("quit", &[]) {
                tracing::warn!(error = %e, "[Player] mpv did not accept quit; the event thread is left to end");
                return;
            }
        }
        if let Some(thread) = self.events.take()
            && thread.join().is_err()
        {
            tracing::error!("[Player] The mpv event thread panicked");
        }
        tracing::info!("[Player] mpv core stopped");
    }
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
            player.set_volume(50.0).unwrap();
            player
                .open(Path::new("/nonexistent/video.mkv"), OpenOptions::default())
                .unwrap();
            // The observed properties report their values on their own, without a file. They arrive from mpv's core
            // asynchronously: dropping the player at once could shut down before them (the test was flaky that way).
            let mut events = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !events.contains(&PlayerEvent::Property(Property::Muted(false))) {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                match event_rx.recv_timeout(left) {
                    Ok(event) => events.push(event),
                    Err(_timeout) => break,
                }
            }
            drop(player);
            events.extend(event_rx.try_iter());
            done_tx.send(events).unwrap();
        });
        let events = done_rx
            .recv_timeout(Duration::from_secs(15))
            .expect("Player::drop hung");
        assert_eq!(events.last(), Some(&PlayerEvent::Shutdown));
        assert!(
            events.contains(&PlayerEvent::Property(Property::Muted(false))),
            "{events:?}"
        );
    }

    #[test]
    fn the_volume_set_through_the_command_thread_is_reported() {
        let (event_tx, event_rx) = mpsc::channel();
        let player = Player::new(&quiet_options(), move |event| {
            event_tx.send(event).ok();
        })
        .unwrap();
        player.set_volume(42.0).unwrap();
        let seen = event_rx
            .iter()
            .take(50)
            .any(|e| e == PlayerEvent::Property(Property::Volume(42.0)));
        assert!(seen);
    }

    #[test]
    fn paths_that_mpv_cannot_take_are_refused() {
        use std::os::unix::ffi::OsStrExt as _;
        let player = Player::new(&quiet_options(), |_| {}).unwrap();
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/v/\xff.mkv"));
        assert!(matches!(
            player.open(bad, OpenOptions::default()),
            Err(PlayerError::NonUtf8Path(_))
        ));
    }
}
