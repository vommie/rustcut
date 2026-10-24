//! Bridge between the player (`rustcut-player`) and the main window: the video surface, the `Playback` global (transport
//! bar, menus, keys, error overlay), opening files.
//!
//! Player events arrive on mpv's event thread. They update the shared [`PlaybackState`] (a short lock, never held across a
//! Slint call) and post the resulting [`PlaybackView`] through the [`UiDispatcher`], coalesced to the playhead rate: every
//! post is the complete state, so the latest one wins. Commands go from the Slint callbacks straight to the player, whose
//! command thread talks to mpv. Seeks, frame steps and the J/K/L shuttle live in `transport.rs`.

pub mod shuttle;
mod state;
mod transport;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use az::CheckedAs as _;
use parking_lot::Mutex;
use rustcut_core::settings::{ExportPanelState, PlayerSettings};
use rustcut_core::time::Timestamp;
use rustcut_player::{
    AudioOptions, EndReason, OpenOptions, Player, PlayerError, PlayerEvent, PlayerOptions, SurfaceError, SurfaceHooks,
    VideoFilter, attach_surface,
};
use slint::ComponentHandle as _;

pub use shuttle::ShuttleKey;
pub use state::{PlaybackState, PlaybackView};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::{MainWindow, Playback, Status};

/// The one coalesced update of the playback view.
const VIEW_KEY: CoalesceKey = CoalesceKey::new("playback", 0);
/// One mouse wheel notch in Slint's `delta-y` on X11 (measured in RustView: winit reports two lines of 60 px); touchpads
/// send pixels that add up to a notch the same way.
const WHEEL_NOTCH: f32 = 120.0;

/// The function that sees every playback view.
type ViewHook = Box<dyn Fn(&PlaybackView)>;

thread_local! {
    /// Called on the UI thread with every playback view after it is shown (the timeline follows the playhead). Views are
    /// posted from mpv's event thread as `Send` closures, which cannot carry the UI-thread objects themselves.
    static VIEW_HOOK: RefCell<Option<ViewHook>> = const { RefCell::new(None) };
}

/// Installs the function that sees every playback view (UI thread; replaces the previous one).
pub fn set_view_hook(hook: impl Fn(&PlaybackView) + 'static) {
    VIEW_HOOK.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
}

/// Errors while starting the player.
#[derive(Debug, thiserror::Error)]
pub enum PlayerBridgeError {
    #[error(transparent)]
    Player(#[from] PlayerError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
}

/// The running player of the main window. The app owns the only strong reference; dropping it stops mpv.
pub struct PlayerBridge {
    player: Player,
    state: Arc<Mutex<PlaybackState>>,
    window: slint::Weak<MainWindow>,
    auto_play: Cell<bool>,
    wheel_step: Cell<u8>,
    /// Wheel movement that has not made a whole notch yet (touchpads send small steps).
    wheel_rest: Cell<f32>,
    /// The emulated backward playback while J is active, and the timer that moves it.
    reverse: Cell<Option<shuttle::Reverse>>,
    reverse_timer: slint::Timer,
}

impl PlayerBridge {
    /// Starts mpv with the player settings and the saved volume, attaches its video surface to `window` and handles the
    /// `Playback` callbacks from now on.
    pub fn start(
        window: &MainWindow,
        ui: &UiDispatcher,
        settings: &PlayerSettings,
        export: &ExportPanelState,
    ) -> Result<Rc<Self>, PlayerBridgeError> {
        let options = PlayerOptions {
            hwdec: settings.hwdec.clone(),
            autoplay: settings.auto_play,
            background: settings.background.clone(),
            audio: AudioOptions {
                mute_last_second: settings.mute_last_second,
                loudnorm: settings.loudnorm_preview,
            },
            volume: f64::from(export.volume),
            muted: export.muted,
            ..PlayerOptions::default()
        };
        let state = Arc::new(Mutex::new(PlaybackState {
            volume: export.volume,
            muted: export.muted,
            ..PlaybackState::default()
        }));

        let events_state = Arc::clone(&state);
        let events_ui = ui.clone();
        let events_window = window.as_weak();
        let player = Player::new(&options, move |event| {
            log_event(&event);
            let view = {
                let mut state = events_state.lock();
                state.apply(&event);
                state.view()
            };
            let window = events_window.clone();
            events_ui.post_latest(VIEW_KEY, rate::PLAYHEAD, move || {
                if let Some(window) = window.upgrade() {
                    show(&window, &view);
                }
            });
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

        let bridge = Rc::new(Self {
            player,
            state,
            window: window.as_weak(),
            auto_play: Cell::new(settings.auto_play),
            wheel_step: Cell::new(settings.wheel_volume_step),
            wheel_rest: Cell::new(0.0),
            reverse: Cell::new(None),
            reverse_timer: slint::Timer::default(),
        });
        bridge.connect(window);
        bridge.refresh();
        Ok(bridge)
    }

    /// The player settings changed in the Settings window: auto play and the wheel step hold from now on; the preview
    /// audio filters, the hardware decoding and the background change at once.
    pub fn apply_settings(&self, before: &PlayerSettings, settings: &PlayerSettings) {
        self.auto_play.set(settings.auto_play);
        self.wheel_step.set(settings.wheel_volume_step);
        let audio = |s: &PlayerSettings| AudioOptions {
            mute_last_second: s.mute_last_second,
            loudnorm: s.loudnorm_preview,
        };
        if audio(before) != audio(settings)
            && let Err(e) = self.player.set_audio(audio(settings))
        {
            tracing::warn!(error = %e, "[Player] The preview audio filters could not be changed");
        }
        if before.hwdec != settings.hwdec
            && let Err(e) = self.player.set_hwdec(&settings.hwdec)
        {
            tracing::warn!(error = %e, "[Player] The hardware decoding could not be changed");
        }
        if before.background != settings.background
            && let Err(e) = self.player.set_background(&settings.background)
        {
            tracing::warn!(error = %e, "[Player] The background could not be changed");
        }
    }

    /// Opens `path` (it waits for the video surface if the window has not rendered yet). Auto play decides whether it
    /// starts playing.
    pub fn open(&self, path: &Path) -> Result<(), PlayerError> {
        self.end_reverse();
        self.state.lock().opening(path);
        self.refresh();
        self.player.open(
            path,
            OpenOptions {
                start: None,
                play: Some(self.auto_play.get()),
            },
        )
    }

    /// Shows the probe render `path` instead of the source: from its start, playing, in a loop.
    pub fn open_probe(&self, path: &Path) -> Result<(), PlayerError> {
        self.end_reverse();
        self.state.lock().opening(path);
        self.refresh();
        self.command(|p| p.set_loop(true));
        self.player.open(
            path,
            OpenOptions {
                start: None,
                play: Some(true),
            },
        )
    }

    /// Back from the probe render: `path` at `position`, paused or playing as before, no loop.
    pub fn return_to(&self, path: &Path, position: Timestamp, paused: bool) -> Result<(), PlayerError> {
        self.end_reverse();
        self.state.lock().opening(path);
        self.refresh();
        self.command(|p| p.set_loop(false));
        self.player.open(
            path,
            OpenOptions {
                start: Some(position.as_secs_f64()),
                play: Some(!paused),
            },
        )
    }

    /// Ends the loop of a probe render without opening anything (another file is about to open).
    pub fn stop_loop(&self) {
        self.command(|p| p.set_loop(false));
    }

    /// Whether the player is paused (or has nothing to play).
    pub fn is_paused(&self) -> bool {
        !self.state.lock().playing()
    }

    /// The preview's video filters (sent to the player's command thread).
    pub fn set_video_filter(&self, filter: VideoFilter) {
        tracing::debug!(lavfi = %filter.lavfi, rotates = filter.rotates_source, "[Player] Video filters");
        self.command(|p| p.set_video_filter(filter));
    }

    /// The file that is open (also when it failed to play).
    pub fn current_file(&self) -> Option<PathBuf> {
        self.state.lock().file.clone()
    }

    /// Volume and mute as mpv has them now (saved in the UI state at quit).
    pub fn volume_and_mute(&self) -> (u8, bool) {
        let state = self.state.lock();
        (state.volume, state.muted)
    }

    /// Loads the current file again at the current position, keeping the pause state (F5, the error overlay).
    fn reload(&self) {
        let (file, start) = {
            let state = self.state.lock();
            let start =
                (state.failure.is_none() && state.position.as_micros() > 0).then(|| state.position.as_secs_f64());
            (state.file.clone(), start)
        };
        let Some(file) = file else { return };
        self.end_reverse();
        tracing::info!(path = %file.display(), start = ?start, "[Player] Reloading");
        self.state.lock().opening(&file);
        self.refresh();
        if let Err(e) = self.player.open(&file, OpenOptions { start, play: None }) {
            tracing::error!(error = %e, "[Player] Reloading failed");
        }
    }

    fn connect(self: &Rc<Self>, window: &MainWindow) {
        let playback = window.global::<Playback<'_>>();
        let this = Rc::downgrade(self);
        playback.on_toggle_pause(with(&this, PlayerBridge::toggle_pause));
        let this = Rc::downgrade(self);
        playback.on_frame_step(move |back| {
            if let Some(b) = this.upgrade() {
                b.step_frame(back);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_toggle_mute(with(&this, |b| {
            let muted = b
                .window
                .upgrade()
                .is_some_and(|w| w.global::<Playback<'_>>().get_muted());
            b.command(|p| p.set_muted(!muted));
        }));
        let this = Rc::downgrade(self);
        playback.on_set_volume(move |percent| {
            if let Some(b) = this.upgrade() {
                b.set_volume(percent);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_wheel(move |delta| {
            if let Some(b) = this.upgrade() {
                b.wheel(delta);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_reload(with(&this, PlayerBridge::reload));
    }

    /// Mouse wheel over the video: one notch changes the volume by the configured step.
    fn wheel(&self, delta: f32) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        // A change of direction starts over.
        let rest = self.wheel_rest.get();
        let total = if rest * delta < 0.0 { delta } else { rest + delta };
        let notches = (total / WHEEL_NOTCH).trunc();
        self.wheel_rest.set(total - notches * WHEEL_NOTCH);
        let Some(notches) = notches.checked_as::<i32>().filter(|n| *n != 0) else {
            return;
        };
        let Some(window) = self.window.upgrade() else { return };
        let current = window.global::<Playback<'_>>().get_volume();
        self.set_volume(current.saturating_add(notches.saturating_mul(i32::from(self.wheel_step.get()))));
    }

    fn set_volume(&self, percent: i32) {
        let percent = percent.clamp(0, 150);
        // Shown at once; mpv's confirmation arrives as a property change.
        if let Some(window) = self.window.upgrade() {
            window.global::<Playback<'_>>().set_volume(percent);
        }
        self.command(|p| p.set_volume(f64::from(percent)));
    }

    fn command(&self, run: impl FnOnce(&Player) -> Result<(), PlayerError>) {
        if let Err(e) = run(&self.player) {
            tracing::warn!(error = %e, "[Player] Command not sent");
        }
    }

    /// Shows the current state at once (UI thread).
    fn refresh(&self) {
        let view = self.state.lock().view();
        if let Some(window) = self.window.upgrade() {
            show(&window, &view);
        }
    }
}

/// A callback without arguments that runs `f` while the bridge exists.
fn with(this: &Weak<PlayerBridge>, f: impl Fn(&PlayerBridge) + 'static) -> impl Fn() + 'static {
    let this = Weak::clone(this);
    move || {
        if let Some(bridge) = this.upgrade() {
            f(&bridge);
        }
    }
}

/// Puts `view` into the `Playback` global and the status line's decoder field (UI thread).
fn show(window: &MainWindow, view: &PlaybackView) {
    let playback = window.global::<Playback<'_>>();
    playback.set_has_file(view.has_file);
    playback.set_has_video(view.has_video);
    playback.set_paused(view.paused);
    playback.set_muted(view.muted);
    playback.set_volume(view.volume);
    playback.set_position_text(view.position_text.as_str().into());
    playback.set_duration_text(view.duration_text.as_str().into());
    playback.set_speed_text(view.speed_text.as_str().into());
    playback.set_error_title(view.error_title.as_str().into());
    playback.set_error_detail(view.error_detail.as_str().into());
    window
        .global::<Status<'_>>()
        .set_decoder(view.decoder_text.as_str().into());
    VIEW_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(view);
        }
    });
}

/// The video area's size in physical pixels.
fn video_size(window: &MainWindow) -> slint::PhysicalSize {
    let scale = window.window().scale_factor();
    let px = |logical: f32| (logical * scale).round().checked_as::<u32>().unwrap_or(0);
    slint::PhysicalSize::new(px(window.get_video_width()), px(window.get_video_height()))
}

/// Logs a player event (on mpv's event thread); property changes only at trace level (the position changes every frame),
/// the end of a file because the window closed (the video surface went away) at debug level.
fn log_event(event: &PlayerEvent) {
    match event {
        PlayerEvent::PlaybackFailed { reason, detail } => {
            tracing::warn!(reason = %reason, detail = ?detail, "[Player] The file cannot be played");
        }
        PlayerEvent::Error(message) => tracing::warn!(message = %message, "[Player] Playback error"),
        PlayerEvent::Ended(EndReason::SurfaceLost) => {
            tracing::debug!("[Player] Playback ended with the video surface (window closed)");
        }
        PlayerEvent::Ended(reason) => tracing::info!(reason = ?reason, "[Player] Playback ended"),
        PlayerEvent::Property(property) => tracing::trace!(property = ?property, "[Player] Property"),
        other => tracing::debug!(event = ?other, "[Player] Event"),
    }
}
