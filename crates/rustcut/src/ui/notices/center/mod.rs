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
use super::bubbles::{Area, BubbleCenter, Win};
use super::history::History;
use super::kind::{Kind, Level};
use super::notice::{Action, Click, Notice};
use super::route::{desktop_now, route};
use super::toast_tags::{self, JobLabels};
use super::toasts::{Added, NewToast, ToastStack};
use super::view;

mod input;
use crate::app::SettingsWriter;
use crate::desktop::notify::{self, Note, Style};
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::menu::ContextMenuBridge;
use crate::ui::status;
use crate::{HistoryView, MainWindow, Notices, ToastTagView, ToastView};

/// How often the toasts' times are checked while any shows.
const TICK: Duration = Duration::from_millis(100);
/// The bell lists this many.
const RECENT: usize = 3;
/// How long a test banner stays.
const TEST_BANNER: Duration = Duration::from_secs(8);
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
    /// The key (or kind id) of the message the status line shows: clearing it empties the line.
    status_key: RefCell<Option<String>>,
    desktop_runs: RefCell<HashMap<u64, Rc<dyn Fn()>>>,
    next_desktop: Cell<u64>,
    toast_model: Rc<VecModel<ToastView>>,
    /// The tag pills per toast (one model each: a card keeps its pills while it is published again).
    toast_tags: RefCell<HashMap<u64, Rc<VecModel<ToastTagView>>>>,
    /// The widths Slint measured, by tag name.
    tag_widths: RefCell<HashMap<String, f32>>,
    /// A new measure waits to be shown.
    tags_waiting: Cell<bool>,
    recent_model: Rc<VecModel<HistoryView>>,
    timer: slint::Timer,
    pulse: Cell<i32>,
    hooks: RefCell<Hooks>,
    bubbles: Rc<BubbleCenter>,
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
            status_key: RefCell::default(),
            desktop_runs: RefCell::default(),
            next_desktop: Cell::new(0),
            toast_model: Rc::new(VecModel::default()),
            toast_tags: RefCell::default(),
            tag_widths: RefCell::default(),
            tags_waiting: Cell::new(false),
            recent_model: Rc::new(VecModel::default()),
            timer: slint::Timer::default(),
            pulse: Cell::new(0),
            hooks: RefCell::default(),
            bubbles: BubbleCenter::new(parts.settings.bubbles.opacity),
        });
        center.bubbles.attach(Win::Main, window);
        let notices = window.global::<Notices<'_>>();
        notices.set_toasts(ModelRc::from(Rc::clone(&center.toast_model)));
        notices.set_recent(ModelRc::from(Rc::clone(&center.recent_model)));
        input::connect(&center, &notices);
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
        self.bubbles.set_opacity(settings.bubbles.opacity);
        self.bubbles
            .retain(|kind| route(kind, kind.info().level, settings).place == NoticePlace::Bubble);
        self.publish_all();
    }

    pub fn layout_changed(&self) {
        self.bubbles.look_again(Win::Main);
        self.bubbles.look_again(Win::Settings);
    }

    /// The bubbles of the Settings window show there (it has its own globals).
    pub fn attach_settings(&self, window: &crate::SettingsWindow) {
        self.bubbles.attach(Win::Settings, window);
    }

    pub fn history(&self) -> std::cell::Ref<'_, History> {
        self.history.borrow()
    }

    pub fn post(&self, notice: Notice) {
        let route = route(notice.kind, notice.level, &self.settings.borrow());
        tracing::debug!(kind = notice.kind.id(), place = ?route.place, muted = route.muted, "[Notice] {}", notice.text());
        let bubble_key = || notice.key.clone().unwrap_or_else(|| notice.kind.id().to_owned());
        if route.place == NoticePlace::Bubble {
            // A state at an input: not in the history (it would fill while the user types).
            self.bubbles.set(&bubble_key(), Some(&notice), notice.grace);
            return;
        }
        if notice.anchor.is_some() {
            // A bubble's kind the user moved elsewhere: no bubble stays behind.
            self.bubbles.set(&bubble_key(), None, Duration::ZERO);
        }
        // hh:mm:ss everywhere (review 34/35, P9.2).
        let clock = chrono::Local::now().format("%H:%M:%S").to_string();
        let fresh = match route.place {
            NoticePlace::Toast => self.toast(&notice, clock.clone()),
            NoticePlace::Banner => self.banner(&notice, clock.clone()),
            NoticePlace::Status => {
                if let Some(window) = self.window.upgrade() {
                    status::show(&window, notice.level, &notice.text());
                }
                *self.status_key.borrow_mut() = Some(bubble_key());
                self.quiet_fresh(&notice)
            }
            NoticePlace::Bubble | NoticePlace::Dialog | NoticePlace::Nothing => self.quiet_fresh(&notice),
        };
        if !fresh {
            return;
        }
        self.history.borrow_mut().push(
            clock,
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

    /// The Settings window's test: the kind's sample, marked "(test)", at the place the kind goes now, on the desktop
    /// too when the kind goes there (even with RustCut in front); a banner of a test goes after a few seconds.
    pub fn test(&self, kind: Kind) -> super::Route {
        let info = kind.info();
        let route = route(kind, info.level, &self.settings.borrow());
        let mut notice = Notice::new(kind, format!("{} (test)", info.sample.0)).detail(info.sample.1);
        if route.place == NoticePlace::Bubble {
            // Its input may not be on screen: the test shows it at the test button.
            let key = format!("test:{}", info.id);
            notice = notice.at(Area::Settings, [key.clone()]);
            let me = Weak::clone(&self.me);
            slint::Timer::single_shot(TEST_BANNER, move || {
                if let Some(center) = me.upgrade() {
                    center.clear(&key);
                }
            });
        }
        if route.place == NoticePlace::Banner {
            let key = format!("test:{}", info.id);
            notice = notice.key(key.clone());
            let me = Weak::clone(&self.me);
            slint::Timer::single_shot(TEST_BANNER, move || {
                if let Some(center) = me.upgrade() {
                    center.clear(&key);
                }
            });
        }
        if route.desktop {
            notice = notice.force_desktop();
        }
        self.post(notice);
        route
    }

    /// The state with `key` is over: its toast fades, its banner and its bubble go.
    pub fn clear(&self, key: &str) {
        self.clear_after(key, Duration::ZERO);
    }

    /// Like [`Self::clear`]; a bubble goes after `grace` unless its key changes before (an input check while typing).
    pub fn clear_after(&self, key: &str, grace: Duration) {
        self.bubbles.set(key, None, grace);
        let now = Instant::now();
        let toast = self.toasts.borrow_mut().close_key(key, now).is_some();
        let banner = self.banners.borrow_mut().clear(key);
        self.banner_runs.borrow_mut().remove(key);
        self.quiet_keys.borrow_mut().remove(key);
        if self.status_key.borrow().as_deref() == Some(key) {
            self.status_key.borrow_mut().take();
            if let Some(window) = self.window.upgrade() {
                status::clear(&window);
            }
        }
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
                actions: notice.actions.iter().map(Action::shown).collect(),
                clickable: notice.click.is_some(),
                time,
                shown_for,
                job: notice.job.clone(),
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

    fn banner(&self, notice: &Notice, time: String) -> bool {
        let key = notice.key.clone().unwrap_or_else(|| notice.kind.id().to_owned());
        let set = self.banners.borrow_mut().set(NewBanner {
            key: key.clone(),
            kind: notice.kind,
            level: notice.level,
            title: notice.title.clone(),
            detail: notice.detail.clone(),
            actions: notice.actions.iter().map(Action::shown).collect(),
            clickable: notice.click.is_some(),
            progress: notice.progress,
            time,
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
            .filter_map(|s| {
                stack.get(s.id).map(|t| {
                    let mut row = view::toast(t, *s, now);
                    if let Some(job) = &t.job {
                        self.show_job(&mut row, t.id, job);
                    }
                    row
                })
            })
            .collect();
        crate::util::model::sync_keyed(&self.toast_model, rows, |r| r.id);
        self.toast_tags
            .borrow_mut()
            .retain(|id, _| layout.shown.iter().any(|s| s.id == *id));
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

    /// A saved job's rating and tags on its card.
    fn show_job(&self, row: &mut ToastView, id: u64, job: &JobLabels) {
        let place = toast_tags::place(job, &self.tag_widths.borrow());
        let mut models = self.toast_tags.borrow_mut();
        let model = models.entry(id).or_default();
        crate::util::model::sync(model, place.pills.iter().map(view::tag_view).collect());
        view::job(row, job, &place, ModelRc::from(Rc::clone(model)));
    }

    /// Slint measured a tag's pill: the pills are placed again when that changes anything. Later, not here: Slint
    /// measures while it makes the pills, and a row changed at that moment keeps its old values.
    fn tag_measured(&self, text: &str, width: f32) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        let old = self.tag_widths.borrow_mut().insert(text.to_owned(), width);
        if old.is_none_or(|w| (w - width).abs() > f32::EPSILON) && !self.tags_waiting.replace(true) {
            let me = Weak::clone(&self.me);
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(center) = me.upgrade() {
                    center.tags_waiting.set(false);
                    center.publish_toasts();
                }
            });
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

pub(super) fn run_click(click: Option<Click>) {
    match click {
        Some(Click::Jump(target)) => crate::ui::jump::go(target),
        Some(Click::Run(run)) => run(),
        None => {}
    }
}
