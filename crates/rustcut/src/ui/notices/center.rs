//! The notice center (UI thread): routes every message to its place, keeps the toasts, banners and the history,
//! publishes them to the `Notices` global, runs the buttons, sends the desktop notifications and answers the toasts'
//! context menu.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use rustcut_core::settings::{BannerAttention, BellBadge, NoticePlace, NoticeSettings};
use slint::winit_030::WinitWindowAccessor as _;
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::banners::{BannerBoard, NewBanner, Set};
use super::history::History;
use super::kind::{Kind, Level};
use super::notice::{Action, Click, Notice};
use super::route::{can_mute, desktop_now, route};
use super::toasts::{Added, NewToast, ToastStack};
use super::view;
use crate::app::SettingsWriter;
use crate::desktop::notify::{self, Note, Style};
use crate::icons::Icon;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::menu::{ContextMenuBridge, MenuAction, MenuItem};
use crate::ui::status;
use crate::{HistoryView, MainWindow, Notices, ToastView};

/// How often the toasts' times are checked while any shows.
const TICK: Duration = Duration::from_millis(100);
/// The bell lists this many.
const RECENT: usize = 3;
/// The summary card's id (the Slint side sends -1; real cards count from 1).
const SUMMARY: u64 = 0;
/// Desktop buttons kept for a click (the oldest are dropped).
const DESKTOP_RUNS: usize = 64;

/// What a toast's or banner's buttons and the click on it run.
#[derive(Clone, Default)]
struct Runs {
    actions: Vec<Action>,
    click: Option<Click>,
}

impl Runs {
    fn of(notice: &Notice) -> Self {
        Self {
            actions: notice.actions.clone(),
            click: notice.click.clone(),
        }
    }
}

/// The other bridges the center leads to.
#[derive(Default)]
pub struct Hooks {
    /// The bell's "Show all": the Logs panel with its "Messages" source.
    pub show_all: Option<Box<dyn Fn()>>,
    /// "Show log" of a message: the Logs panel with RustCut's own log (the details are there).
    pub show_log: Option<Box<dyn Fn()>>,
}

pub struct NoticeCenter {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    ui: UiDispatcher,
    writer: SettingsWriter,
    menu: Weak<ContextMenuBridge>,
    settings: RefCell<NoticeSettings>,
    toasts: RefCell<ToastStack>,
    banners: RefCell<BannerBoard>,
    history: RefCell<History>,
    toast_runs: RefCell<HashMap<u64, Runs>>,
    banner_runs: RefCell<HashMap<String, Runs>>,
    /// The title of keyed messages that show nowhere in the window (the history notes only a new title).
    quiet_keys: RefCell<HashMap<String, String>>,
    desktop_runs: RefCell<HashMap<u64, Rc<dyn Fn()>>>,
    next_desktop: Cell<u64>,
    toast_model: Rc<VecModel<ToastView>>,
    recent_model: Rc<VecModel<HistoryView>>,
    timer: slint::Timer,
    pulse: Cell<i32>,
    hooks: RefCell<Hooks>,
}

/// What the center needs from the app.
pub struct Parts<'a> {
    pub ui: &'a UiDispatcher,
    pub runtime: tokio::runtime::Handle,
    pub writer: SettingsWriter,
    pub menu: &'a Rc<ContextMenuBridge>,
    pub settings: &'a NoticeSettings,
}

impl NoticeCenter {
    pub fn new(window: &MainWindow, parts: Parts<'_>) -> Rc<Self> {
        let center = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime: parts.runtime,
            ui: parts.ui.clone(),
            writer: parts.writer,
            menu: Rc::downgrade(parts.menu),
            settings: RefCell::new(parts.settings.clone()),
            toasts: RefCell::default(),
            banners: RefCell::default(),
            history: RefCell::default(),
            toast_runs: RefCell::default(),
            banner_runs: RefCell::default(),
            quiet_keys: RefCell::default(),
            desktop_runs: RefCell::default(),
            next_desktop: Cell::new(0),
            toast_model: Rc::new(VecModel::default()),
            recent_model: Rc::new(VecModel::default()),
            timer: slint::Timer::default(),
            pulse: Cell::new(0),
            hooks: RefCell::default(),
        });
        let notices = window.global::<Notices<'_>>();
        notices.set_toasts(ModelRc::from(Rc::clone(&center.toast_model)));
        notices.set_recent(ModelRc::from(Rc::clone(&center.recent_model)));
        connect(&center, &notices);
        center.publish_all();
        center
    }

    pub fn hooks(&self) -> std::cell::RefMut<'_, Hooks> {
        self.hooks.borrow_mut()
    }

    /// The Settings window changed `[notices]`.
    pub fn apply_settings(&self, settings: &NoticeSettings) {
        if *self.settings.borrow() == *settings {
            return;
        }
        self.settings.replace(settings.clone());
        self.publish_all();
    }

    pub fn history(&self) -> std::cell::Ref<'_, History> {
        self.history.borrow()
    }

    pub fn post(&self, notice: Notice) {
        let route = route(notice.kind, notice.level, &self.settings.borrow());
        tracing::debug!(kind = notice.kind.id(), place = ?route.place, muted = route.muted, "[Notice] {}", notice.text());
        let clock = chrono::Local::now();
        let fresh = match route.place {
            NoticePlace::Toast => self.toast(&notice, clock.format("%H:%M").to_string()),
            NoticePlace::Banner => self.banner(&notice, clock.format("%H:%M").to_string()),
            NoticePlace::Status => {
                if let Some(window) = self.window.upgrade() {
                    status::show(&window, notice.level, &notice.text());
                }
                self.quiet_fresh(&notice)
            }
            NoticePlace::Bubble | NoticePlace::Dialog | NoticePlace::Nothing => self.quiet_fresh(&notice),
        };
        if !fresh {
            return;
        }
        self.history.borrow_mut().push(
            clock.format("%H:%M:%S").to_string(),
            notice.kind,
            notice.level,
            notice.title.clone(),
            notice.detail.clone(),
            route.place == NoticePlace::Nothing,
        );
        self.publish_bell();
        if notice.force_desktop || desktop_now(route, self.in_front()) {
            self.send_desktop(&notice);
        }
    }

    /// The state with `key` is over: its toast fades, its banner goes.
    pub fn clear(&self, key: &str) {
        let now = Instant::now();
        let toast = self.toasts.borrow_mut().close_key(key, now).is_some();
        let banner = self.banners.borrow_mut().clear(key);
        self.banner_runs.borrow_mut().remove(key);
        self.quiet_keys.borrow_mut().remove(key);
        if toast {
            self.publish_toasts();
        }
        if banner {
            self.publish_banner();
        }
    }

    /// A toast: new, a repeat, or the keyed one changed; `true` when it is news for the history.
    fn toast(&self, notice: &Notice, time: String) -> bool {
        let now = Instant::now();
        let old_title = notice.key.as_ref().and_then(|key| {
            self.toasts
                .borrow()
                .toasts()
                .iter()
                .find(|t| t.leaving.is_none() && t.key.as_ref() == Some(key))
                .map(|t| t.title.clone())
        });
        let shown_for = if notice.stays {
            None
        } else {
            self.toast_time(notice.level)
        };
        let added = self.toasts.borrow_mut().add(
            NewToast {
                kind: notice.kind,
                level: notice.level,
                title: notice.title.clone(),
                detail: notice.detail.clone(),
                key: notice.key.clone(),
                progress: notice.progress,
                actions: notice.actions.iter().map(|a| a.label.clone()).collect(),
                clickable: notice.click.is_some(),
                time,
                shown_for,
            },
            now,
        );
        self.toast_runs.borrow_mut().insert(added.id(), Runs::of(notice));
        self.publish_toasts();
        self.start_timer();
        match added {
            Added::New(_) | Added::Repeated(_) => true,
            Added::Updated(_) => old_title.as_deref() != Some(notice.title.as_str()),
        }
    }

    fn toast_time(&self, level: Level) -> Option<Duration> {
        let settings = self.settings.borrow();
        let t = &settings.toasts;
        let secs = match level {
            Level::Info => t.info_secs,
            Level::Success => t.success_secs,
            Level::Warning => t.warning_secs,
            Level::Error => t.error_secs,
        };
        (secs > 0).then(|| Duration::from_secs(u64::from(secs)))
    }

    fn banner(&self, notice: &Notice, since: String) -> bool {
        let key = notice.key.clone().unwrap_or_else(|| notice.kind.id().to_owned());
        let set = self.banners.borrow_mut().set(NewBanner {
            key: key.clone(),
            kind: notice.kind,
            level: notice.level,
            title: notice.title.clone(),
            detail: notice.detail.clone(),
            actions: notice.actions.iter().map(|a| a.label.clone()).collect(),
            clickable: notice.click.is_some(),
            progress: notice.progress,
            since,
        });
        self.banner_runs.borrow_mut().insert(key, Runs::of(notice));
        if set != Set::Same
            && notice.level == Level::Error
            && self.settings.borrow().banners.new_error == BannerAttention::Pulse
        {
            self.pulse.set(self.pulse.get().wrapping_add(1));
        }
        self.publish_banner();
        set != Set::Same
    }

    /// A message without a toast or banner: news unless its key showed the same title before.
    fn quiet_fresh(&self, notice: &Notice) -> bool {
        let Some(key) = &notice.key else { return true };
        self.quiet_keys
            .borrow_mut()
            .insert(key.clone(), notice.title.clone())
            .as_deref()
            != Some(&notice.title)
    }

    fn in_front(&self) -> bool {
        self.window.upgrade().is_none_or(|main| {
            let window = main.window();
            window.is_visible()
                && window
                    .with_winit_window(|w| w.has_focus() && w.is_minimized() != Some(true))
                    .unwrap_or(true)
        })
    }

    fn send_desktop(&self, notice: &Notice) {
        let settings = self.settings.borrow();
        let style = match (notice.kind, notice.level) {
            (Kind::PowerCountdown, _) => Style::Power,
            (_, Level::Error) => Style::Failed,
            _ => Style::Done,
        };
        let first = notice.actions.first();
        let on_action = first.map(|action| -> Box<dyn FnOnce() + Send> {
            let id = self.next_desktop.get().wrapping_add(1);
            self.next_desktop.set(id);
            let mut runs = self.desktop_runs.borrow_mut();
            runs.insert(id, Rc::clone(&action.run));
            if runs.len() > DESKTOP_RUNS
                && let Some(oldest) = runs.keys().min().copied()
            {
                runs.remove(&oldest);
            }
            let ui = self.ui.clone();
            Box::new(move || ui.post(move || super::run_desktop(id)))
        });
        notify::send(
            &self.runtime,
            Note {
                style,
                title: notice.title.clone(),
                body: notice.detail.clone(),
                timeout_secs: notice
                    .desktop_secs
                    .unwrap_or_else(|| u32::from(settings.desktop.timeout_secs)),
                action: first.map(|a| a.label.clone()),
            },
            on_action,
        );
    }

    pub(super) fn run_desktop(&self, id: u64) {
        let run = self.desktop_runs.borrow_mut().remove(&id);
        if let Some(run) = run {
            run();
        }
    }

    fn start_timer(&self) {
        if self.timer.running() {
            return;
        }
        let me = Weak::clone(&self.me);
        self.timer.start(slint::TimerMode::Repeated, TICK, move || {
            if let Some(center) = me.upgrade() {
                center.tick();
            }
        });
    }

    fn tick(&self) {
        let changed = self.toasts.borrow_mut().tick(Instant::now());
        if changed {
            let mut runs = self.toast_runs.borrow_mut();
            let toasts = self.toasts.borrow();
            runs.retain(|id, _| toasts.get(*id).is_some());
            drop((runs, toasts));
            self.publish_toasts();
        }
        if self.toasts.borrow().is_empty() {
            self.timer.stop();
        }
    }

    // ---- what the user does on the cards

    fn close_toast(&self, id: u64) {
        let now = Instant::now();
        if id == SUMMARY {
            self.toasts.borrow_mut().close_all(now);
            self.publish_toasts();
        } else if self.toasts.borrow_mut().close(id, now) {
            self.publish_toasts();
        }
    }

    fn toast_clicked(&self, id: u64) {
        if id == SUMMARY {
            self.toasts.borrow_mut().set_expanded(true);
            self.publish_toasts();
            return;
        }
        let click = self.toast_runs.borrow().get(&id).and_then(|r| r.click.clone());
        self.close_toast(id);
        run_click(click);
    }

    fn toast_action(&self, id: u64, index: usize) {
        let action = self
            .toast_runs
            .borrow()
            .get(&id)
            .and_then(|r| r.actions.get(index).cloned());
        let Some(action) = action else { return };
        if action.closes {
            self.close_toast(id);
        }
        (action.run)();
    }

    fn toast_menu(&self, id: u64, x: f32, y: f32) {
        let Some(toast) = self.toasts.borrow().get(id).cloned() else {
            return;
        };
        let Some(menu) = self.menu.upgrade() else { return };
        let mut items = vec![MenuItem::Action(MenuAction::new(
            "copy",
            "Copy the text",
            Some(Icon::Copy),
        ))];
        if can_mute(toast.kind) && toast.level.is_quiet() {
            items.push(MenuItem::Action(MenuAction::new(
                "mute",
                "Don't show this kind again",
                Some(Icon::NoticeMute),
            )));
        }
        items.push(MenuItem::Action(MenuAction::new(
            "close_all",
            "Close all messages",
            Some(Icon::Close),
        )));
        let me = Weak::clone(&self.me);
        let text = if toast.detail.is_empty() {
            toast.title
        } else {
            format!("{}: {}", toast.title.trim_end_matches('.'), toast.detail)
        };
        menu.show(&items, x, y, move |pick| {
            let Some(center) = me.upgrade() else { return };
            match pick {
                "copy" => {
                    crate::util::clipboard::copy(text.clone());
                }
                "mute" => center.mute(id),
                "close_all" => {
                    center.toasts.borrow_mut().close_all(Instant::now());
                    center.publish_toasts();
                }
                _ => {}
            }
        });
    }

    /// "Don't show this kind again" (hints and confirmations only, Q-N8).
    fn mute(&self, id: u64) {
        let Some(kind) = self.toasts.borrow().get(id).map(|t| t.kind) else {
            return;
        };
        if !can_mute(kind) {
            return;
        }
        tracing::info!(kind = kind.id(), "[Notice] Muted");
        self.settings.borrow_mut().muted.insert(kind.id().to_owned());
        self.writer.update(|s| {
            s.notices.muted.insert(kind.id().to_owned());
        });
        self.close_toast(id);
    }

    fn banner_key(&self) -> Option<String> {
        self.banners.borrow().current().map(|(b, _, _)| b.key.clone())
    }

    fn banner_closed(&self) {
        if let Some(key) = self.banner_key() {
            self.banners.borrow_mut().hide(&key);
            self.publish_banner();
        }
    }

    fn banner_clicked(&self) {
        let click = self
            .banner_key()
            .and_then(|key| self.banner_runs.borrow().get(&key).and_then(|r| r.click.clone()));
        run_click(click);
    }

    fn banner_action(&self, index: usize) {
        let Some(key) = self.banner_key() else { return };
        let action = self
            .banner_runs
            .borrow()
            .get(&key)
            .and_then(|r| r.actions.get(index).cloned());
        let Some(action) = action else { return };
        if action.closes {
            self.banners.borrow_mut().hide(&key);
            self.publish_banner();
        }
        (action.run)();
    }

    fn bell_toggled(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let notices = window.global::<Notices<'_>>();
        let open = !notices.get_bell_open();
        notices.set_bell_open(open);
        if open {
            self.history.borrow_mut().mark_read();
        }
        self.publish_bell();
    }

    pub(super) fn show_log(&self) {
        if let Some(show) = &self.hooks.borrow().show_log {
            show();
        }
    }

    fn show_all(&self) {
        if let Some(window) = self.window.upgrade() {
            window.global::<Notices<'_>>().set_bell_open(false);
        }
        self.history.borrow_mut().mark_read();
        self.publish_bell();
        if let Some(show) = &self.hooks.borrow().show_all {
            show();
        }
    }

    // ---- publishing

    fn publish_all(&self) {
        self.publish_toasts();
        self.publish_banner();
        self.publish_bell();
    }

    fn publish_toasts(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let settings = self.settings.borrow();
        let now = Instant::now();
        let mut stack = self.toasts.borrow_mut();
        let layout = stack.layout(&settings.toasts);
        let rows: Vec<ToastView> = layout
            .shown
            .iter()
            .filter_map(|s| stack.get(s.id).map(|t| view::toast(t, *s, now)))
            .collect();
        crate::util::model::sync_keyed(&self.toast_model, rows, |r| r.id);
        let n = window.global::<Notices<'_>>();
        n.set_position(view::position(settings.toasts.position));
        n.set_stacking(view::stacking(settings.toasts.stacking));
        n.set_animation(view::animation(settings.toasts.animation));
        n.set_max_visible(i32::from(settings.toasts.max_visible));
        n.set_more(i32::try_from(layout.more).unwrap_or(i32::MAX));
        n.set_expanded(stack.expanded());
        let (page, pages) = layout.pager.unwrap_or((0, 0));
        n.set_page(i32::try_from(page).unwrap_or(0));
        n.set_pages(i32::try_from(pages).unwrap_or(0));
        n.set_summary(layout.summary.is_some());
        if let Some(summary) = &layout.summary {
            n.set_summary_text(view::summary(summary).into());
            n.set_summary_level(summary.level.index());
        }
    }

    fn publish_banner(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let n = window.global::<Notices<'_>>();
        let settings = self.settings.borrow();
        n.set_banner_animation(view::banner_animation(settings.banners.animation));
        n.set_banner_pulse(self.pulse.get());
        let banners = self.banners.borrow();
        match banners.current() {
            Some((banner, index, count)) => {
                n.set_banner(view::banner(banner));
                n.set_banner_index(i32::try_from(index).unwrap_or(0));
                n.set_banner_count(i32::try_from(count).unwrap_or(0));
                n.set_has_banner(true);
            }
            // The last view stays set so the banner can slide away with its words.
            None => n.set_has_banner(false),
        }
    }

    fn publish_bell(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let n = window.global::<Notices<'_>>();
        let settings = self.settings.borrow();
        let history = self.history.borrow();
        n.set_bell(settings.history.bell);
        let (unread, level) = history.unread(settings.history.badge);
        n.set_unread(if settings.history.badge == BellBadge::None {
            0
        } else {
            i32::try_from(unread).unwrap_or(i32::MAX)
        });
        n.set_unread_level(level.index());
        n.set_total(i32::try_from(history.len()).unwrap_or(i32::MAX));
        crate::util::model::sync(
            &self.recent_model,
            history.recent(RECENT).into_iter().map(view::history).collect(),
        );
    }
}

fn run_click(click: Option<Click>) {
    match click {
        Some(Click::Jump(target)) => crate::ui::jump::go(target),
        Some(Click::Run(run)) => run(),
        None => {}
    }
}

fn id_of(id: i32) -> u64 {
    u64::try_from(id).unwrap_or(0)
}

fn connect(center: &Rc<NoticeCenter>, notices: &Notices<'_>) {
    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident: $ty:ty)*| $body:expr) => {{
            let weak = Rc::downgrade(center);
            notices.$setter(move |$($arg: $ty),*| {
                if let Some($c) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_toast_clicked, |c, id: i32| c.toast_clicked(id_of(id)));
    on!(on_toast_closed, |c, id: i32| c.close_toast(id_of(id)));
    on!(on_toast_action, |c, id: i32, index: i32| c
        .toast_action(id_of(id), usize::try_from(index).unwrap_or(0)));
    on!(on_toast_hover, |c, id: i32, on: bool| {
        c.toasts.borrow_mut().hover(id_of(id), on, Instant::now());
        c.publish_toasts();
    });
    on!(on_toast_menu, |c, id: i32, x: f32, y: f32| c.toast_menu(
        id_of(id),
        x,
        y
    ));
    on!(on_toast_mute, |c, id: i32| c.mute(id_of(id)));
    on!(on_set_expanded, |c, on: bool| {
        c.toasts.borrow_mut().set_expanded(on);
        c.publish_toasts();
    });
    on!(on_page_toasts, |c, delta: i32| {
        c.toasts.borrow_mut().page(delta);
        c.publish_toasts();
    });
    on!(on_banner_closed, |c| c.banner_closed());
    on!(on_banner_clicked, |c| c.banner_clicked());
    on!(on_banner_action, |c, index: i32| c
        .banner_action(usize::try_from(index).unwrap_or(0)));
    on!(on_page_banners, |c, delta: i32| {
        c.banners.borrow_mut().page(delta);
        c.publish_banner();
    });
    on!(on_bell_toggled, |c| c.bell_toggled());
    on!(on_show_all, |c| c.show_all());
}
