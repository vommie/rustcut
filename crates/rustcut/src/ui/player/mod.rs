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
    AudioOptions, EndReason, Normalization, OpenOptions, Player, PlayerError, PlayerEvent, PlayerOptions, SurfaceError,
    SurfaceHooks, VideoFilter, attach_surface,
};
use slint::ComponentHandle as _;

pub use shuttle::ShuttleKey;
pub use state::{Motion, PlaybackState, PlaybackView};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::ui::notices::{self, Kind, Notice};
use crate::{MainWindow, Playback, Status};

/// The one coalesced update of the playback view.
const VIEW_KEY: CoalesceKey = CoalesceKey::new("playback", 0);
/// One mouse wheel notch in Slint's `delta-y` on X11 (measured in RustView: winit reports two lines of 60 px); touchpads
/// send pixels that add up to a notch the same way.
const WHEEL_NOTCH: f32 = 120.0;
/// The volume bar's end in whole percent (`rustcut_player::MAX_VOLUME`).
const MAX_PERCENT: i32 = 100;

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
    /// "Flash a play or pause symbol" (P9.9).
    flash: Cell<bool>,
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
            audio: audio_options(settings),
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
            flash: Cell::new(settings.flash_play_pause),
            wheel_step: Cell::new(settings.wheel_volume_step),
            wheel_rest: Cell::new(0.0),
            reverse: Cell::new(None),
            reverse_timer: slint::Timer::default(),
        });
        bridge.connect(window);
        show_symbols(&window.global::<Playback<'_>>(), settings);
        bridge.refresh();
        Ok(bridge)
    }

    /// The player settings changed in the Settings window: auto play and the wheel step hold from now on; the preview
    /// audio filters, the hardware decoding and the background change at once.
    pub fn apply_settings(&self, before: &PlayerSettings, settings: &PlayerSettings) {
        self.auto_play.set(settings.auto_play);
        self.flash.set(settings.flash_play_pause);
        if let Some(window) = self.window.upgrade() {
            show_symbols(&window.global::<Playback<'_>>(), settings);
        }
        self.wheel_step.set(settings.wheel_volume_step);
        if audio_options(before) != audio_options(settings)
            && let Err(e) = self.player.set_audio(audio_options(settings))
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

    /// Opens `path` at `position`, paused or playing (`None`: as the auto play setting says).
    pub fn open_at(&self, path: &Path, position: Timestamp, paused: Option<bool>) -> Result<(), PlayerError> {
        self.end_reverse();
        self.state.lock().opening(path);
        self.refresh();
        self.player.open(
            path,
            OpenOptions {
                start: Some(position.as_secs_f64()),
                play: Some(paused.map_or_else(|| self.auto_play.get(), |paused| !paused)),
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

    /// Pauses or plays (a dialog that needs the user's eyes pauses a playing video and plays it again afterwards).
    pub fn set_paused(&self, paused: bool) {
        self.state.lock().user_playback();
        self.command(|p| p.set_paused(paused));
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

    /// The open file with its playhead and whether it is paused (to open it again at the same place).
    pub fn resume_point(&self) -> Option<(PathBuf, Timestamp, bool)> {
        let state = self.state.lock();
        let file = state.file.clone()?;
        Some((file, state.seek_target.unwrap_or(state.position), !state.playing()))
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
        let this = Rc::downgrade(self);
        playback.on_speed_reset(with(&this, PlayerBridge::normal_speed));
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
        let playback = window.global::<Playback<'_>>();
        let current = playback.get_volume();
        self.set_volume(current.saturating_add(notches.saturating_mul(i32::from(self.wheel_step.get()))));
        // Only the wheel shows the volume bar over the video (the slider shows the level itself; RustView's rule).
        if playback.get_has_video() && playback.get_show_volume_bar() {
            playback.set_volume_flash_serial(playback.get_volume_flash_serial().wrapping_add(1));
        }
    }

    fn set_volume(&self, percent: i32) {
        let percent = percent.clamp(0, MAX_PERCENT);
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

/// What the symbol in the middle of the video shows after a key (`Playback.flash-kind`; one look for all of them, user
/// request 2026-10-27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashKind {
    /// Play or pause (`Playback.flash-play`).
    PlayPause,
    SoundOn,
    SoundOff,
    /// A rating from 0 to 5 (Ctrl+0..5).
    Rating(u8),
    /// The normalization's keys (`Playback.flash-text`).
    Normalization,
}

/// Which symbols over the video the settings show (each on its own, user request 2026-10-27).
fn show_symbols(playback: &Playback<'_>, settings: &PlayerSettings) {
    playback.set_keep_pause_symbol(settings.keep_pause_symbol);
    playback.set_show_mute_symbol(settings.flash_mute);
    playback.set_show_rating_symbol(settings.flash_rating);
    playback.set_show_volume_bar(settings.volume_bar);
    playback.set_normalized(settings.normalization != rustcut_core::settings::Normalization::None);
    playback.set_normalization_name(settings.normalization.title().into());
    playback.set_normalization_back(settings.return_to().title().into());
    playback.set_hide_pointer(settings.hide_pointer);
    playback.set_hide_pointer_ms(i32::try_from(settings.hide_pointer_ms).unwrap_or(i32::MAX));
    playback.set_hide_pointer_after(crate::ui::control_bar::seconds_text(settings.hide_pointer_ms).into());
}

/// The preview's audio filters from the settings.
fn audio_options(settings: &PlayerSettings) -> AudioOptions {
    AudioOptions {
        mute_last_second: settings.mute_last_second,
        normalization: match settings.normalization {
            rustcut_core::settings::Normalization::None => Normalization::None,
            rustcut_core::settings::Normalization::Loudness => Normalization::Loudness,
            rustcut_core::settings::Normalization::Dynamic => Normalization::Dynamic,
            rustcut_core::settings::Normalization::Speech => Normalization::Speech,
            rustcut_core::settings::Normalization::Compressor => Normalization::Compressor,
        },
    }
}

/// Shows `kind` in the middle of the video (a new one replaces the one that shows); nothing without a video or while
/// its setting is off (play/pause is checked by `key_flash`).
pub fn flash(playback: &Playback<'_>, kind: FlashKind) {
    let wanted = match kind {
        // The caller asked their settings.
        FlashKind::PlayPause | FlashKind::Normalization => true,
        FlashKind::SoundOn | FlashKind::SoundOff => playback.get_show_mute_symbol(),
        FlashKind::Rating(_) => playback.get_show_rating_symbol(),
    };
    if !wanted || !playback.get_has_video() {
        return;
    }
    let (code, rating) = match kind {
        FlashKind::PlayPause => (0, 0),
        FlashKind::SoundOn => (1, 0),
        FlashKind::SoundOff => (2, 0),
        FlashKind::Rating(r) => (3, i32::from(r)),
        FlashKind::Normalization => (4, 0),
    };
    playback.set_flash_kind(code);
    playback.set_flash_rating(rating);
    playback.set_flash_serial(playback.get_flash_serial().wrapping_add(1));
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
    playback.set_pause_symbol(view.pause_symbol);
    playback.set_muted(view.muted);
    playback.set_volume(view.volume);
    playback.set_position_text(view.position_text.as_str().into());
    playback.set_duration_text(view.duration_text.as_str().into());
    playback.set_speed_text(view.speed_text.as_str().into());
    playback.set_error_title(view.error_title.as_str().into());
    playback.set_error_detail(view.error_detail.as_str().into());
    show_failure(window, view);
    window
        .global::<Status<'_>>()
        .set_decoder(view.decoder_text.as_str().into());
    VIEW_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(view);
        }
    });
}

thread_local! {
    /// The failure the banner shows ("" = none), so it is posted once per failure.
    static SHOWN_FAILURE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// A file that does not play: a banner with Reload and the way to the log (Q-N8 #13); it goes when a file plays.
fn show_failure(window: &MainWindow, view: &PlaybackView) {
    let key = format!("{}\n{}", view.error_title, view.error_detail);
    if SHOWN_FAILURE.with(|s| s.replace(key.clone())) == key {
        return;
    }
    if view.error_title.is_empty() {
        notices::clear_kind(Kind::PlaybackFailed);
        return;
    }
    let weak = window.as_weak();
    notices::post(
        Notice::new(Kind::PlaybackFailed, view.error_title.as_str())
            .detail(view.error_detail.as_str())
            .action("Reload", move || {
                if let Some(window) = weak.upgrade() {
                    window.global::<Playback<'_>>().invoke_reload();
                }
            })
            .leading_action("Show log", notices::show_app_log),
    );
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
