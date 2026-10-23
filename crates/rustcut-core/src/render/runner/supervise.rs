//! What the runner does to a running ffmpeg, decided without touching it (pure, tested with made-up clocks): pause and
//! resume the process group, the cancel ladder `q` -> SIGTERM -> SIGKILL, and the stall watchdog.

use std::time::Duration;

use tokio::time::Instant;

use crate::render::ProgressSample;

/// How long ffmpeg may take to finish the file after `q` before it gets SIGTERM.
pub const QUIT_GRACE: Duration = Duration::from_secs(5);
/// How long it may take after SIGTERM before it gets SIGKILL.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(3);

/// What the owner of a job wants from its render (a `watch` channel value: the latest wish counts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Control {
    #[default]
    Run,
    /// Stop the process group (SIGSTOP); it uses no CPU until resumed.
    Pause,
    /// End the render; the temp file is deleted, an existing target stays.
    Cancel,
}

/// A signal for ffmpeg's process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSignal {
    Stop,
    Continue,
    Terminate,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Signal(GroupSignal),
    /// Write `q` to ffmpeg's stdin: it stops reading, finishes the file and exits.
    Quit,
}

/// Why the runner is ending the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Canceled,
    /// No progress for the stall timeout.
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Quit,
    Terminate,
    Kill,
}

#[derive(Debug, Clone, Copy)]
struct Stopping {
    reason: StopReason,
    step: Step,
    since: Instant,
}

/// The supervision state of one ffmpeg run.
#[derive(Debug, Clone)]
pub struct Supervisor {
    paused: bool,
    stopping: Option<Stopping>,
    stall_timeout: Option<Duration>,
    last_advance: Instant,
    last_sample: ProgressSample,
}

impl Supervisor {
    /// A freshly started process; `stall_timeout` = `None` turns the watchdog off.
    pub fn new(stall_timeout: Option<Duration>, now: Instant) -> Self {
        Self {
            paused: false,
            stopping: None,
            stall_timeout: stall_timeout.filter(|t| !t.is_zero()),
            last_advance: now,
            last_sample: ProgressSample::default(),
        }
    }

    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn stop_reason(&self) -> Option<StopReason> {
        self.stopping.map(|s| s.reason)
    }

    /// The owner's wish changed (or is read for the first time).
    pub fn control(&mut self, wanted: Control, now: Instant) -> Vec<Action> {
        if self.stopping.is_some() {
            return Vec::new();
        }
        match wanted {
            Control::Cancel => self.begin_stop(StopReason::Canceled, now),
            Control::Pause if !self.paused => {
                self.paused = true;
                vec![Action::Signal(GroupSignal::Stop)]
            }
            Control::Run if self.paused => {
                self.paused = false;
                // The time spent paused is no stall.
                self.last_advance = now;
                vec![Action::Signal(GroupSignal::Continue)]
            }
            Control::Pause | Control::Run => Vec::new(),
        }
    }

    /// A progress block arrived.
    pub fn progress(&mut self, sample: &ProgressSample, now: Instant) {
        if sample.advanced_since(&self.last_sample) {
            self.last_advance = now;
        }
        self.last_sample = *sample;
    }

    /// Time passed: the next step of a stop, or the watchdog.
    pub fn tick(&mut self, now: Instant) -> Vec<Action> {
        if let Some(stopping) = self.stopping.as_mut() {
            let due = match stopping.step {
                Step::Quit => now.saturating_duration_since(stopping.since) >= QUIT_GRACE,
                Step::Terminate => now.saturating_duration_since(stopping.since) >= TERMINATE_GRACE,
                Step::Kill => false,
            };
            if !due {
                return Vec::new();
            }
            let (step, signal) = match stopping.step {
                Step::Quit => (Step::Terminate, GroupSignal::Terminate),
                Step::Terminate | Step::Kill => (Step::Kill, GroupSignal::Kill),
            };
            *stopping = Stopping {
                step,
                since: now,
                ..*stopping
            };
            return vec![Action::Signal(signal)];
        }
        match self.stall_timeout {
            Some(timeout) if !self.paused && now.saturating_duration_since(self.last_advance) >= timeout => {
                self.begin_stop(StopReason::Stalled, now)
            }
            _ => Vec::new(),
        }
    }

    /// When [`Supervisor::tick`] has something to do next; `None` = only an event can change anything.
    pub fn deadline(&self) -> Option<Instant> {
        match self.stopping {
            Some(Stopping {
                step: Step::Quit,
                since,
                ..
            }) => since.checked_add(QUIT_GRACE),
            Some(Stopping {
                step: Step::Terminate,
                since,
                ..
            }) => since.checked_add(TERMINATE_GRACE),
            Some(Stopping { step: Step::Kill, .. }) => None,
            None if self.paused => None,
            None => self.stall_timeout.and_then(|t| self.last_advance.checked_add(t)),
        }
    }

    fn begin_stop(&mut self, reason: StopReason, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.paused {
            // A stopped process reads no `q` and handles SIGTERM only once it runs again.
            actions.push(Action::Signal(GroupSignal::Continue));
            self.paused = false;
        }
        actions.push(Action::Quit);
        self.stopping = Some(Stopping {
            reason,
            step: Step::Quit,
            since: now,
        });
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Timestamp;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    fn sample(frame: u64) -> ProgressSample {
        ProgressSample {
            frame: Some(frame),
            out_time: Some(Timestamp::from_millis(i64::try_from(frame).unwrap() * 40)),
            ..ProgressSample::default()
        }
    }

    #[test]
    fn pause_and_resume_signal_the_group_once() {
        let t0 = Instant::now();
        let mut s = Supervisor::new(None, t0);
        assert_eq!(s.control(Control::Run, t0), []);
        assert_eq!(s.control(Control::Pause, t0), [Action::Signal(GroupSignal::Stop)]);
        assert_eq!(s.control(Control::Pause, t0), [], "already paused");
        assert!(s.is_paused());
        assert_eq!(s.control(Control::Run, t0), [Action::Signal(GroupSignal::Continue)]);
        assert!(!s.is_paused());
        assert_eq!(s.deadline(), None, "no watchdog, nothing to wait for");
    }

    #[test]
    fn cancel_climbs_the_ladder() {
        let t0 = Instant::now();
        let mut s = Supervisor::new(Some(Duration::from_secs(60)), t0);
        assert_eq!(s.control(Control::Cancel, t0), [Action::Quit]);
        assert_eq!(s.stop_reason(), Some(StopReason::Canceled));
        assert_eq!(s.control(Control::Run, t0), [], "a stop is final");
        assert_eq!(s.deadline(), Some(t0 + QUIT_GRACE));
        assert_eq!(s.tick(at(t0, 4)), []);
        let t1 = t0 + QUIT_GRACE;
        assert_eq!(s.tick(t1), [Action::Signal(GroupSignal::Terminate)]);
        assert_eq!(s.deadline(), Some(t1 + TERMINATE_GRACE));
        assert_eq!(s.tick(t1 + TERMINATE_GRACE), [Action::Signal(GroupSignal::Kill)]);
        assert_eq!(s.deadline(), None);
        assert_eq!(s.tick(at(t0, 100)), []);
    }

    #[test]
    fn cancel_while_paused_continues_first() {
        let t0 = Instant::now();
        let mut s = Supervisor::new(None, t0);
        s.control(Control::Pause, t0);
        assert_eq!(
            s.control(Control::Cancel, t0),
            [Action::Signal(GroupSignal::Continue), Action::Quit]
        );
        assert!(!s.is_paused());
    }

    #[test]
    fn the_watchdog_ends_a_stalled_render_but_not_a_paused_one() {
        let t0 = Instant::now();
        let timeout = Duration::from_secs(30);
        let mut s = Supervisor::new(Some(timeout), t0);
        s.progress(&sample(10), at(t0, 20));
        assert_eq!(s.deadline(), Some(at(t0, 50)));
        // The same numbers again are no progress.
        s.progress(&sample(10), at(t0, 40));
        assert_eq!(s.tick(at(t0, 49)), []);
        // Paused at 45 for a long time: no stall; resuming restarts the clock.
        s.control(Control::Pause, at(t0, 45));
        assert_eq!(s.deadline(), None);
        assert_eq!(s.tick(at(t0, 500)), []);
        s.control(Control::Run, at(t0, 500));
        assert_eq!(s.deadline(), Some(at(t0, 530)));
        assert_eq!(s.tick(at(t0, 530)), [Action::Quit]);
        assert_eq!(s.stop_reason(), Some(StopReason::Stalled));
    }

    #[test]
    fn a_zero_timeout_means_no_watchdog() {
        let t0 = Instant::now();
        let mut s = Supervisor::new(Some(Duration::ZERO), t0);
        assert_eq!(s.deadline(), None);
        assert_eq!(s.tick(at(t0, 10_000)), []);
    }
}
