//! The optional tray icon (P5 M5.2, ADR-15; `desktop.tray.*`): Slint's `SystemTrayIcon` (`ui/tray.slint`) with the
//! queue in its tooltip, a click that shows or hides the main window, and a menu to pause or resume the queue and to quit.
//! With `desktop.tray.minimize_to_tray`, minimizing the window while a job renders hides it in the tray. Hiding takes
//! the video output away from the player, so the open file is opened again at the same place when the window comes back.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use rustcut_core::settings::TraySettings;
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;
use slint::winit_030::WinitWindowAccessor as _;

use crate::ui::player::PlayerBridge;
use crate::ui::queue::{QueueBridge, QueueStatus};
use crate::{AppTray, MainWindow, Queue};

/// How often a minimized window is looked for while a job renders (minimize to the tray).
const MINIMIZE_POLL: Duration = Duration::from_millis(500);

pub struct TrayBridge {
    me: Weak<Self>,
    main: slint::Weak<MainWindow>,
    player: Weak<PlayerBridge>,
    tray: RefCell<Option<AppTray>>,
    settings: RefCell<TraySettings>,
    status: Cell<Option<QueueStatus>>,
    /// The main window is hidden in the tray.
    hidden: Cell<bool>,
    /// The file that played when the window was hidden: opened again at this place when it comes back.
    resume: RefCell<Option<(PathBuf, Timestamp, bool)>>,
    poll: slint::Timer,
}

impl TrayBridge {
    /// Shows the tray icon when `desktop.tray.enabled` is on. The app owns the only strong reference.
    pub fn install(
        main: &MainWindow,
        queue: &Rc<QueueBridge>,
        player: &Rc<PlayerBridge>,
        settings: &TraySettings,
    ) -> Rc<Self> {
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            main: main.as_weak(),
            player: Rc::downgrade(player),
            tray: RefCell::new(None),
            settings: RefCell::new(settings.clone()),
            status: Cell::new(None),
            hidden: Cell::new(false),
            resume: RefCell::new(None),
            poll: slint::Timer::default(),
        });
        let weak = Rc::downgrade(&bridge);
        queue.hooks().status.push(Box::new(move |status| {
            if let Some(b) = weak.upgrade() {
                b.follow(status);
            }
        }));
        bridge.apply();
        bridge
    }

    /// `desktop.tray` changed in the Settings window.
    pub fn apply_settings(&self, settings: &TraySettings) {
        self.settings.replace(settings.clone());
        self.apply();
    }

    /// Shows the tray icon or removes it (a hidden window comes back first), and starts or stops watching for a
    /// minimized window.
    fn apply(&self) {
        let enabled = self.settings.borrow().enabled;
        if enabled && self.tray.borrow().is_none() {
            match AppTray::new() {
                Ok(tray) => {
                    self.connect(&tray);
                    if let Err(e) = tray.show() {
                        tracing::warn!(error = %e, "[Tray] The tray icon could not be shown");
                    }
                    tracing::info!("[Tray] Tray icon shown");
                    *self.tray.borrow_mut() = Some(tray);
                }
                Err(e) => tracing::warn!(error = %e, "[Tray] The tray icon could not be made"),
            }
        } else if !enabled && let Some(tray) = self.tray.borrow_mut().take() {
            if self.hidden.get() {
                self.show_window();
            }
            let _ = tray.hide();
            tracing::info!("[Tray] Tray icon removed");
        }
        self.publish();
        self.watch_minimize();
    }

    fn connect(&self, tray: &AppTray) {
        let on = |f: fn(&Self)| {
            let weak = Weak::clone(&self.me);
            move || {
                if let Some(b) = weak.upgrade() {
                    f(&b);
                }
            }
        };
        tray.on_toggle_window(on(Self::toggle_window));
        tray.on_toggle_paused(on(|b| {
            if let Some(main) = b.main.upgrade() {
                main.global::<Queue<'_>>().invoke_toggle_paused();
            }
        }));
        tray.on_quit_app(on(|b| {
            // The window comes back first: the close question needs it, and the geometry is saved while it shows.
            b.show_window();
            if let Some(main) = b.main.upgrade() {
                main.invoke_request_quit();
            }
        }));
    }

    fn follow(&self, status: QueueStatus) {
        self.status.set(Some(status));
        self.publish();
        self.watch_minimize();
    }

    /// A click on the tray icon: a hidden or minimized window comes back to the front, a shown one goes to the tray.
    fn toggle_window(&self) {
        let minimized = self.main.upgrade().is_some_and(|m| is_minimized(&m));
        if self.hidden.get() || minimized {
            self.show_window();
        } else {
            self.hide_window();
        }
    }

    fn hide_window(&self) {
        let Some(main) = self.main.upgrade() else { return };
        *self.resume.borrow_mut() = self.player.upgrade().and_then(|p| p.resume_point());
        if let Err(e) = main.hide() {
            tracing::warn!(error = %e, "[Tray] The window could not be hidden");
            return;
        }
        self.hidden.set(true);
        tracing::info!("[Tray] Window hidden in the tray");
        self.publish();
    }

    fn show_window(&self) {
        let Some(main) = self.main.upgrade() else { return };
        if let Err(e) = main.show() {
            tracing::warn!(error = %e, "[Tray] The window could not be shown");
            return;
        }
        main.window().set_minimized(false);
        main.window()
            .with_winit_window(slint::winit_030::winit::window::Window::focus_window);
        self.hidden.set(false);
        if let Some((path, position, paused)) = self.resume.borrow_mut().take()
            && let Some(player) = self.player.upgrade()
            && let Err(e) = player.return_to(&path, position, paused)
        {
            tracing::warn!(error = %e, "[Tray] The video could not be opened again");
        }
        tracing::info!("[Tray] Window shown again");
        self.publish();
    }

    /// Minimize to the tray: while a job renders, a minimized window goes to the tray.
    fn watch_minimize(&self) {
        let wanted = {
            let settings = self.settings.borrow();
            settings.enabled
                && settings.minimize_to_tray
                && !self.hidden.get()
                && self.status.get().is_some_and(|s| s.rendering > 0)
        };
        if !wanted {
            self.poll.stop();
            return;
        }
        if self.poll.running() {
            return;
        }
        let weak = Weak::clone(&self.me);
        self.poll.start(slint::TimerMode::Repeated, MINIMIZE_POLL, move || {
            if let Some(b) = weak.upgrade()
                && let Some(main) = b.main.upgrade()
                && is_minimized(&main)
            {
                b.hide_window();
                b.poll.stop();
            }
        });
    }

    fn publish(&self) {
        let tray = self.tray.borrow();
        let Some(tray) = tray.as_ref() else { return };
        tray.set_window_shown(!self.hidden.get());
        let status = self.status.get();
        tray.set_paused(status.is_some_and(|s| s.processing_paused));
        tray.set_status(tooltip(status.as_ref()).into());
    }
}

/// Whether the window manager shows the window minimized. Slint's own flag only knows its own `set_minimized`; winit
/// reads the window's state from the system (X11: `_NET_WM_STATE_HIDDEN`).
fn is_minimized(main: &MainWindow) -> bool {
    main.window()
        .with_winit_window(slint::winit_030::winit::window::Window::is_minimized)
        .flatten()
        .unwrap_or(false)
}

/// "RustCut · 1 rendering, 2 waiting · 42 %", or "RustCut · the queue is idle".
pub fn tooltip(status: Option<&QueueStatus>) -> String {
    let Some(s) = status.filter(|s| s.open > 0) else {
        return "RustCut · the queue is idle".into();
    };
    let waiting = s.open.saturating_sub(s.active);
    let mut text = format!("RustCut · {} rendering, {waiting} waiting", s.rendering);
    if s.processing_paused {
        text.push_str(" · paused");
    }
    if let Some(o) = s.overall {
        text = format!("{text} · {} %", (o.fraction * 100.0).round());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::queue::Overall;

    #[test]
    fn the_tooltip_tells_how_busy_the_queue_is() {
        assert_eq!(tooltip(None), "RustCut · the queue is idle");
        let busy = QueueStatus {
            rendering: 1,
            active: 1,
            processing_paused: false,
            open: 3,
            overall: Some(Overall {
                fraction: 0.416,
                done: 1,
                total: 4,
                eta_secs: None,
                partial: false,
            }),
        };
        assert_eq!(tooltip(Some(&busy)), "RustCut · 1 rendering, 2 waiting · 42 %");
        let paused = QueueStatus {
            rendering: 0,
            processing_paused: true,
            overall: None,
            ..busy
        };
        assert_eq!(tooltip(Some(&paused)), "RustCut · 0 rendering, 2 waiting · paused");
    }
}
