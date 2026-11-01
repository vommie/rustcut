//! The folder wheel's animations without Slint (P12 3.9): where the whole wheel and each spoke stand at a moment of a
//! running animation. The wheel works while it moves: the navigation never waits for these.

use std::f32::consts::PI;

use rustcut_core::settings::{WheelLevels, WheelOpening};

/// One spoke's or the whole wheel's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub alpha: f32,
    pub zoom: f32,
    /// Degrees, clockwise.
    pub turn: f32,
}

impl Pose {
    pub const REST: Self = Self {
        alpha: 1.0,
        zoom: 1.0,
        turn: 0.0,
    };
}

/// What runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    Open(WheelOpening),
    Level {
        kind: WheelLevels,
        back: bool,
        /// The place the level opened from.
        place: Option<usize>,
    },
}

/// A place's time after the one before it, when the spokes come one after the other.
const STAGGER: f32 = 28.0;
const SPOKE: f32 = 150.0;
/// How visible a new level starts: the old one is gone at once, and from nothing the video would flash through.
const LEVEL_ALPHA: f32 = 0.45;

fn eased(t: f32) -> f32 {
    let rest = 1.0 - t.clamp(0.0, 1.0);
    1.0 - rest * rest * rest
}

fn count(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    (to - from).mul_add(t, from)
}

fn came(from: Pose, t: f32) -> Pose {
    Pose {
        alpha: lerp(from.alpha, 1.0, t),
        zoom: lerp(from.zoom, 1.0, t),
        turn: lerp(from.turn, 0.0, t),
    }
}

impl Run {
    /// How long it runs at normal speed (milliseconds) with `n` places.
    fn length(self, n: usize) -> f32 {
        match self {
            Self::Open(WheelOpening::Off)
            | Self::Level {
                kind: WheelLevels::Off, ..
            } => 0.0,
            Self::Open(WheelOpening::Fade)
            | Self::Level {
                kind: WheelLevels::Zoom,
                ..
            } => 170.0,
            Self::Open(WheelOpening::Spokes) => STAGGER.mul_add(count(n.saturating_sub(1)), SPOKE),
            Self::Open(WheelOpening::Fan) => 280.0,
            Self::Open(WheelOpening::Spin)
            | Self::Level {
                kind: WheelLevels::Spoke,
                ..
            } => 240.0,
            Self::Level {
                kind: WheelLevels::Turn,
                ..
            } => 190.0,
        }
    }

    /// `elapsed` milliseconds at `speed` percent of the normal time are over.
    pub fn is_done(self, elapsed: f32, speed: u16, n: usize) -> bool {
        elapsed >= self.length(n) * f32::from(speed) / 100.0
    }

    /// The whole wheel.
    pub fn whole(self, elapsed: f32, speed: u16, n: usize) -> Pose {
        let total = (self.length(n) * f32::from(speed) / 100.0).max(1.0);
        let t = eased(elapsed / total);
        let from = match self {
            Self::Open(WheelOpening::Fade) => Pose {
                alpha: 0.0,
                zoom: 0.82,
                turn: 0.0,
            },
            Self::Open(WheelOpening::Spin) => Pose {
                alpha: 0.0,
                zoom: 0.9,
                turn: -50.0,
            },
            // Going back the level comes from outside: the way out of the level left.
            Self::Level {
                kind: WheelLevels::Zoom,
                back,
                ..
            }
            | Self::Level {
                kind: WheelLevels::Spoke,
                back: back @ true,
                ..
            } => Pose {
                alpha: LEVEL_ALPHA,
                zoom: if back { 1.22 } else { 0.72 },
                turn: 0.0,
            },
            Self::Level {
                kind: WheelLevels::Turn,
                back,
                ..
            } => Pose {
                alpha: LEVEL_ALPHA,
                zoom: 1.0,
                turn: if back { 60.0 } else { -60.0 },
            },
            _ => return Pose::REST,
        };
        came(from, t)
    }

    /// Spoke `index` of `n` inside the whole wheel.
    pub fn spoke(self, elapsed: f32, speed: u16, index: usize, n: usize) -> Pose {
        let scale = f32::from(speed) / 100.0;
        let step = 360.0 / count(n.max(1));
        match self {
            Self::Open(WheelOpening::Spokes) => {
                let t = (elapsed - STAGGER * scale * count(index)) / (SPOKE * scale).max(1.0);
                came(
                    Pose {
                        alpha: 0.0,
                        zoom: 0.7,
                        turn: 0.0,
                    },
                    eased(t),
                )
            }
            Self::Open(WheelOpening::Fan) => {
                let t = elapsed / (280.0 * scale).max(1.0);
                came(
                    Pose {
                        alpha: 0.0,
                        zoom: 1.0,
                        turn: -step * count(index),
                    },
                    eased(t),
                )
            }
            Self::Level {
                kind: WheelLevels::Spoke,
                back: false,
                place: Some(place),
            } => {
                // Every spoke comes from the angle the chosen one stood at on the level before (its place count is
                // not kept: eight is the usual case and only the direction matters).
                let t = elapsed / (240.0 * scale).max(1.0);
                let from = step.mul_add(-count(index), count(place) * 45.0);
                came(
                    Pose {
                        alpha: LEVEL_ALPHA,
                        zoom: 0.6,
                        turn: from,
                    },
                    eased(t),
                )
            }
            _ => Pose::REST,
        }
    }
}

/// The one flash of the current folder at the opening: 0 at rest, up to 1 and back, over `length` milliseconds.
pub fn pulse(elapsed: f32, length: f32) -> f32 {
    if elapsed <= 0.0 || elapsed >= length {
        return 0.0;
    }
    (elapsed / length * PI).sin()
}

/// How far the middle faded over.
pub fn fade(elapsed: f32, length: f32) -> f32 {
    if length <= 0.0 {
        return 1.0;
    }
    eased(elapsed / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_animation_ends_at_rest() {
        let opens = [
            WheelOpening::Fade,
            WheelOpening::Spokes,
            WheelOpening::Fan,
            WheelOpening::Spin,
        ];
        let levels = [WheelLevels::Zoom, WheelLevels::Spoke, WheelLevels::Turn];
        let runs = opens
            .into_iter()
            .map(Run::Open)
            .chain(levels.into_iter().flat_map(|kind| {
                [true, false].map(|back| Run::Level {
                    kind,
                    back,
                    place: Some(2),
                })
            }));
        for run in runs {
            assert!(!run.is_done(0.0, 100, 8), "{run:?}");
            assert!(run.is_done(10_000.0, 100, 8));
            assert_eq!(run.whole(10_000.0, 100, 8), Pose::REST, "{run:?}");
            assert_eq!(run.spoke(10_000.0, 100, 7, 8), Pose::REST, "{run:?}");
        }
        assert!(Run::Open(WheelOpening::Off).is_done(0.0, 100, 8), "off: nothing runs");
    }

    #[test]
    fn spokes_come_one_after_the_other_and_the_speed_scales_the_time() {
        let run = Run::Open(WheelOpening::Spokes);
        assert!(run.spoke(100.0, 100, 0, 8).alpha > run.spoke(100.0, 100, 3, 8).alpha);
        assert!(
            run.spoke(100.0, 50, 3, 8).alpha > run.spoke(100.0, 100, 3, 8).alpha,
            "50 % is twice as fast"
        );
        assert!(run.is_done(180.0, 50, 8) && !run.is_done(180.0, 100, 8));
    }

    #[test]
    fn the_pulse_rises_and_falls() {
        assert!(pulse(0.0, 400.0).abs() < 0.001 && pulse(400.0, 400.0).abs() < 0.001);
        assert!(pulse(200.0, 400.0) > 0.99);
    }
}
