//! When the job card shows (Q-P12h): after the tooltip delay while the pointer rests on a row; once it is open, moving
//! to the next row shows that job at once (no new delay), and a short grace keeps it open between two rows. After it
//! closed, a row entered within the warm-up shows at once too, like the tooltips. A press closes it, and the next one
//! waits again. Pure: the bridge passes the time and runs one timer for the [`Step::wake`] it is told.

use std::time::{Duration, Instant};

use rustcut_core::job::JobId;

/// Between leaving one row and entering the next, the card stays.
pub const GRACE: Duration = Duration::from_millis(120);
/// After the card closed, another row shows at once within this time (`TooltipSettings.warm-up`).
pub const WARM: Duration = Duration::from_millis(700);

#[derive(Debug, Default)]
pub struct Hover {
    /// The row under the pointer.
    pointer: Option<JobId>,
    /// The job the card shows.
    shown: Option<JobId>,
    /// When the pointer came to `pointer` (the delay runs from there).
    since: Option<Instant>,
    /// When the pointer left the shown row (the grace runs from there).
    left: Option<Instant>,
    warm_until: Option<Instant>,
}

/// What the bridge does next: show or hide the card when `changed`, and call [`Hover::due`] after `wake`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub shown: Option<JobId>,
    pub changed: bool,
    pub wake: Option<Duration>,
}

impl Hover {
    pub fn shown(&self) -> Option<JobId> {
        self.shown
    }

    pub fn enter(&mut self, id: JobId, now: Instant, delay: Duration) -> Step {
        self.pointer = Some(id);
        self.since = Some(now);
        self.left = None;
        let warm = self.warm_until.is_some_and(|until| now < until);
        if self.shown.is_some() || warm || delay.is_zero() {
            return self.show(id);
        }
        self.step(false, Some(delay))
    }

    pub fn leave(&mut self, id: JobId, now: Instant) -> Step {
        if self.pointer != Some(id) {
            return self.step(false, None);
        }
        self.pointer = None;
        self.since = None;
        if self.shown.is_none() {
            return self.step(false, None);
        }
        self.left = Some(now);
        self.step(false, Some(GRACE))
    }

    /// A press on a row, a drag, a menu: the card closes and the next one waits for the delay.
    pub fn press(&mut self) -> Step {
        let changed = self.shown.is_some();
        *self = Self::default();
        self.step(changed, None)
    }

    /// The timer the last step asked for ran out.
    pub fn due(&mut self, now: Instant, delay: Duration) -> Step {
        if let (Some(id), Some(since)) = (self.pointer, self.since) {
            if self.shown == Some(id) {
                return self.step(false, None);
            }
            let waited = now.saturating_duration_since(since);
            return if waited >= delay {
                self.show(id)
            } else {
                self.step(false, Some(delay.saturating_sub(waited)))
            };
        }
        match self.left {
            Some(left) if self.shown.is_some() => {
                let waited = now.saturating_duration_since(left);
                if waited < GRACE {
                    return self.step(false, Some(GRACE.saturating_sub(waited)));
                }
                self.shown = None;
                self.left = None;
                self.warm_until = Some(now + WARM);
                self.step(true, None)
            }
            _ => self.step(false, None),
        }
    }

    /// The shown job went away (removed, filtered out).
    pub fn forget(&mut self, id: JobId) -> Step {
        if self.pointer == Some(id) {
            self.pointer = None;
            self.since = None;
        }
        if self.shown == Some(id) {
            self.shown = None;
            self.left = None;
            return self.step(true, None);
        }
        self.step(false, None)
    }

    fn show(&mut self, id: JobId) -> Step {
        let changed = self.shown != Some(id);
        self.shown = Some(id);
        self.warm_until = None;
        self.step(changed, None)
    }

    fn step(&self, changed: bool, wake: Option<Duration>) -> Step {
        Step {
            shown: self.shown,
            changed,
            wake,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY: Duration = Duration::from_millis(600);

    fn ms(start: Instant, n: u64) -> Instant {
        start + Duration::from_millis(n)
    }

    #[test]
    fn the_card_waits_for_the_delay_then_moves_on_at_once() {
        let t = Instant::now();
        let mut h = Hover::default();
        let s = h.enter(JobId(1), t, DELAY);
        assert_eq!((s.shown, s.wake), (None, Some(DELAY)));
        assert_eq!(
            h.due(ms(t, 300), DELAY).wake,
            Some(Duration::from_millis(300)),
            "not yet"
        );
        let s = h.due(ms(t, 600), DELAY);
        assert_eq!((s.shown, s.changed), (Some(JobId(1)), true));
        // To the next row: leave, then enter within the grace; the card switches at once.
        assert_eq!(h.leave(JobId(1), ms(t, 900)).wake, Some(GRACE));
        let s = h.enter(JobId(2), ms(t, 905), DELAY);
        assert_eq!((s.shown, s.changed), (Some(JobId(2)), true));
        assert!(
            !h.due(ms(t, 1_020), DELAY).changed,
            "the grace timer finds a row under the pointer"
        );
    }

    #[test]
    fn leaving_closes_after_the_grace_and_stays_warm() {
        let t = Instant::now();
        let mut h = Hover::default();
        h.enter(JobId(1), t, DELAY);
        h.due(ms(t, 600), DELAY);
        h.leave(JobId(1), ms(t, 700));
        let s = h.due(ms(t, 820), DELAY);
        assert_eq!((s.shown, s.changed), (None, true));
        let s = h.enter(JobId(3), ms(t, 1_000), DELAY);
        assert_eq!(s.shown, Some(JobId(3)), "warm: at once");
        h.leave(JobId(3), ms(t, 1_100));
        h.due(ms(t, 1_300), DELAY);
        let s = h.enter(JobId(4), ms(t, 2_100), DELAY);
        assert_eq!((s.shown, s.wake), (None, Some(DELAY)), "cold again after the warm-up");
    }

    #[test]
    fn a_press_closes_cold_and_a_gone_job_closes() {
        let t = Instant::now();
        let mut h = Hover::default();
        h.enter(JobId(1), t, Duration::ZERO);
        assert_eq!(h.shown(), Some(JobId(1)), "no delay set");
        assert!(h.press().changed);
        assert_eq!(h.enter(JobId(1), ms(t, 10), DELAY).shown, None, "the next one waits");
        h.due(ms(t, 610), DELAY);
        assert!(h.forget(JobId(1)).changed);
        assert_eq!(h.shown(), None);
        assert!(!h.leave(JobId(9), ms(t, 700)).changed, "another row's leave");
    }
}
