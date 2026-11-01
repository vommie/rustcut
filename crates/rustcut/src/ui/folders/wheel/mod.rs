//! The folder wheel (P11.5, user decision Q-FP2): opened by holding the middle mouse button over the video (a ring fills
//! first) or by its key anywhere; always centered on the video. `nav.rs` is the navigation, `geometry.rs` the size and
//! the shapes; this bridge feeds them the pointer, the keys and the time, and draws the result into `FolderWheel`.
//! Picking a folder only sets the Job Finalization card's target (never saves). `draw.rs` makes what a wheel shows (the
//! small wheel in Settings shares it), `anim.rs` where it stands in an animation.

pub mod anim;
pub mod draw;
pub mod geometry;
pub mod label;
pub mod nav;

use std::cell::{Cell, RefCell};
use std::f32::consts::TAU;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use rustcut_core::folders::{FolderId, FolderTree, Layout};
use rustcut_core::settings::{Settings, WheelButton, WheelLevels, WheelOpening};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use self::anim::{Pose, Run};
use self::draw::{MiddleInput, Scene};
use self::nav::{Context, KeyAction, Mode, Outcome, Radii, Timing, WheelNav};
use super::pictures::Pictures;
use super::presence::FolderPresence;
use crate::app::SettingsWriter;
use crate::ui::export::ExportBridge;
use crate::ui::keymap::{Action, Chord, Keymap};
use crate::util::model::sync;
use crate::{FolderWheel, MainWindow, Shortcuts, WheelSlotView};

/// How often a running hold or dwell is drawn again.
const TICK: Duration = Duration::from_millis(16);
/// The hold ring's square (`WheelHoldRing`).
const RING: f32 = 64.0;
/// How long the current folder lights up at the opening, and the middle fades over, at normal speed (ms).
const PULSE_MS: f32 = 420.0;
const FADE_MS: f32 = 160.0;

fn timing(settings: &Settings) -> Timing {
    let w = &settings.folder_wheel;
    Timing {
        hold_ms: u64::from(w.hold_ms),
        dwell_ms: u64::from(w.dwell_ms),
        push: w.push,
        center: w.pointer_center,
    }
}

fn nav_radii(settings: &Settings, diameter: f32) -> Radii {
    let w = &settings.folder_wheel;
    let (inner, outer) = geometry::radii(diameter, w.middle_size);
    Radii {
        inner,
        outer,
        dead: geometry::share(diameter, w.pointer_distance),
    }
}

fn layout(settings: &Settings) -> Layout {
    Layout {
        places: settings.folder_wheel.places,
        divided: settings.folder_wheel.divided,
    }
}

/// A key's text as a wheel key; `again` = the wheel's own keys (their labels: "F").
fn key_action(text: &str, again: &[String]) -> Option<KeyAction> {
    use slint::platform::Key;
    let is = |k: Key| text == slint::SharedString::from(k).as_str();
    let lower = text.to_lowercase();
    if again
        .iter()
        .any(|k| k.chars().count() == 1 && k.to_lowercase() == lower)
    {
        return Some(KeyAction::Again);
    }
    match lower.as_str() {
        "a" => return Some(KeyAction::Left),
        "d" => return Some(KeyAction::Right),
        "w" => return Some(KeyAction::Open),
        "s" => return Some(KeyAction::Back),
        _ => {}
    }
    [
        (Key::LeftArrow, KeyAction::Left),
        (Key::RightArrow, KeyAction::Right),
        (Key::UpArrow, KeyAction::Open),
        (Key::DownArrow, KeyAction::Back),
        (Key::Return, KeyAction::Enter),
        (Key::Escape, KeyAction::Escape),
    ]
    .into_iter()
    .find(|(k, _)| is(*k))
    .map(|(_, a)| a)
}

pub struct WheelBridge {
    window: slint::Weak<MainWindow>,
    writer: SettingsWriter,
    pictures: Rc<Pictures>,
    export: Weak<ExportBridge>,
    nav: RefCell<WheelNav>,
    started: Instant,
    timer: slint::Timer,
    /// Where the hold started, relative to the video's center (the ring stands there).
    held_at: RefCell<(f32, f32)>,
    /// The missing folders' check (the warning mark).
    presence: Rc<FolderPresence>,
    runtime: tokio::runtime::Handle,
    /// The running animation and when it started.
    running: Cell<Option<(Run, u64)>>,
    /// When the wheel opened (the current folder's flash) and when the middle changed.
    opened_at: Cell<Option<u64>>,
    middle_at: Cell<(Option<usize>, u64)>,
    slot_rows: Rc<VecModel<WheelSlotView>>,
}

impl WheelBridge {
    pub fn install(
        window: &MainWindow,
        writer: SettingsWriter,
        pictures: &Rc<Pictures>,
        export: &Rc<ExportBridge>,
        runtime: tokio::runtime::Handle,
    ) -> Rc<Self> {
        let settings = writer.current();
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            nav: RefCell::new(WheelNav::new(
                timing(&settings),
                Radii {
                    inner: 50.0,
                    outer: 150.0,
                    dead: 30.0,
                },
            )),
            writer,
            pictures: Rc::clone(pictures),
            export: Rc::downgrade(export),
            started: Instant::now(),
            timer: slint::Timer::default(),
            held_at: RefCell::new((0.0, 0.0)),
            presence: Rc::default(),
            runtime,
            running: Cell::new(None),
            opened_at: Cell::new(None),
            middle_at: Cell::new((None, 0)),
            slot_rows: Rc::default(),
        });
        let ui = window.global::<FolderWheel<'_>>();
        ui.set_slots(ModelRc::from(Rc::clone(&bridge.slot_rows)));
        let weak = Rc::downgrade(&bridge);
        ui.on_pressed(move |x, y| {
            if let Some(b) = weak.upgrade() {
                b.pressed(x, y);
            }
        });
        let weak = Rc::downgrade(&bridge);
        ui.on_moved(move |x, y| {
            if let Some(b) = weak.upgrade() {
                b.moved(x, y);
            }
        });
        let weak = Rc::downgrade(&bridge);
        ui.on_released(move || {
            if let Some(b) = weak.upgrade() {
                b.released();
            }
        });
        let weak = Rc::downgrade(&bridge);
        ui.on_clicked(move |x, y| {
            if let Some(b) = weak.upgrade() {
                b.clicked(x, y);
            }
        });
        let weak = Rc::downgrade(&bridge);
        ui.on_key(move |text| weak.upgrade().is_some_and(|b| b.key(&text)));
        WHEEL.with(|w| *w.borrow_mut() = Rc::downgrade(&bridge));
        let weak = Rc::downgrade(&bridge);
        pictures.listen(move || {
            if let Some(b) = weak.upgrade()
                && b.nav.borrow().is_open()
            {
                b.publish();
            }
        });
        bridge
    }

    fn now(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The wheel's diameter in the main window as it is now.
    fn diameter(&self, settings: &Settings) -> f32 {
        let Some(window) = self.window.upgrade() else {
            return geometry::MIN_DIAMETER;
        };
        let w = window.window();
        let size = w.size().to_logical(w.scale_factor());
        geometry::diameter(settings.folder_wheel.size, size.width, size.height)
    }

    /// Runs `step` with the navigation, the tree and the layout; then acts on its outcome.
    fn step(&self, step: impl FnOnce(&mut WheelNav, Context<'_>, u64) -> Outcome) {
        let settings = self.writer.current();
        let radii = nav_radii(&settings, self.diameter(&settings));
        let current = self.current();
        let now = self.now();
        let (outcome, change) = {
            let mut nav = self.nav.borrow_mut();
            nav.set_timing(timing(&settings));
            nav.set_radii(radii);
            let ctx = Context {
                tree: &settings.export.folders,
                layout: layout(&settings),
                current,
            };
            let outcome = step(&mut nav, ctx, now);
            (outcome, nav.take_change())
        };
        let w = &settings.folder_wheel;
        if let Some(change) = change.filter(|_| w.animations && w.levels != WheelLevels::Off) {
            self.running.set(Some((
                Run::Level {
                    kind: w.levels,
                    back: change.back,
                    place: change.place,
                },
                now,
            )));
        }
        match outcome {
            // A running animation still has to be drawn to its end.
            Outcome::Nothing if !self.timer.running() => return,
            Outcome::Nothing => {}
            Outcome::Picked(id) => {
                tracing::info!(id = id.0, "[Folders] Target folder chosen on the wheel");
                if let Some(export) = self.export.upgrade() {
                    export.choose_target(id);
                }
                self.give_back_focus();
            }
            Outcome::Closed => {
                tracing::debug!("[Folders] Folder wheel closed");
                self.give_back_focus();
            }
            Outcome::Opened => {
                tracing::debug!("[Folders] Folder wheel opened");
                self.opened_at.set(Some(now));
                self.middle_at.set((None, 0));
                self.running
                    .set((w.animations && w.opening != WheelOpening::Off).then_some((Run::Open(w.opening), now)));
                self.refresh_presence(&settings);
            }
            Outcome::Settings => {
                tracing::info!("[Folders] The empty folder wheel leads to Settings > Target Folders");
                self.give_back_focus();
                crate::ui::jump::go(crate::ui::jump::JumpTarget::Settings {
                    page: crate::ui::settings::page::Page::Folders,
                    key: crate::ui::export::FOLDERS_KEY,
                });
            }
            Outcome::Changed => {}
        }
        self.publish();
        self.keep_ticking();
    }

    fn give_back_focus(&self) {
        if let Some(window) = self.window.upgrade() {
            window.global::<Shortcuts<'_>>().invoke_release_focus();
        }
    }

    /// The job's target folder.
    fn current(&self) -> Option<FolderId> {
        self.export.upgrade().and_then(|e| e.current_target())
    }

    /// Checks the folders on the runtime (the first wheel draws before the answer comes; then it draws again).
    fn refresh_presence(&self, settings: &Settings) {
        let paths = settings
            .export
            .folders
            .folders()
            .into_iter()
            .filter_map(|f| f.path.clone())
            .collect();
        let weak = WHEEL.with(|w| w.borrow().clone());
        self.presence.refresh(&self.runtime, paths, move || {
            if let Some(b) = weak.upgrade()
                && b.nav.borrow().is_open()
            {
                b.publish();
            }
        });
    }

    /// An animation, the current folder's flash or the middle's fade still runs.
    fn animating(&self, settings: &Settings) -> bool {
        let w = &settings.folder_wheel;
        if !w.animations || !self.nav.borrow().is_open() {
            return false;
        }
        let now = self.now();
        let speed = f32::from(w.animation_speed) / 100.0;
        let since = |at: u64| to_ms(now.saturating_sub(at));
        let n = self.nav.borrow().level().map_or(0, |l| l.slots.len());
        let running = self
            .running
            .get()
            .is_some_and(|(run, at)| !run.is_done(since(at), w.animation_speed, n));
        let pulse = w.pulse && self.opened_at.get().is_some_and(|at| since(at) < PULSE_MS * speed);
        let fade = w.crossfade && since(self.middle_at.get().1) < FADE_MS * speed;
        running || pulse || fade
    }

    /// A timer draws a running hold, dwell or animation (and lets it act) while one runs.
    fn keep_ticking(&self) {
        let settings = self.writer.current();
        let needed = self.nav.borrow().needs_ticks() || self.animating(&settings);
        if needed && !self.timer.running() {
            let weak = WHEEL.with(|w| w.borrow().clone());
            self.timer.start(slint::TimerMode::Repeated, TICK, move || {
                if let Some(b) = weak.upgrade() {
                    b.step(|nav, ctx, now| nav.tick(now, ctx));
                }
            });
        } else if !needed && self.timer.running() {
            self.timer.stop();
        }
    }

    /// The middle button went down over the video (`x`, `y` from its center).
    fn pressed(&self, x: f32, y: f32) {
        if self.writer.current().folder_wheel.button == WheelButton::Off {
            return;
        }
        *self.held_at.borrow_mut() = (x, y);
        self.step(|nav, ctx, now| {
            let out = nav.press(now);
            nav.moved(x, y, now, ctx);
            out
        });
    }

    fn moved(&self, x: f32, y: f32) {
        self.step(|nav, ctx, now| nav.moved(x, y, now, ctx));
    }

    fn released(&self) {
        self.step(|nav, ctx, _| nav.release(ctx));
    }

    fn clicked(&self, x: f32, y: f32) {
        self.step(|nav, ctx, _| nav.click(x, y, ctx));
    }

    fn key(&self, text: &str) -> bool {
        // The wheel's own keys as the keymap has them now ("F"): pressed again, one level up.
        let (keymap, _notes) = Keymap::from_table(&self.writer.current().keymap);
        let again: Vec<String> = keymap.chords(Action::FolderWheel).iter().map(Chord::label).collect();
        let Some(action) = key_action(text, &again) else {
            return false;
        };
        self.step(|nav, ctx, _| nav.key(action, ctx));
        true
    }

    /// The wheel's key (the keymap): opens it, or one level up while it is open.
    pub fn open_by_key(&self) {
        if self.nav.borrow().is_arming() {
            return;
        }
        self.step(|nav, ctx, _| nav.open_by_key(ctx));
    }

    /// The video went away or a dialog opened: the wheel closes.
    pub fn close(&self) {
        if self.nav.borrow().is_open() || self.nav.borrow().is_arming() {
            self.nav.borrow_mut().close();
            self.publish();
            self.keep_ticking();
        }
    }

    /// Draws the wheel as the navigation has it.
    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<FolderWheel<'_>>();
        let settings = self.writer.current();
        let nav = self.nav.borrow();
        let now = self.now();
        let w = &settings.folder_wheel;
        ui.set_arming(nav.is_arming());
        let (hx, hy) = *self.held_at.borrow();
        ui.set_ring_x(hx);
        ui.set_ring_y(hy);
        ui.set_ring_arc(
            geometry::arc(
                RING / 2.0,
                RING / 2.0 - 8.5,
                0.0,
                nav.hold_progress(now).max(0.01) * TAU,
            )
            .into(),
        );
        ui.set_tracking(nav.is_arming() || nav.mode() == Some(Mode::Held));
        ui.set_shown(nav.is_open());
        if !nav.is_open() {
            // The next wheel is measured from the video's center until its layer knows where it stands.
            ui.set_shift_x(0.0);
            ui.set_shift_y(0.0);
        }
        ui.set_keys(matches!(nav.mode(), Some(Mode::Keys | Mode::Clicks)));
        let dot = nav.virtual_pointer();
        ui.set_hide_pointer(dot.is_some());
        ui.set_dot(dot.is_some());
        let (dx, dy) = dot.unwrap_or_default();
        ui.set_dot_x(dx);
        ui.set_dot_y(dy);
        if !nav.is_open() {
            return;
        }
        let size = self.diameter(&settings);
        let (inner, _) = geometry::radii(size, w.middle_size);
        let c = size / 2.0;
        ui.set_look(draw::look(w, size, draw::font(size)));
        ui.set_dim(if w.dim { f32::from(w.video_dim) / 100.0 } else { 0.0 });
        let tree = &settings.export.folders;
        let Some(level) = nav.level() else { return };
        let n = level.slots.len();
        let current = self.current();
        let run = self.running.get().filter(|_| w.animations);
        let since = |at: u64| to_ms(now.saturating_sub(at));
        let speed = w.animation_speed;
        let pose = |i: usize| run.map_or(Pose::REST, |(r, at)| r.spoke(since(at), speed, i, n));
        let whole = run.map_or(Pose::REST, |(r, at)| r.whole(since(at), speed, n));
        ui.set_whole_alpha(whole.alpha);
        ui.set_whole_zoom(whole.zoom);
        ui.set_whole_turn(whole.turn);
        let flash = self
            .opened_at
            .get()
            .filter(|_| w.animations && w.pulse)
            .map_or(0.0, |at| anim::pulse(since(at), PULSE_MS * f32::from(speed) / 100.0));
        let missing = |path: &std::path::Path| self.presence.is_missing(path);
        let scale = window.window().scale_factor();
        let scene = Scene {
            wheel: w,
            tree,
            level,
            diameter: size,
            scale,
            highlight: nav.highlight(),
            current,
            fill: nav.fill(now),
            pictures: &self.pictures,
            missing: &missing,
            pose: &pose,
            flash,
        };
        sync(&self.slot_rows, scene.slots());
        let (pointed, mut since_change) = self.middle_at.get();
        if pointed != nav.highlight() {
            since_change = now;
            self.middle_at.set((nav.highlight(), now));
        }
        let mut middle = MiddleInput {
            wheel: w,
            tree,
            level,
            diameter: size,
            highlight: nav.highlight(),
            current,
            pictures: &self.pictures,
        }
        .view();
        middle.fade = if w.animations && w.crossfade {
            anim::fade(since(since_change), FADE_MS * f32::from(speed) / 100.0)
        } else {
            1.0
        };
        let empty = nav.is_empty();
        if let Some(progress) = nav.middle(now) {
            let back = nav.depth() > 0;
            middle.mode = if empty {
                3
            } else if back {
                1
            } else {
                2
            };
            if (back || empty) && progress > 0.0 {
                middle.arc = geometry::arc(c, inner - 3.0, 0.0, progress * TAU).into();
            }
        } else if empty {
            // Without favorites the first level is empty: the middle is a jump to the settings (the user's wish).
            middle.mode = 3;
        }
        ui.set_middle(middle);
        ui.set_breadcrumb(
            if w.breadcrumb && nav.depth() > 0 {
                breadcrumb(tree, &nav)
            } else {
                String::new()
            }
            .into(),
        );
    }
}

/// Milliseconds as `f32` (animations are short).
fn to_ms(ms: u64) -> f32 {
    f32::from(u16::try_from(ms.min(60_000)).unwrap_or(u16::MAX))
}

/// "Start › Clients › More ..." for the opened levels.
fn breadcrumb(tree: &FolderTree, nav: &WheelNav) -> String {
    std::iter::once("Start".to_owned())
        .chain(nav.path().iter().skip(1).map(|level| match &level.key {
            rustcut_core::folders::LevelKey::Group(id) => tree.find(*id).map(|n| n.name.clone()).unwrap_or_default(),
            _ => "More ...".to_owned(),
        }))
        .collect::<Vec<_>>()
        .join(" › ")
}

thread_local! {
    /// The bridge, for its timer.
    static WHEEL: RefCell<Weak<WheelBridge>> = const { RefCell::new(Weak::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_of_the_wheel() {
        let again = vec!["F".to_owned()];
        assert_eq!(key_action("f", &again), Some(KeyAction::Again));
        assert_eq!(key_action("A", &again), Some(KeyAction::Left));
        assert_eq!(key_action("d", &again), Some(KeyAction::Right));
        assert_eq!(key_action("w", &again), Some(KeyAction::Open));
        assert_eq!(key_action("s", &again), Some(KeyAction::Back));
        let enter = slint::SharedString::from(slint::platform::Key::Return).to_string();
        assert_eq!(key_action(&enter, &again), Some(KeyAction::Enter));
        assert_eq!(key_action("x", &again), None);
    }
}
