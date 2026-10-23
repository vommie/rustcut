//! J/K/L shuttle (NLE style, user decision 2026-10-23): L plays forward and doubles the speed on each press (up to 8x), J
//! plays backward the same way, K stops. mpv's own backward playback is documented as "extremely fragile", so reverse is
//! emulated: the player stays paused and jumps back to keyframes along a moving target; stopping seeks exactly to where
//! the target was.

use std::time::{Duration, Instant};

use az::CheckedAs as _;
use rustcut_core::time::Timestamp;

/// The fastest shuttle speed in either direction.
pub const MAX_RATE: i8 = 8;
/// How often the reverse target moves and a keyframe seek is sent.
pub const REVERSE_TICK: Duration = Duration::from_millis(100);

/// A shuttle key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuttleKey {
    Reverse,
    Stop,
    Forward,
}

/// The shuttle speed: negative = backward, 0 = stopped, positive = forward (1, 2, 4, 8).
pub fn next_rate(current: i8, key: ShuttleKey) -> i8 {
    match key {
        ShuttleKey::Stop => 0,
        ShuttleKey::Forward if current <= 0 => 1,
        ShuttleKey::Forward => current.saturating_mul(2).min(MAX_RATE),
        ShuttleKey::Reverse if current >= 0 => -1,
        ShuttleKey::Reverse => current.saturating_mul(2).max(-MAX_RATE),
    }
}

/// The current shuttle speed from what the player does: reversing at `reverse_rate` (0 = not reversing), playing at mpv's
/// speed, or stopped.
pub fn current_rate(reverse_rate: i8, playing: bool, speed: f64) -> i8 {
    if reverse_rate > 0 {
        return -reverse_rate.min(MAX_RATE);
    }
    if !playing {
        return 0;
    }
    // Speeds between the steps (another program set them) count as the step below.
    let mut rate = 1_i8;
    while rate < MAX_RATE && f64::from(rate) * 2.0 <= speed + 1e-6 {
        rate *= 2;
    }
    rate
}

/// The speed text next to the time: empty at normal playback and when stopped.
pub fn rate_text(rate: i8) -> String {
    match rate {
        0 | 1 => String::new(),
        r if r < 0 => format!("\u{25C0} {}\u{00D7}", -i16::from(r)),
        r => format!("{r}\u{00D7}"),
    }
}

/// An emulated backward playback: the target moves back from `from` at `rate` times real time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reverse {
    from: Timestamp,
    since: Instant,
    rate: i8,
}

impl Reverse {
    /// Starts at `from` with the speed `rate` (1-8, backward).
    pub fn new(from: Timestamp, rate: i8, now: Instant) -> Self {
        Self {
            from,
            since: now,
            rate: rate.clamp(1, MAX_RATE),
        }
    }

    pub fn rate(&self) -> i8 {
        self.rate
    }

    /// Where the target is at `now` (never before the beginning).
    pub fn target(&self, now: Instant) -> Timestamp {
        let elapsed = now.saturating_duration_since(self.since).as_micros();
        let back = elapsed
            .saturating_mul(u128::from(self.rate.unsigned_abs()))
            .checked_as::<i64>()
            .unwrap_or(i64::MAX);
        (self.from - Timestamp::from_micros(back)).max(Timestamp::ZERO)
    }

    /// The same target from `now` on, at another speed.
    pub fn with_rate(&self, rate: i8, now: Instant) -> Self {
        Self::new(self.target(now), rate, now)
    }

    /// The target reached the beginning: the shuttle stops there.
    pub fn at_start(&self, now: Instant) -> bool {
        self.target(now) == Timestamp::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_step_through_the_speeds() {
        let mut rate = 0;
        let mut seen = Vec::new();
        for key in [ShuttleKey::Forward; 5] {
            rate = next_rate(rate, key);
            seen.push(rate);
        }
        assert_eq!(seen, [1, 2, 4, 8, 8]);
        // J while playing forward turns around at 1x, then speeds up backward.
        assert_eq!(next_rate(4, ShuttleKey::Reverse), -1);
        assert_eq!(next_rate(-1, ShuttleKey::Reverse), -2);
        assert_eq!(next_rate(-8, ShuttleKey::Reverse), -8);
        assert_eq!(next_rate(-4, ShuttleKey::Forward), 1);
        assert_eq!(next_rate(-4, ShuttleKey::Stop), 0);
    }

    #[test]
    fn the_rate_follows_the_player() {
        let reverse = Reverse::new(Timestamp::from_secs(10), 2, Instant::now());
        assert_eq!(current_rate(reverse.rate(), false, 1.0), -2);
        assert_eq!(current_rate(0, false, 4.0), 0);
        assert_eq!(current_rate(0, true, 1.0), 1);
        assert_eq!(current_rate(0, true, 4.0), 4);
        assert_eq!(current_rate(0, true, 3.0), 2);
        assert_eq!(current_rate(0, true, 0.5), 1);
        assert_eq!(current_rate(0, true, 50.0), 8);
        assert_eq!(rate_text(1), "");
        assert_eq!(rate_text(0), "");
        assert_eq!(rate_text(4), "4\u{00D7}");
        assert_eq!(rate_text(-2), "\u{25C0} 2\u{00D7}");
    }

    #[test]
    fn the_reverse_target_moves_back_and_stops_at_zero() {
        let start = Instant::now();
        let reverse = Reverse::new(Timestamp::from_secs(10), 4, start);
        assert_eq!(reverse.target(start), Timestamp::from_secs(10));
        let later = start + Duration::from_millis(500);
        assert_eq!(reverse.target(later), Timestamp::from_secs(8));
        let faster = reverse.with_rate(8, later);
        assert_eq!(
            faster.target(later + Duration::from_millis(250)),
            Timestamp::from_secs(6)
        );
        assert!(!faster.at_start(later));
        assert!(faster.at_start(later + Duration::from_secs(2)));
        assert_eq!(faster.target(later + Duration::from_secs(60)), Timestamp::ZERO);
        // A clock that went backward does not move the target forward.
        assert_eq!(
            Reverse::new(Timestamp::from_secs(3), 1, later).target(start),
            Timestamp::from_secs(3)
        );
    }
}
