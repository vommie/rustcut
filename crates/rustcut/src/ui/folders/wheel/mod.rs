//! The folder wheel (P11.5, user decision Q-FP2): opened by holding the middle mouse button over the video (a ring fills
//! first) or by its key anywhere; always centered on the video. `nav.rs` is the navigation, `geometry.rs` the size and
//! the shapes; this bridge feeds them the pointer, the keys and the time, and draws the result into `FolderWheel`.
//! Picking a folder only sets the Job Finalization card's target (never saves).

pub mod geometry;
pub mod label;
pub mod nav;

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use rustcut_core::folders::{FolderTree, Layout, Slot};
use rustcut_core::settings::{ColorMode, PictureShape, Settings, WheelButton, WheelLabels, WheelMiddle};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use self::nav::{Context, KeyAction, Mode, Outcome, Radii, Timing, WheelNav};
use super::pictures::Pictures;
use super::view;
use crate::app::SettingsWriter;
use crate::ui::export::ExportBridge;
use crate::ui::keymap::{Action, Chord, Keymap};
use crate::{FolderPictureView, FolderWheel, MainWindow, Shortcuts, WheelSlotView};

/// How often a running hold or dwell is drawn again.
const TICK: Duration = Duration::from_millis(16);
/// The hold ring's square (`WheelHoldRing`).
const RING: f32 = 64.0;

fn timing(settings: &Settings) -> Timing {
    let w = &settings.folder_wheel;
    Timing {
        hold_ms: u64::from(w.hold_ms),
        dwell_ms: u64::from(w.dwell_ms),
        push: w.push,
    }
}

fn layout(settings: &Settings) -> Layout {
    Layout {
        places: settings.folder_wheel.places,
        divided: settings.folder_wheel.divided,
    }
}

const fn shape(shape: PictureShape) -> i32 {
    match shape {
        PictureShape::Round => 0,
        PictureShape::Rounded => 1,
        PictureShape::Square => 2,
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
}

impl WheelBridge {
    pub fn install(
        window: &MainWindow,
        writer: SettingsWriter,
        pictures: &Rc<Pictures>,
        export: &Rc<ExportBridge>,
    ) -> Rc<Self> {
        let settings = writer.current();
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            nav: RefCell::new(WheelNav::new(
                timing(&settings),
                Radii {
                    inner: 50.0,
                    outer: 150.0,
                },
            )),
            writer,
            pictures: Rc::clone(pictures),
            export: Rc::downgrade(export),
            started: Instant::now(),
            timer: slint::Timer::default(),
            held_at: RefCell::new((0.0, 0.0)),
        });
        let ui = window.global::<FolderWheel<'_>>();
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
        let (inner, outer) = geometry::radii(self.diameter(&settings));
        let outcome = {
            let mut nav = self.nav.borrow_mut();
            nav.set_timing(timing(&settings));
            nav.set_radii(Radii { inner, outer });
            let ctx = Context {
                tree: &settings.export.folders,
                layout: layout(&settings),
            };
            step(&mut nav, ctx, self.now())
        };
        match outcome {
            Outcome::Nothing => return,
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
            Outcome::Opened => tracing::debug!("[Folders] Folder wheel opened"),
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

    /// A timer draws a running hold or dwell (and lets it act) while one runs.
    fn keep_ticking(&self) {
        let needed = self.nav.borrow().needs_ticks();
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
        if !nav.is_open() {
            return;
        }
        let size = self.diameter(&settings);
        let (inner, outer) = geometry::radii(size);
        let c = size / 2.0;
        ui.set_size(size);
        ui.set_inner(inner);
        ui.set_outer(outer);
        let font = (size * 0.034).clamp(11.0, 22.0);
        ui.set_font_size(font);
        ui.set_dim(w.dim);
        ui.set_animations(w.animations);
        ui.set_labels(match w.labels {
            WheelLabels::PictureAndName => 0,
            WheelLabels::Picture => 1,
            WheelLabels::Name => 2,
        });
        ui.set_shape(shape(w.wheel_shape));
        ui.set_tint(w.color_mode == ColorMode::BackgroundAndIcons);
        ui.set_opacity(f32::from(w.opacity) / 100.0);
        let tree = &settings.export.folders;
        let Some(level) = nav.level() else { return };
        let n = level.slots.len();
        let fill = nav.fill(now);
        let step = TAU / f32::from(u16::try_from(n.max(1)).unwrap_or(u16::MAX));
        let mid = f32::midpoint(inner, outer);
        let chord = 2.0 * mid * (step / 2.0).sin();
        let box_w = (chord - 6.0).min((outer - inner) * 1.5).max(24.0);
        let box_h = (outer - inner) * 0.86;
        let slots: Vec<WheelSlotView> = level
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let (lx, ly) = geometry::at(c, c, mid, geometry::angle(i, n));
                let highlighted = nav.highlight() == Some(i);
                let entry = match slot {
                    Slot::Entry(id) => tree.entry(*id),
                    _ => None,
                };
                let color = entry.and_then(|e| view::color(e.color()));
                let tinted = color.is_some() && (w.color_mode != ColorMode::Pointed || highlighted);
                // The wheel's name (the short one when set), broken and cut to fit the field (the user's answers).
                let name = match slot {
                    Slot::More(ids) => format!("More ... ({})", ids.len()),
                    _ => entry
                        .map(rustcut_core::folders::EntryRef::wheel_name)
                        .unwrap_or_default()
                        .to_owned(),
                };
                let label = label::fit(&name, box_w, font);
                WheelSlotView {
                    segment: geometry::segment(c, inner, outer, i, n).into(),
                    fill: fill
                        .filter(|(f, _)| *f == i)
                        .map(|(_, p)| geometry::fill(c, inner, outer, i, n, p))
                        .unwrap_or_default()
                        .into(),
                    mark: if slot.opens(tree).is_some() {
                        geometry::group_mark(c, outer, i, n).into()
                    } else {
                        "".into()
                    },
                    x: lx - box_w / 2.0,
                    y: ly - box_h / 2.0,
                    w: box_w,
                    h: box_h,
                    name: label.text().into(),
                    scale: label.scale,
                    picture: entry.map_or_else(FolderPictureView::default, |e| view::picture(e, &self.pictures)),
                    filled: !matches!(slot, Slot::Empty),
                    highlighted,
                    tint: color.unwrap_or_default(),
                    tinted,
                    more: matches!(slot, Slot::More(_)),
                }
            })
            .collect();
        ui.set_slots(ModelRc::new(VecModel::from(slots)));
        let empty = nav.is_empty();
        if let Some(progress) = nav.middle(now) {
            let back = nav.depth() > 0;
            ui.set_middle_mode(if empty {
                3
            } else if back {
                1
            } else {
                2
            });
            ui.set_middle_arc(if (back || empty) && progress > 0.0 {
                geometry::arc(c, inner - 3.0, 0.0, progress * TAU).into()
            } else {
                "".into()
            });
        } else if empty {
            // Without favorites the first level is empty: the middle is a jump to the settings (the user's wish).
            ui.set_middle_mode(3);
            ui.set_middle_arc("".into());
        } else {
            ui.set_middle_mode(0);
            ui.set_middle_arc("".into());
            let pointed = nav.highlight().and_then(|i| level.slots.get(i));
            let entry = match pointed {
                Some(Slot::Entry(id)) => tree.entry(*id),
                _ => None,
            };
            let name = match pointed {
                Some(Slot::More(ids)) => format!("More ... ({})", ids.len()),
                _ => entry.map(|e| e.wheel_name().to_owned()).unwrap_or_default(),
            };
            // The middle's text is fitted like a field's (its width: three quarters of the middle).
            let middle = label::fit(&name, (inner - 4.0) * 2.0 * 0.76, font);
            let picture = entry.map(|e| view::picture(e, &self.pictures)).unwrap_or_default();
            // The picture (the default); an entry without one shows its first letter instead, like everywhere else
            // (the user's wish 2026-10-28). "More ..." has neither: its name.
            ui.set_middle_shows_picture(w.middle == WheelMiddle::Picture && entry.is_some());
            ui.set_middle_picture(picture);
            ui.set_middle_name(middle.text().into());
            ui.set_middle_scale(middle.scale);
        }
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
