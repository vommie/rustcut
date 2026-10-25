//! Moving through the video: the playhead, seeks (absolute from the timeline, relative from the arrow keys), frame
//! steps, play/pause and the J/K/L shuttle with its emulated backward playback. Every mpv call goes through the player's
//! command thread; the UI thread only records where a seek goes, so the playhead shows the target at once.

use std::rc::Rc;
use std::time::Instant;

use rustcut_core::time::Timestamp;
use rustcut_player::Player;
use slint::ComponentHandle as _;

use super::PlayerBridge;
use super::shuttle::{REVERSE_TICK, Reverse, ShuttleKey, current_rate, next_rate};
use crate::Playback;

impl PlayerBridge {
    /// The playhead: where a seek in flight goes, else mpv's position.
    pub fn position(&self) -> Timestamp {
        self.state.lock().playhead()
    }

    /// The file's duration as mpv reports it.
    pub fn duration(&self) -> Option<Timestamp> {
        self.state.lock().duration
    }

    /// A file is loaded and can be moved through.
    pub fn is_loaded(&self) -> bool {
        let state = self.state.lock();
        state.loaded && state.failure.is_none()
    }

    /// Jumps to `t`; `exact` decodes up to that frame, otherwise the nearest keyframe is enough (while dragging).
    pub fn seek_to(&self, t: Timestamp, exact: bool) {
        if !self.is_loaded() {
            return;
        }
        self.end_reverse();
        let t = t.max(Timestamp::ZERO);
        self.state.lock().seeking(t);
        self.command(|p| p.seek(t.as_secs_f64(), exact));
        self.refresh();
    }

    /// Jumps by `seconds` (negative = back) to the exact frame. The step starts at the playhead, which is the target of a
    /// seek still in flight: quick presses add up, and a mark set right after them lands where the steps went.
    pub fn seek_by(&self, seconds: f64) {
        let Some(delta) = Timestamp::from_secs_f64(seconds.abs()) else {
            return;
        };
        let (here, duration) = {
            let state = self.state.lock();
            (state.playhead(), state.duration)
        };
        let target = if seconds < 0.0 { here - delta } else { here + delta };
        self.seek_to(duration.map_or(target, |d| target.min(d)), true);
    }

    /// The next or previous frame (pauses).
    pub fn step_frame(&self, back: bool) {
        if !self.is_loaded() {
            return;
        }
        self.end_reverse();
        self.command(|p| p.frame_step(back));
    }

    /// Space: stops a backward shuttle, pauses a fast forward one (back to normal speed), else toggles play/pause (at
    /// the end of the file playing starts over).
    pub fn toggle_pause(&self) {
        if self.end_reverse() {
            return;
        }
        let (playing, speed) = {
            let state = self.state.lock();
            (state.playing(), state.speed)
        };
        if (speed - 1.0).abs() > 1e-6 {
            self.command(|p| p.set_speed(1.0));
            if playing {
                self.command(|p| p.set_paused(true));
                return;
            }
        }
        self.command(Player::toggle_pause);
    }

    /// Whether the video moves (plays forward or the backward shuttle runs).
    pub fn is_moving(&self) -> bool {
        self.reverse.get().is_some() || self.state.lock().playing()
    }

    /// A key played or paused the video (P9.9, user decision Q-P9.9: only keys): the play or pause symbol flashes when
    /// the setting is on.
    pub fn key_flash(&self, play: bool) {
        if !self.flash.get() || !self.is_loaded() {
            return;
        }
        if let Some(window) = self.window.upgrade() {
            let playback = window.global::<Playback<'_>>();
            playback.set_flash_play(play);
            playback.set_flash_serial(playback.get_flash_serial().wrapping_add(1));
        }
    }

    /// J, K or L; `Some(moving)` = whether the video moves afterwards, `None` when nothing is loaded.
    pub fn shuttle(self: &Rc<Self>, key: ShuttleKey) -> Option<bool> {
        if !self.is_loaded() {
            return None;
        }
        let now = Instant::now();
        let reverse = self.reverse.get();
        let (playing, speed) = {
            let state = self.state.lock();
            (state.playing(), state.speed)
        };
        let current = current_rate(reverse.map_or(0, |r| r.rate()), playing, speed);
        let next = next_rate(current, key);
        tracing::debug!(current, next, "[Player] Shuttle");
        match next {
            0 => {
                if !self.end_reverse() {
                    self.command(|p| p.set_paused(true));
                }
                self.command(|p| p.set_speed(1.0));
            }
            rate if rate > 0 => {
                self.end_reverse();
                self.command(|p| p.set_speed(f64::from(rate)));
                if current <= 0 {
                    // Toggling (not unpausing) starts over at the end of the file.
                    self.command(Player::toggle_pause);
                }
            }
            rate => {
                let rate = -rate;
                match reverse {
                    Some(r) => self.reverse.set(Some(r.with_rate(rate, now))),
                    None => self.start_reverse(rate, now),
                }
                self.state.lock().reverse_rate = rate;
                self.refresh();
            }
        }
        Some(next != 0)
    }

    fn start_reverse(self: &Rc<Self>, rate: i8, now: Instant) {
        self.command(|p| p.set_paused(true));
        self.command(|p| p.set_speed(1.0));
        let from = self.position();
        self.reverse.set(Some(Reverse::new(from, rate, now)));
        let this = Rc::downgrade(self);
        self.reverse_timer
            .start(slint::TimerMode::Repeated, REVERSE_TICK, move || {
                if let Some(bridge) = this.upgrade() {
                    bridge.reverse_tick();
                }
            });
        tracing::debug!(from = %from, rate, "[Player] Playing backward");
    }

    /// Moves the backward target and sends a keyframe seek there; at the beginning the shuttle stops.
    fn reverse_tick(&self) {
        let Some(reverse) = self.reverse.get() else {
            self.reverse_timer.stop();
            return;
        };
        let now = Instant::now();
        let target = reverse.target(now);
        self.state.lock().seeking(target);
        self.command(|p| p.seek(target.as_secs_f64(), false));
        if reverse.at_start(now) {
            self.end_reverse();
        } else {
            self.refresh();
        }
    }

    /// Stops the emulated backward playback with an exact seek to where its target was; `true` when it was running.
    pub(super) fn end_reverse(&self) -> bool {
        let Some(reverse) = self.reverse.take() else {
            return false;
        };
        self.reverse_timer.stop();
        let target = reverse.target(Instant::now());
        {
            let mut state = self.state.lock();
            state.reverse_rate = 0;
            state.seeking(target);
        }
        self.command(|p| p.seek(target.as_secs_f64(), true));
        self.refresh();
        true
    }
}
