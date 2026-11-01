//! The small wheel of Target Folders (P12 3.3): the real wheel, so it is also the live preview of the wheel's look and
//! animations. A click on a free place puts the selected entry there, a click on a taken one selects it; dragging a
//! place onto another moves it or swaps the two, and the swap shows before the drop. `preview.rs` plays its scene.

use rustcut_core::folders::{FolderId, Layout, LevelKey, Slot, level};
use rustcut_core::settings::Settings;
use slint::ComponentHandle as _;

use super::details;
use super::preview::SceneState;
use crate::FolderTreeUi;
use crate::ui::folders::wheel::anim::Pose;
use crate::ui::folders::wheel::draw::{self, MiddleInput, Scene};
use crate::ui::folders::wheel::{geometry, nav};
use crate::ui::settings::SettingsBridge;
use crate::util::model::sync;

/// The small wheel's diameter (`FolderTreeUi.wheel-size`).
pub(super) const DIAMETER: f32 = 240.0;
/// How far a press moves before it is a drag.
const DRAG_PX: f32 = 4.0;

/// A place being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct WheelDrag {
    from: usize,
    id: FolderId,
    press: (f32, f32),
    moved: bool,
    target: Option<usize>,
}

fn layout(settings: &Settings) -> Layout {
    Layout {
        places: settings.folder_wheel.places,
        divided: settings.folder_wheel.divided,
    }
}

impl SettingsBridge {
    /// The place under a point relative to the wheel's center.
    fn wheel_place(&self, x: f32, y: f32) -> Option<usize> {
        let settings = self.writer.current();
        let start = level(&settings.export.folders, &LevelKey::Start, layout(&settings))?;
        let (inner, outer) = geometry::radii(DIAMETER, settings.folder_wheel.middle_size);
        if x.hypot(y) > outer * 1.08 {
            return None;
        }
        nav::place_at(x, y, start.slots.len(), inner)
    }

    fn wheel_hover(&self, x: f32, y: f32) {
        let place = self.wheel_place(x, y);
        if self.folder_page.wheel_hover.replace(place) != place {
            self.publish_wheel(&self.writer.current());
        }
    }

    fn wheel_leave(&self) {
        if self.folder_page.wheel_hover.replace(None).is_some() {
            self.publish_wheel(&self.writer.current());
        }
    }

    fn wheel_press(&self, x: f32, y: f32) {
        let Some(from) = self.wheel_place(x, y) else { return };
        let settings = self.writer.current();
        let tree = &settings.export.folders;
        let count = settings.folder_wheel.places;
        let occupant = details::places(tree, None, count).get(from).and_then(|p| p.occupant);
        match occupant {
            Some(id) => self.folder_page.wheel_drag.set(Some(WheelDrag {
                from,
                id,
                press: (x, y),
                moved: false,
                target: None,
            })),
            None => self.folder_put_on_place(i32::try_from(from).unwrap_or(-1)),
        }
    }

    fn wheel_move(&self, x: f32, y: f32) {
        let Some(mut drag) = self.folder_page.wheel_drag.get() else {
            return;
        };
        if !drag.moved {
            if (x - drag.press.0).hypot(y - drag.press.1) < DRAG_PX {
                return;
            }
            drag.moved = true;
            let settings = self.writer.current();
            let picture = settings
                .export
                .folders
                .entry(drag.id)
                .map(|e| crate::ui::folders::view::picture(e, &self.pictures));
            let window = self.window.borrow();
            if let Some(window) = window.as_ref() {
                let ui = window.global::<FolderTreeUi<'_>>();
                ui.set_ghost_picture(picture.unwrap_or_default());
                ui.set_ghost(true);
                let me = std::rc::Weak::clone(&self.me);
                crate::ui::drag::begin(window, "", move || {
                    if let Some(b) = me.upgrade() {
                        b.wheel_end(false);
                    }
                });
            }
        }
        let target = self.wheel_place(x, y).filter(|t| *t != drag.from);
        let changed = drag.target != target;
        drag.target = target;
        self.folder_page.wheel_drag.set(Some(drag));
        if let Some(window) = self.window.borrow().as_ref() {
            let ui = window.global::<FolderTreeUi<'_>>();
            ui.set_ghost_x(x);
            ui.set_ghost_y(y);
        }
        if changed {
            self.publish_wheel(&self.writer.current());
        }
    }

    fn wheel_release(&self) {
        let Some(drag) = self.folder_page.wheel_drag.get() else {
            return;
        };
        if drag.moved {
            self.wheel_end(true);
        } else {
            // A click on a taken place selects its entry.
            self.folder_page.wheel_drag.set(None);
            self.folder_select_entry(drag.id);
        }
    }

    /// The drag ends: dropped on a place, or canceled (Escape; beside the wheel nothing happens either).
    fn wheel_end(&self, drop: bool) {
        let Some(drag) = self.folder_page.wheel_drag.take() else {
            return;
        };
        if let Some(window) = self.window.borrow().as_ref() {
            crate::ui::drag::end(window);
            window.global::<FolderTreeUi<'_>>().set_ghost(false);
        }
        if let (true, Some(target)) = (drop, drag.target)
            && let Ok(target) = u8::try_from(target)
        {
            self.update(|s| {
                let _ = s.export.folders.put_on_place(drag.id, target);
            });
            tracing::info!(
                id = drag.id.0,
                place = target,
                "[Settings] Wheel place changed by dragging"
            );
        }
        self.publish_wheel(&self.writer.current());
    }

    /// Draws the small wheel: a dragged place already stands where it would land.
    pub(super) fn publish_wheel(&self, settings: &Settings) {
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<FolderTreeUi<'_>>();
        let page = &self.folder_page;
        let tree = &settings.export.folders;
        let Some(mut start) = level(tree, &LevelKey::Start, layout(settings)) else {
            return;
        };
        let w = &settings.folder_wheel;
        let drag = page.wheel_drag.get().filter(|d| d.moved);
        if let Some((from, target)) = drag.and_then(|d| d.target.map(|t| (d.from, t))) {
            start.slots.swap(from, target);
        } else if let Some(slot) = drag.and_then(|d| start.slots.get_mut(d.from)) {
            *slot = Slot::Empty;
        }
        let drop_place = usize::try_from(ui.get_drop_place()).ok();
        let highlight = drag.and_then(|d| d.target).or(drop_place).or(page.wheel_hover.get());
        let n = start.slots.len();
        let scene_state: SceneState = page.scene.state(w, n);
        let pose = |i: usize| scene_state.spoke(w.animation_speed, i, n);
        let missing = |path: &std::path::Path| self.presence.is_missing(path);
        let current = page.selected.get();
        let mut slots = Scene {
            wheel: w,
            tree,
            level: &start,
            diameter: DIAMETER,
            scale: window.window().scale_factor(),
            highlight,
            current,
            fill: None,
            pictures: &self.pictures,
            missing: &missing,
            pose: &pose,
            flash: scene_state.flash,
        }
        .slots();
        // A free place shows its number.
        for (i, slot) in slots.iter_mut().enumerate() {
            if !slot.filled {
                slot.hint = (i + 1).to_string().into();
            }
        }
        sync(&page.wheel_rows, slots);
        let whole: Pose = scene_state.whole;
        ui.set_wheel_alpha(whole.alpha);
        ui.set_wheel_zoom(whole.zoom);
        ui.set_wheel_turn(whole.turn);
        ui.set_wheel_look(draw::look(w, DIAMETER, draw::font(DIAMETER)));
        let mut middle = MiddleInput {
            wheel: w,
            tree,
            level: &start,
            diameter: DIAMETER,
            highlight,
            current,
            pictures: &self.pictures,
        }
        .view();
        middle.resting = false;
        middle.fade = 1.0;
        ui.set_wheel_middle(middle);
        // The X of the place under the pointer: its favorite loses the star.
        let clear = page
            .wheel_hover
            .get()
            .filter(|_| drag.is_none())
            .filter(|i| matches!(start.slots.get(*i), Some(Slot::Entry(_))));
        ui.set_clear_shown(clear.is_some());
        if let Some(i) = clear {
            let (inner, outer) = geometry::radii(DIAMETER, w.middle_size);
            let (x, y) = geometry::at(
                DIAMETER / 2.0,
                DIAMETER / 2.0,
                f32::midpoint(f32::midpoint(inner, outer), outer),
                geometry::angle(i, n) + 0.28,
            );
            ui.set_clear_x(x);
            ui.set_clear_y(y);
            ui.set_clear_index(i32::try_from(i).unwrap_or(-1));
        }
    }
}

pub(super) fn connect(this: &std::rc::Weak<SettingsBridge>, ui: &FolderTreeUi<'_>) {
    let t = this.clone();
    ui.on_wheel_hover(move |x, y| {
        if let Some(b) = t.upgrade() {
            b.wheel_hover(x, y);
        }
    });
    let t = this.clone();
    ui.on_wheel_leave(move || {
        if let Some(b) = t.upgrade() {
            b.wheel_leave();
        }
    });
    let t = this.clone();
    ui.on_wheel_press(move |x, y| {
        if let Some(b) = t.upgrade() {
            b.wheel_press(x, y);
        }
    });
    let t = this.clone();
    ui.on_wheel_move(move |x, y| {
        if let Some(b) = t.upgrade() {
            b.wheel_move(x, y);
        }
    });
    let t = this.clone();
    ui.on_wheel_release(move || {
        if let Some(b) = t.upgrade() {
            b.wheel_release();
        }
    });
    let t = this.clone();
    ui.on_drop_place_changed(move || {
        if let Some(b) = t.upgrade() {
            b.publish_wheel(&b.writer.current());
        }
    });
    let t = this.clone();
    ui.on_play_again(move || {
        if let Some(b) = t.upgrade() {
            b.play_scene();
        }
    });
}
