//! The timeline's thumbnails on the UI side (P12 block 6, user decision Q-P12e): follows the pointer on the timeline, shows
//! the nearest picture as the big picture shows it, keeps the strip of generated places and the status bar's activity, and
//! tells the core's service what the user looks at. The pictures stay as stored: only the one on screen is decoded and turned
//! (0.16 to 0.26 ms, `rustcut-core/benches/thumb_view.rs`), on the UI thread, and a few recent ones are kept.

pub mod geometry;
mod messages;

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustcut_core::media::MediaInfo;
use rustcut_core::settings::{ThumbLook, ThumbMissing, ThumbProgress, ThumbZone, ThumbnailSettings};
use rustcut_core::thumbs::{
    Thumb, ThumbCommand, ThumbEvent, ThumbHandle, ThumbService, ThumbShared, ThumbStats, ThumbVideo, ThumbView, render,
};
use rustcut_core::time::Timestamp;
use slint::{ComponentHandle as _, Image, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer};

use crate::app::SettingsWriter;

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::ui::filters::FiltersBridge;
use crate::ui::status::activity;
use crate::ui::timeline::viewport::Viewport;
use crate::{MainWindow, Theme, Thumbs, Timeline, TimelineMetrics};

/// The pointer rests this long, then the exact picture is fetched.
const REST: Duration = Duration::from_millis(300);
/// Pictures kept decoded: a pointer that goes back and forth does not decode again.
const DECODED: usize = 48;
/// A picture this many spacings away counts as far (the dot, the empty field).
const FAR_SPACINGS: u64 = 2;
/// The status bar shows the generation once it takes longer than this.
const ACTIVITY_AFTER: Duration = Duration::from_secs(3);
const ACTIVITY_KEY: &str = "thumbnails";
/// The strip's lines stay in the background (user request, 2026-11-01).
const STRIP_ALPHA: u8 = 140;
/// A picture with the time under it is at least this wide (`thumb_popup.slint`).
const TIME_WIDTH: f32 = 84.0;

pub use messages::{check_cache_size, set_fallback_memory};

/// The pointer on the timeline, in track pixels.
#[derive(Debug, Clone, Copy)]
pub struct TimelineHover {
    pub x: f32,
    pub time: Timestamp,
    pub on_ruler: bool,
    pub dragging: bool,
}

/// The pointer in the video column's coordinates.
#[derive(Debug, Clone, Copy)]
struct Place {
    x: f32,
    ms: u64,
    time: Timestamp,
}

#[derive(Default)]
struct Decoded {
    view: Option<ThumbView>,
    recent: VecDeque<(u64, Image, f32)>,
}

pub struct ThumbsBridge {
    window: slint::Weak<MainWindow>,
    handle: ThumbHandle,
    shared: Arc<Mutex<ThumbShared>>,
    settings: RefCell<ThumbnailSettings>,
    filters: RefCell<Weak<FiltersBridge>>,
    place: Cell<Option<Place>>,
    shown: Cell<bool>,
    delay: slint::Timer,
    rest: slint::Timer,
    decoded: RefCell<Decoded>,
    viewport: RefCell<Option<Viewport>>,
    last_view: Cell<(u64, u64)>,
    strip_key: Cell<(usize, u32, u64, u64)>,
    running_since: Cell<Option<Instant>>,
    activity_on: Cell<bool>,
    jobs: RefCell<Vec<(i64, PathBuf)>>,
    rendering: Cell<bool>,
    stats: Cell<ThumbStats>,
    writer: SettingsWriter,
    runtime: tokio::runtime::Handle,
    ui: UiDispatcher,
    this: RefCell<Weak<Self>>,
}

thread_local! {
    static BRIDGE: RefCell<Weak<ThumbsBridge>> = const { RefCell::new(Weak::new()) };
}

fn with(run: impl FnOnce(&ThumbsBridge)) {
    if let Some(bridge) = BRIDGE.with_borrow(Weak::upgrade) {
        run(&bridge);
    }
}

/// The editor's report of the pointer and the view.
pub fn timeline_changed(hover: Option<TimelineHover>, viewport: &Viewport) {
    with(|bridge| bridge.timeline_changed(hover, viewport));
}

/// The key and the View menu.
pub fn toggle() {
    with(|bridge| {
        bridge
            .writer
            .update(|settings| settings.thumbnails.enabled = !settings.thumbnails.enabled);
    });
}

/// The hold button in Settings.
pub fn clear_cache() {
    with(|bridge| {
        let _ = bridge.handle.send(ThumbCommand::ClearCache);
        bridge.refresh_stats();
    });
}

/// The hold button's tooltip.
pub fn stats_text() -> String {
    let mut text = String::new();
    with(|bridge| {
        let stats = bridge.stats.get();
        text = format!(
            "Empties the cache of job sources (videos: {}, thumbnails: {}, size: {}).",
            stats.sources,
            stats.thumbs,
            humansize::format_size(stats.bytes, humansize::BINARY),
        );
    });
    text
}

fn ms_of(time: Timestamp) -> u64 {
    u64::try_from(time.as_micros() / 1000).unwrap_or(0)
}

impl ThumbsBridge {
    pub fn install(
        window: &MainWindow,
        service: &ThumbService,
        settings: ThumbnailSettings,
        writer: SettingsWriter,
        ui: &UiDispatcher,
        runtime: &tokio::runtime::Handle,
    ) -> Rc<Self> {
        window.global::<Thumbs<'_>>().set_enabled(settings.enabled);
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            handle: service.handle.clone(),
            shared: Arc::clone(&service.shared),
            settings: RefCell::new(settings),
            filters: RefCell::new(Weak::new()),
            place: Cell::new(None),
            shown: Cell::new(false),
            delay: slint::Timer::default(),
            rest: slint::Timer::default(),
            decoded: RefCell::default(),
            viewport: RefCell::new(None),
            last_view: Cell::new((0, 0)),
            strip_key: Cell::new((0, 0, 0, 0)),
            running_since: Cell::new(None),
            activity_on: Cell::new(false),
            jobs: RefCell::default(),
            rendering: Cell::new(false),
            stats: Cell::new(ThumbStats::default()),
            writer,
            runtime: runtime.clone(),
            ui: ui.clone(),
            this: RefCell::new(Weak::new()),
        });
        *bridge.this.borrow_mut() = Rc::downgrade(&bridge);
        BRIDGE.set(Rc::downgrade(&bridge));
        follow_events(service, ui, runtime);
        bridge.refresh_stats();
        bridge
    }

    pub fn set_filters(&self, filters: &Rc<FiltersBridge>) {
        *self.filters.borrow_mut() = Rc::downgrade(filters);
    }

    /// The probe has news about the open video.
    pub fn probed(&self, path: Option<PathBuf>, info: Option<&Arc<MediaInfo>>) {
        let video = path.zip(info).and_then(|(path, info)| {
            let (_stream, video) = info.primary_video()?;
            let duration = info.container.duration?;
            let aspect = video.sample_aspect.map_or((1, 1), |ratio| {
                (
                    i32::try_from(ratio.num).unwrap_or(1),
                    i32::try_from(ratio.den).unwrap_or(1),
                )
            });
            Some(ThumbVideo {
                path,
                duration_ms: ms_of(duration),
                size: (video.width?, video.height?),
                aspect,
            })
        });
        let command = video.map_or(ThumbCommand::Close, ThumbCommand::Open);
        let _ = self.handle.send(command);
    }

    /// The Settings window changed something.
    pub fn apply_settings(&self, settings: &ThumbnailSettings, render_nice: u8) {
        *self.settings.borrow_mut() = settings.clone();
        if let Some(window) = self.window.upgrade() {
            window.global::<Thumbs<'_>>().set_enabled(settings.enabled);
        }
        let _ = self.handle.send(ThumbCommand::Settings {
            settings: settings.clone(),
            render_nice,
        });
        self.strip_key.set((0, 0, 0, 0));
        self.refresh_strip();
        self.place_changed();
    }

    /// After a refresh of the queue: which sources have jobs. A job that went away lets the cache follow.
    pub fn jobs_changed(&self, jobs: Vec<(i64, PathBuf)>) {
        let removed = {
            let mut known = self.jobs.borrow_mut();
            if *known == jobs {
                return;
            }
            let removed = known.iter().any(|(id, _)| !jobs.iter().any(|(other, _)| other == id));
            *known = jobs;
            removed
        };
        let sources: HashSet<PathBuf> = self.jobs.borrow().iter().map(|(_, source)| source.clone()).collect();
        let _ = self.handle.send(ThumbCommand::Jobs { sources, removed });
        self.refresh_stats();
    }

    pub fn rendering_changed(&self, rendering: bool) {
        if self.rendering.replace(rendering) != rendering {
            let _ = self.handle.send(ThumbCommand::Rendering(rendering));
        }
    }

    /// Asks for the cache's numbers; the tooltip shows the last answer.
    fn refresh_stats(&self) {
        let (handle, ui) = (self.handle.clone(), self.ui.clone());
        self.runtime.spawn(async move {
            if let Ok(stats) = handle.request(|reply| ThumbCommand::Stats { reply }).await {
                ui.post(move || with(|bridge| bridge.stats.set(stats)));
            }
        });
    }

    /// The editor's report of the pointer and the view.
    pub fn timeline_changed(&self, hover: Option<TimelineHover>, viewport: &Viewport) {
        *self.viewport.borrow_mut() = Some(*viewport);
        self.follow_view(viewport);
        self.refresh_strip();
        let settings = self.settings.borrow();
        let wanted = hover.filter(|hover| {
            settings.enabled
                && (settings.while_dragging || !hover.dragging)
                && match settings.zone {
                    ThumbZone::RulerAndTrack => true,
                    ThumbZone::Ruler => hover.on_ruler,
                    ThumbZone::Track => !hover.on_ruler,
                }
        });
        let delay = settings.delay_ms;
        drop(settings);
        let Some(hover) = wanted else {
            self.place.set(None);
            self.delay.stop();
            self.rest.stop();
            self.hide();
            return;
        };
        let inset = self.inset();
        let place = Place {
            x: hover.x + inset,
            ms: ms_of(hover.time),
            time: hover.time,
        };
        let entering = self.place.get().is_none();
        self.place.set(Some(place));
        let _ = self.handle.send(ThumbCommand::Hover { ms: place.ms });
        let weak = self.this.borrow().clone();
        self.rest.start(slint::TimerMode::SingleShot, REST, move || {
            if let Some(bridge) = weak.upgrade()
                && let Some(place) = bridge.place.get()
            {
                let _ = bridge.handle.send(ThumbCommand::Rest { ms: place.ms });
            }
        });
        if entering && delay > 0 {
            let weak = self.this.borrow().clone();
            self.delay.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(u64::from(delay)),
                move || {
                    if let Some(bridge) = weak.upgrade() {
                        bridge.place_changed();
                    }
                },
            );
        } else if !self.delay.running() {
            self.place_changed();
        }
    }

    fn inset(&self) -> f32 {
        self.window
            .upgrade()
            .map_or(0.0, |window| window.global::<TimelineMetrics<'_>>().get_inset())
    }

    fn follow_view(&self, viewport: &Viewport) {
        let (from, to) = viewport.visible();
        let view = (ms_of(from), ms_of(to));
        if self.last_view.replace(view) != view && viewport.is_zoomed() {
            let _ = self.handle.send(ThumbCommand::View {
                from_ms: view.0,
                to_ms: view.1,
            });
        }
    }

    fn hide(&self) {
        if self.shown.replace(false)
            && let Some(window) = self.window.upgrade()
        {
            window.global::<Thumbs<'_>>().set_shown(false);
            window.global::<Timeline<'_>>().set_hover_time_covered(false);
        }
    }

    fn view(&self) -> ThumbView {
        let show_filters = self.settings.borrow().show_filters;
        self.filters
            .borrow()
            .upgrade()
            .map_or_else(ThumbView::default, |filters| filters.thumb_view(show_filters))
    }

    /// Whether the moment allows a picture: the crop is edited, or the probe plays.
    fn allowed(&self, window: &MainWindow) -> bool {
        let editing = self
            .filters
            .borrow()
            .upgrade()
            .is_some_and(|filters| filters.editing_crop());
        !editing && !window.global::<Timeline<'_>>().get_locked()
    }

    /// Shows what belongs to the current place.
    fn place_changed(&self) {
        let (Some(place), Some(window)) = (self.place.get(), self.window.upgrade()) else {
            return;
        };
        if !self.allowed(&window) {
            self.hide();
            return;
        }
        let settings = self.settings.borrow();
        let nearest = {
            let shared = self.shared.lock();
            let far_from = shared.spacing_ms.saturating_mul(FAR_SPACINGS);
            shared.set.nearest(place.ms).cloned().map(|thumb| {
                let far = thumb.time_ms.abs_diff(place.ms) > far_from;
                (thumb, far)
            })
        };
        let (thumb, far) = match nearest {
            Some(pair) => (Some(pair.0), pair.1),
            None => (None, true),
        };
        if far && settings.missing == ThumbMissing::Nothing
            || thumb.is_none() && settings.missing != ThumbMissing::EmptyField
        {
            self.hide();
            return;
        }
        let empty = far && settings.missing == ThumbMissing::EmptyField || thumb.is_none();
        let picture = thumb.filter(|_| !empty).and_then(|thumb| self.picture(&thumb));
        let empty = picture.is_none();
        let aspect = picture.as_ref().map_or(16.0 / 9.0, |(_, aspect)| *aspect);
        let column = self.viewport.borrow().as_ref().map_or(0.0, Viewport::width) + 2.0 * self.inset();
        let (width, height) = geometry::size(&settings, column, aspect);
        let thumbs = window.global::<Thumbs<'_>>();
        if let Some((image, _)) = picture {
            thumbs.set_picture(image);
        }
        thumbs.set_empty(empty);
        thumbs.set_dot(far && !empty && settings.missing == ThumbMissing::NearestWithDot);
        thumbs.set_time(place.time.to_string().into());
        thumbs.set_time_below(settings.look == ThumbLook::TimeBelow);
        thumbs.set_time_inside(settings.look == ThumbLook::TimeInside);
        thumbs.set_picture_width(width);
        thumbs.set_picture_height(height);
        let box_width = if settings.look == ThumbLook::TimeBelow {
            width.max(TIME_WIDTH)
        } else {
            width
        };
        thumbs.set_left(geometry::left(settings.place, column, place.x, box_width));
        thumbs.set_shown(true);
        window
            .global::<Timeline<'_>>()
            .set_hover_time_covered(settings.look != ThumbLook::PictureOnly);
        self.shown.set(true);
    }

    /// The picture on screen, decoded and turned once.
    fn picture(&self, thumb: &Thumb) -> Option<(Image, f32)> {
        let view = self.view();
        let mut decoded = self.decoded.borrow_mut();
        if decoded.view != Some(view) {
            decoded.view = Some(view);
            decoded.recent.clear();
        }
        if let Some(found) = decoded.recent.iter().find(|(time, _, _)| *time == thumb.time_ms) {
            return Some((found.1.clone(), found.2));
        }
        let shown = render(&thumb.webp, &view)?;
        let (width, height) = shown.dimensions();
        let buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(shown.as_raw(), width, height);
        let image = Image::from_rgb8(buffer);
        let aspect = shape(width, height);
        if decoded.recent.len() >= DECODED {
            decoded.recent.pop_front();
        }
        decoded.recent.push_back((thumb.time_ms, image.clone(), aspect));
        Some((image, aspect))
    }

    /// The strip of generated places: one line per picture, in the timeline's coordinates.
    fn refresh_strip(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let settings = self.settings.borrow();
        let viewport = self.viewport.borrow();
        let thumbs = window.global::<Thumbs<'_>>();
        let (Some(viewport), true) = (
            viewport.as_ref(),
            settings.enabled && settings.progress == ThumbProgress::Strip,
        ) else {
            thumbs.set_strip_shown(false);
            return;
        };
        let shared = self.shared.lock();
        if shared.path.is_none() {
            thumbs.set_strip_shown(false);
            return;
        }
        let width = pixels_of(viewport.width());
        let (from, to) = viewport.visible();
        let key = (shared.set.len(), width, ms_of(from), ms_of(to));
        if self.strip_key.replace(key) == key {
            return;
        }
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), 1);
        let color = window.global::<Theme<'_>>().get_text_muted().to_argb_u8();
        let line = Rgba8Pixel::new(color.red, color.green, color.blue, STRIP_ALPHA);
        let pixels = buffer.make_mut_slice();
        for time in shared.set.times() {
            let x = viewport.x_of(Timestamp::from_millis(i64::try_from(time).unwrap_or(0)));
            if x >= 0.0
                && let Some(pixel) = usize::try_from(pixels_of(x)).ok().and_then(|x| pixels.get_mut(x))
            {
                *pixel = line;
            }
        }
        drop(shared);
        thumbs.set_strip(Image::from_rgba8(buffer));
        thumbs.set_strip_shown(true);
    }

    /// The service has news.
    fn updated(&self) {
        self.refresh_strip();
        if self.shown.get() || self.place.get().is_some() {
            self.place_changed();
        }
        self.follow_activity();
        if !self.shared.lock().running {
            self.refresh_stats();
        }
    }

    /// The status bar's entry, when the settings ask for it and the work takes a while.
    fn follow_activity(&self) {
        let (running, done, total) = {
            let shared = self.shared.lock();
            (shared.running, shared.done, shared.total)
        };
        let wanted = self.settings.borrow().progress == ThumbProgress::StatusBar;
        if !running || !wanted {
            self.running_since.set(None);
            if self.activity_on.replace(false) {
                activity::finish(ACTIVITY_KEY);
            }
            return;
        }
        let since = self.running_since.get().unwrap_or_else(Instant::now);
        self.running_since.set(Some(since));
        let text = format!("Thumbnails {done} / {total}");
        if self.activity_on.get() {
            activity::update(ACTIVITY_KEY, text);
        } else if since.elapsed() >= ACTIVITY_AFTER {
            self.activity_on.set(true);
            let handle = self.handle.clone();
            activity::begin(
                ACTIVITY_KEY,
                text,
                Some(Rc::new(move || {
                    let _ = handle.send(ThumbCommand::Cancel);
                })),
            );
        }
    }
}

fn pixels_of(length: f32) -> u32 {
    az::saturating_cast(length.round())
}

/// Width by height of a small picture.
fn shape(width: u32, height: u32) -> f32 {
    let side = |value: u32| f32::from(u16::try_from(value).unwrap_or(u16::MAX));
    side(width) / side(height.max(1))
}

/// The service's events on the UI thread.
fn follow_events(service: &ThumbService, ui: &UiDispatcher, runtime: &tokio::runtime::Handle) {
    let mut events = service.events.subscribe();
    let ui = ui.clone();
    runtime.spawn(async move {
        loop {
            match events.recv().await {
                Ok(ThumbEvent::Updated) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    ui.post_latest(CoalesceKey::new("thumbs", 0), rate::PROGRESS, || {
                        with(ThumbsBridge::updated);
                    });
                }
                Ok(ThumbEvent::HelperWorks) => ui.post(messages::helper_worked),
                Ok(ThumbEvent::FallbackUsed) => ui.post(messages::fallback_used),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}
