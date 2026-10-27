//! What happens when the queue is done and when RustCut is closed while it renders (P5 M5.1, user decisions Q-P1,
//! Q-P2): the queue panel's "when done" menu (nothing, notify, quit, suspend, power off; kept in `UiState.queue`), the
//! countdown message with Abort before quit, suspend or power off (`Kind::PowerCountdown`, M6.5), the close
//! question with three ways, and the desktop actor's sleep lock and taskbar entry following the queue. Pure parts in
//! `state.rs`.

mod job_messages;
mod question;
pub mod state;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use rustcut_core::queue::QueueCommand;
use rustcut_core::settings::{QueuePanelState, Settings, TaskbarShows, WhenDone};
use slint::ComponentHandle as _;

use self::state::{CHOICES, Countdown, Tick};
use crate::desktop::logind::PowerAction;
use crate::desktop::service::{DesktopCommand, DesktopHandle};
use crate::desktop::taskbar::Badge;
use crate::ui::menu::{ContextMenuBridge, MenuAction, MenuItem};
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::queue::{QueueBridge, QueueStatus};
use crate::{MainWindow, Power, Queue};

pub struct PowerBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    desktop: DesktopHandle,
    queue: Weak<QueueBridge>,
    menu: Weak<ContextMenuBridge>,
    when_done: Cell<WhenDone>,
    countdown: Cell<Option<Countdown>>,
    timer: slint::Timer,
    /// The close question while it is open (`question.rs`).
    question: question::Question,
    /// The user wanted to quit when the queue got done: quit after the "when done" action (suspend).
    quit_after: Cell<bool>,
    /// `queue.when_done_countdown_secs`, `general.confirm_quit_while_rendering`.
    seconds: Cell<u8>,
    confirm: Cell<bool>,
    /// `desktop.taskbar_shows`: what the taskbar entry's bar follows.
    taskbar_shows: Cell<TaskbarShows>,
    /// "3 jobs finished." while the countdown runs.
    done_text: RefCell<String>,
    /// What the desktop actor was told last.
    rendering: Cell<bool>,
    badge: Cell<Badge>,
    /// What a quit would lose (a temporary queue's unfinished jobs, M6.5d): the question's text, `None` = nothing.
    loss: RefCell<Option<LossCheck>>,
}

/// Tells what quitting now would lose.
pub type LossCheck = Box<dyn Fn() -> Option<String>>;

impl PowerBridge {
    /// Takes over the "when done" button, the countdown banner and the close question. The app owns the only strong
    /// reference. `when_done` is the choice the queue started with.
    pub fn install(
        window: &MainWindow,
        parts: (&tokio::runtime::Handle, &Rc<QueueBridge>, &Rc<ContextMenuBridge>),
        desktop: DesktopHandle,
        settings: &Settings,
        when_done: WhenDone,
    ) -> Rc<Self> {
        let (runtime, queue, menu) = parts;
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime: runtime.clone(),
            desktop,
            queue: Rc::downgrade(queue),
            menu: Rc::downgrade(menu),
            when_done: Cell::new(when_done),
            countdown: Cell::new(None),
            timer: slint::Timer::default(),
            question: question::Question::default(),
            quit_after: Cell::new(false),
            seconds: Cell::new(settings.queue.when_done_countdown_secs),
            confirm: Cell::new(settings.general.confirm_quit_while_rendering),
            taskbar_shows: Cell::new(settings.desktop.taskbar_shows),
            done_text: RefCell::default(),
            rendering: Cell::new(false),
            badge: Cell::new(Badge::NONE),
            loss: RefCell::new(None),
        });
        connect(&bridge, window);
        {
            let mut hooks = queue.hooks();
            let weak = Rc::downgrade(&bridge);
            hooks.status.push(Box::new(move |status| {
                if let Some(b) = weak.upgrade() {
                    b.follow(status);
                }
            }));
            let weak = Rc::downgrade(&bridge);
            hooks.job_done = Some(Box::new(move |row| {
                if let Some(b) = weak.upgrade() {
                    b.job_done(row);
                }
            }));
            let weak = Rc::downgrade(&bridge);
            hooks.done = Some(Box::new(move |finished, failed| {
                if let Some(b) = weak.upgrade() {
                    b.queue_done(finished, failed);
                }
            }));
        }
        bridge.publish();
        bridge
    }

    /// The settings changed (the Settings window).
    pub fn apply_settings(&self, settings: &Settings) {
        self.seconds.set(settings.queue.when_done_countdown_secs);
        self.confirm.set(settings.general.confirm_quit_while_rendering);
        self.taskbar_shows.set(settings.desktop.taskbar_shows);
        let _ = self.desktop.send(DesktopCommand::Settings {
            inhibit: settings.desktop.inhibit_sleep,
            taskbar: settings.desktop.taskbar_progress,
        });
    }

    /// The choice for `ui-state.toml` (the countdown's actions reset it, so a quit never repeats at the next start).
    pub fn save_into(&self, state: &mut QueuePanelState) {
        state.when_done = Some(self.when_done.get());
    }

    /// Escape: aborts a running countdown (`true` = it did).
    pub fn escape(&self) -> bool {
        if self.countdown.get().is_none() {
            return false;
        }
        self.abort();
        true
    }

    fn set_when_done(&self, choice: WhenDone) {
        self.when_done.set(choice);
        if let Some(queue) = self.queue.upgrade()
            && queue.handle().send(QueueCommand::SetWhenDone(choice)).is_err()
        {
            tracing::warn!("[Power] The queue has stopped; the choice holds for this run only");
        }
        tracing::info!(?choice, "[Power] When the queue is done");
        self.publish();
    }

    /// The "When the queue finishes…" menu, hanging from its button at `anchor` (window x, y, width, height).
    fn show_menu(&self, anchor: [f32; 4]) {
        let Some(menu) = self.menu.upgrade() else { return };
        let current = self.when_done.get();
        let mut items: Vec<MenuItem> = vec![
            MenuItem::Heading("When the queue finishes…".to_owned()),
            MenuItem::Separator,
        ];
        items.extend(CHOICES.iter().map(|choice| {
            let (icon, tint) = state::look(*choice);
            MenuAction::new(state::index(*choice).to_string(), state::label(*choice), Some(icon))
                .tint(tint)
                .checked(*choice == current)
                .into()
        }));
        let weak = Weak::clone(&self.me);
        menu.show_at(&items, anchor, move |id| {
            if let (Some(b), Some(choice)) = (weak.upgrade(), id.parse::<usize>().ok().and_then(|i| CHOICES.get(i))) {
                b.set_when_done(*choice);
            }
        });
    }

    /// The queue's state after a refresh: the sleep lock and the taskbar entry follow it.
    fn follow(&self, status: QueueStatus) {
        let rendering = status.rendering > 0;
        if rendering != self.rendering.replace(rendering) {
            let _ = self.desktop.send(DesktopCommand::Rendering(rendering));
        }
        let badge = state::badge(&status, self.taskbar_shows.get());
        if badge != self.badge.replace(badge) {
            let _ = self.desktop.send(DesktopCommand::Badge(badge));
        }
        self.question_follows(&status);
    }

    fn queue_done(&self, finished: u32, failed: u32) {
        let choice = self.when_done.get();
        let text = state::done_text(finished, failed);
        tracing::info!(?choice, finished, failed, "[Power] The queue is done");
        // The close question was waiting for this: the chosen action (quitting too) or a short countdown to the quit.
        let quit_after = self.question_on_done(choice);
        if matches!(choice, WhenDone::Nothing | WhenDone::Notify) {
            let mut notice = Notice::new(Kind::QueueDone, "The queue is done").detail(text);
            if failed > 0 {
                notice = notice.level(Level::Warning);
            }
            // "When done: notify" asked for the desktop notification.
            if choice == WhenDone::Notify {
                notice = notice.force_desktop();
            }
            notices::post(notice);
            return;
        }
        let Some(countdown) = Countdown::start(choice, u32::from(self.seconds.get())) else {
            return;
        };
        self.quit_after.set(quit_after);
        self.countdown.set(Some(countdown));
        self.done_text.replace(text);
        let weak = Weak::clone(&self.me);
        self.timer
            .start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
                if let Some(b) = weak.upgrade() {
                    b.tick();
                }
            });
        self.tick();
    }

    fn tick(&self) {
        let Some(mut countdown) = self.countdown.get() else {
            self.timer.stop();
            return;
        };
        match countdown.tick() {
            Tick::Wait => {
                self.countdown.set(Some(countdown));
                self.show_countdown(countdown);
            }
            Tick::Fire(action) => {
                self.timer.stop();
                self.countdown.set(None);
                notices::clear(COUNTDOWN);
                self.fire(action);
            }
        }
    }

    /// The countdown's message: its title stays, the seconds run in the detail; Abort and Escape stop it.
    fn show_countdown(&self, countdown: Countdown) {
        let weak = Weak::clone(&self.me);
        notices::post(
            Notice::new(Kind::PowerCountdown, countdown.verb())
                .detail(format!("in {} s · {}", countdown.left, self.done_text.borrow()))
                .live(COUNTDOWN)
                .action("Abort", move || {
                    if let Some(b) = weak.upgrade() {
                        b.abort();
                    }
                })
                .desktop_secs(u32::from(self.seconds.get()).max(5)),
        );
    }

    fn abort(&self) {
        self.timer.stop();
        if self.countdown.take().is_some() {
            tracing::info!("[Power] Countdown aborted");
            notices::clear(COUNTDOWN);
            Self::tell(Notice::new(Kind::PowerChoice, "Aborted: nothing happens."));
        }
    }

    /// Quit, suspend or power off now. The choice goes back to "nothing" first, so it never repeats by surprise.
    fn fire(&self, action: WhenDone) {
        self.set_when_done(WhenDone::Nothing);
        let power = match action {
            WhenDone::Quit => {
                self.quit_now();
                return;
            }
            WhenDone::Suspend => PowerAction::Suspend,
            WhenDone::PowerOff => PowerAction::PowerOff,
            WhenDone::Nothing | WhenDone::Notify => return,
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self
            .desktop
            .send(DesktopCommand::Power { action: power, reply })
            .is_err()
        {
            Self::tell(
                Notice::new(Kind::PowerRefused, "The computer was not suspended")
                    .detail("The desktop service has stopped."),
            );
            return;
        }
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let result = answer.await.unwrap_or_else(|_gone| Err("no answer".into()));
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(()) if power == PowerAction::PowerOff || b.quit_after.take() => b.quit_now(),
                Ok(()) => {}
                Err(reason) => Self::tell(Notice::new(Kind::PowerRefused, "The system refused").detail(reason)),
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Power] The event loop is gone");
        }
    }

    /// What a quit would lose: asked before every quit (the database bridge, while the queue is temporary).
    pub fn set_loss_check(&self, check: LossCheck) {
        *self.loss.borrow_mut() = Some(check);
    }

    fn quit_now(&self) {
        self.close_question();
        if let Some(window) = self.window.upgrade() {
            window.invoke_quit();
        }
    }

    fn tell(notice: Notice) {
        tracing::info!("[Power] {}", notice.text());
        notices::post(notice);
    }

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let q = window.global::<Queue<'_>>();
        let choice = self.when_done.get();
        q.set_when_done(i32::try_from(state::index(choice)).unwrap_or(0));
        q.set_when_done_tip(
            match choice {
                WhenDone::Nothing => "When the queue finishes: Do Nothing".to_owned(),
                other => format!(
                    "When the queue finishes: {} (a right click resets it to Do Nothing)",
                    state::label(other)
                ),
            }
            .into(),
        );
    }
}

/// The key of the countdown's live message.
const COUNTDOWN: &str = "power_countdown";

thread_local! {
    /// The bridge, for a failed job's "Show log".
    static POWER: RefCell<Weak<PowerBridge>> = const { RefCell::new(Weak::new()) };
}

fn connect(bridge: &Rc<PowerBridge>, window: &MainWindow) {
    POWER.with(|p| *p.borrow_mut() = Rc::downgrade(bridge));
    let on = |f: fn(&PowerBridge)| {
        let weak = Rc::downgrade(bridge);
        move || {
            if let Some(b) = weak.upgrade() {
                f(&b);
            }
        }
    };
    window.on_request_quit(on(PowerBridge::request_quit));
    let power = window.global::<Power<'_>>();
    power.on_keep(on(PowerBridge::close_question));
    power.on_quit_now(on(PowerBridge::quit_now));
    power.on_quit_when_done(on(|b| {
        b.close_question();
        b.set_when_done(WhenDone::Quit);
        PowerBridge::tell(Notice::new(Kind::PowerChoice, "RustCut quits when the queue is done."));
    }));
    let weak = Rc::downgrade(bridge);
    window
        .global::<Queue<'_>>()
        .on_when_done_menu(move |x, y, width, height| {
            if let Some(b) = weak.upgrade() {
                b.show_menu([x, y, width, height]);
            }
        });
    let weak = Rc::downgrade(bridge);
    window.global::<Queue<'_>>().on_when_done_reset(move || {
        if let Some(b) = weak.upgrade()
            && b.when_done.get() != WhenDone::Nothing
        {
            b.set_when_done(WhenDone::Nothing);
        }
    });
}
