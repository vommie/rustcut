//! The small wheel's scene (P12 3.9): it opens, goes into a level and back, so the animation settings can be seen. It
//! runs once after a change of the wheel's options, and over and over while the pointer rests on an animation's row.

use std::cell::Cell;
use std::rc::Weak;
use std::time::{Duration, Instant};

use rustcut_core::settings::{FolderWheelSettings, WheelLevels};

use crate::ui::folders::wheel::anim::{self, Pose, Run};
use crate::ui::settings::SettingsBridge;

/// How often the running scene is drawn.
const TICK: Duration = Duration::from_millis(16);
/// The pause between the scene's parts and before it repeats (ms).
const PAUSE: f32 = 450.0;
const PULSE_MS: f32 = 420.0;
/// The rows that are effects: the pointer on one repeats the scene.
const EFFECTS: [&str; 6] = [
    "folder_wheel.animations",
    "folder_wheel.opening",
    "folder_wheel.levels",
    "folder_wheel.animation_speed",
    "folder_wheel.pulse",
    "folder_wheel.crossfade",
];

/// What the scene shows at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SceneState {
    pub run: Option<(Run, f32)>,
    pub whole: Pose,
    pub flash: f32,
}

impl SceneState {
    const REST: Self = Self {
        run: None,
        whole: Pose::REST,
        flash: 0.0,
    };

    pub fn spoke(&self, speed: u16, index: usize, n: usize) -> Pose {
        self.run.map_or(Pose::REST, |(run, t)| run.spoke(t, speed, index, n))
    }
}

/// The scene's parts: which animation runs at `t` ms, with its own time, and when the scene is over.
fn part(w: &FolderWheelSettings, n: usize, t: f32) -> (Option<(Run, f32)>, bool) {
    let long = |run: Run| -> f32 {
        // How long a part takes, found from the animation itself in steps of 20 ms.
        let mut ms = 0.0;
        while !run.is_done(ms, w.animation_speed, n) && ms < 5_000.0 {
            ms += 20.0;
        }
        ms
    };
    let open = Run::Open(w.opening);
    let level = |back: bool| Run::Level {
        kind: w.levels,
        back,
        place: (!back).then_some(1),
    };
    let t1 = long(open);
    if t < t1 + PAUSE {
        return (Some((open, t)), false);
    }
    if w.levels == WheelLevels::Off {
        return (None, t >= t1 + 2.0 * PAUSE);
    }
    let t2 = t1 + PAUSE + long(level(false));
    if t < t2 + PAUSE {
        return (Some((level(false), t - t1 - PAUSE)), false);
    }
    let t3 = t2 + PAUSE + long(level(true));
    if t < t3 {
        return (Some((level(true), t - t2 - PAUSE)), false);
    }
    (None, t >= t3 + PAUSE)
}

/// The running scene.
pub(super) struct WheelScene {
    started: Cell<Option<Instant>>,
    repeat: Cell<bool>,
    timer: slint::Timer,
}

impl Default for WheelScene {
    fn default() -> Self {
        Self {
            started: Cell::new(None),
            repeat: Cell::new(false),
            timer: slint::Timer::default(),
        }
    }
}

impl WheelScene {
    pub fn state(&self, w: &FolderWheelSettings, n: usize) -> SceneState {
        let Some(started) = self.started.get().filter(|_| w.animations) else {
            return SceneState::REST;
        };
        let t = started.elapsed().as_secs_f32() * 1000.0;
        let (run, _) = part(w, n, t);
        let Some((run, local)) = run else {
            return SceneState::REST;
        };
        let flash = if w.pulse {
            anim::pulse(t, PULSE_MS * f32::from(w.animation_speed) / 100.0)
        } else {
            0.0
        };
        SceneState {
            run: Some((run, local)),
            whole: run.whole(local, w.animation_speed, n),
            flash,
        }
    }
}

impl SettingsBridge {
    /// Runs the scene from its start.
    pub(in crate::ui::settings) fn play_scene(&self) {
        let scene = &self.folder_page.scene;
        scene.started.set(Some(Instant::now()));
        if scene.timer.running() {
            return;
        }
        let weak = Weak::clone(&self.me);
        scene.timer.start(slint::TimerMode::Repeated, TICK, move || {
            if let Some(b) = weak.upgrade() {
                b.scene_tick();
            }
        });
    }

    fn scene_tick(&self) {
        let settings = self.writer.current();
        let w = &settings.folder_wheel;
        let scene = &self.folder_page.scene;
        let over = scene.started.get().is_none_or(|s| {
            let n = usize::from(w.places);
            part(w, n, s.elapsed().as_secs_f32() * 1000.0).1
        });
        if over {
            if scene.repeat.get() {
                scene.started.set(Some(Instant::now()));
            } else {
                scene.started.set(None);
                scene.timer.stop();
            }
        }
        self.publish_wheel(&settings);
    }

    /// The pointer came to or left a row of the Settings window.
    pub(in crate::ui::settings) fn scene_row_hover(&self, key: &str, on: bool) {
        if !EFFECTS.contains(&key) {
            return;
        }
        self.folder_page.scene.repeat.set(on);
        if on && self.folder_page.scene.started.get().is_none() {
            self.play_scene();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_runs_its_parts_and_ends() {
        let w = FolderWheelSettings::default();
        assert!(matches!(part(&w, 8, 10.0).0, Some((Run::Open(_), _))));
        assert!(matches!(
            part(&w, 8, 1_000.0).0,
            Some((Run::Level { back: false, .. }, _))
        ));
        assert!(part(&w, 8, 10_000.0).1, "it ends");
        let off = FolderWheelSettings {
            levels: WheelLevels::Off,
            ..w
        };
        assert!(part(&off, 8, 10_000.0).1);
    }
}
