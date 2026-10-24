//! The one way RustCut quits. Closing the window, File > Quit and SIGTERM/SIGINT/SIGHUP all capture the window geometry
//! while the window is still shown and end the event loop; `window.run()` then returns and the app saves the UI state and
//! stops the player (`app::run`). A second signal while that runs ends the process at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rustcut_core::settings::WindowState;
use slint::ComponentHandle as _;
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::window;
use crate::MainWindow;
use crate::ui::dispatcher::UiDispatcher;

/// Captures the window geometry on the way out (UI thread).
pub struct QuitPath {
    geometry: RefCell<WindowState>,
    captured: Cell<bool>,
}

impl QuitPath {
    /// Handles the window's close request and the `quit` callback; `saved` is the geometry restored at startup.
    pub fn install(main: &MainWindow, saved: &WindowState) -> Rc<Self> {
        let path = Rc::new(Self {
            geometry: RefCell::new(saved.clone()),
            captured: Cell::new(false),
        });
        let (this, weak) = (Rc::downgrade(&path), main.as_weak());
        main.window().on_close_requested(move || {
            if let (Some(path), Some(main)) = (this.upgrade(), weak.upgrade()) {
                tracing::info!("[App] Window closed; quitting");
                path.capture(&main);
            }
            // The Settings window may still be open: closing the main window quits all the same.
            if let Err(e) = slint::quit_event_loop() {
                tracing::warn!(error = %e, "[App] The event loop could not be ended");
            }
            slint::CloseRequestResponse::HideWindow
        });
        let (this, weak) = (Rc::downgrade(&path), main.as_weak());
        main.on_quit(move || {
            if let (Some(path), Some(main)) = (this.upgrade(), weak.upgrade()) {
                path.capture(&main);
            }
            if let Err(e) = slint::quit_event_loop() {
                tracing::warn!(error = %e, "[App] The event loop could not be ended");
            }
        });
        path
    }

    fn capture(&self, main: &MainWindow) {
        window::capture(main, &mut self.geometry.borrow_mut());
        self.captured.set(true);
    }

    /// The geometry to save; captured now if no quit route did it while the window was shown.
    pub fn geometry(&self, main: &MainWindow) -> WindowState {
        if !self.captured.get() {
            tracing::debug!("[App] Window geometry captured after the event loop");
            self.capture(main);
        }
        self.geometry.borrow().clone()
    }
}

/// Quits like File > Quit on the first SIGTERM, SIGINT or SIGHUP; exits at once on a second one.
pub fn watch_signals(runtime: &tokio::runtime::Handle, ui: UiDispatcher, main: slint::Weak<MainWindow>) {
    runtime.spawn(async move {
        let mut signals = match Signals::new() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "[App] Signal handlers could not be installed");
                return;
            }
        };
        let (name, _) = signals.next().await;
        tracing::info!(signal = name, "[App] Signal received; quitting");
        ui.post(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_quit();
            }
        });
        let (name, number) = signals.next().await;
        tracing::warn!(signal = name, "[App] Second signal; exiting at once without saving");
        exit_now(128 + number);
    });
}

#[expect(
    clippy::exit,
    reason = "a second signal means the user wants the process gone even if saving hangs"
)]
fn exit_now(code: i32) -> ! {
    std::process::exit(code)
}

struct Signals {
    term: Signal,
    int: Signal,
    hup: Signal,
}

impl Signals {
    fn new() -> std::io::Result<Self> {
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
            hup: signal(SignalKind::hangup())?,
        })
    }

    /// The next signal's name and number.
    async fn next(&mut self) -> (&'static str, i32) {
        tokio::select! {
            _ = self.term.recv() => ("SIGTERM", 15),
            _ = self.int.recv() => ("SIGINT", 2),
            _ = self.hup.recv() => ("SIGHUP", 1),
        }
    }
}
