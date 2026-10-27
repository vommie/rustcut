//! What happens when the queue is done and when RustCut is closed while it renders (pure; user decisions Q-P1, Q-P2).

use rustcut_core::settings::{TaskbarShows, WhenDone};

use super::super::queue::QueueStatus;
use crate::desktop::taskbar::Badge;
use crate::icons::Icon;
use crate::ui::menu::MenuTint;

/// The choices of the queue panel's "when done" button, in its order.
pub const CHOICES: [WhenDone; 5] = [
    WhenDone::Nothing,
    WhenDone::Notify,
    WhenDone::Quit,
    WhenDone::Suspend,
    WhenDone::PowerOff,
];

/// The entry of the "When the queue finishes…" menu (P9.6, the UI review's note 19).
pub const fn label(choice: WhenDone) -> &'static str {
    match choice {
        WhenDone::Nothing => "Do Nothing",
        WhenDone::Notify => "Send Notification",
        WhenDone::Quit => "Quit App",
        WhenDone::Suspend => "Suspend Device",
        WhenDone::PowerOff => "Power Off Device",
    }
}

/// The entry's icon, colored by how far it goes (neutral, green, yellow, orange, red); the button's dot has the color.
pub const fn look(choice: WhenDone) -> (Icon, MenuTint) {
    match choice {
        WhenDone::Nothing => (Icon::WhenDoneNothing, MenuTint::Plain),
        WhenDone::Notify => (Icon::WhenDoneNotify, MenuTint::Green),
        WhenDone::Quit => (Icon::WhenDoneQuit, MenuTint::Yellow),
        WhenDone::Suspend => (Icon::WhenDoneSuspend, MenuTint::Orange),
        WhenDone::PowerOff => (Icon::WhenDonePowerOff, MenuTint::Red),
    }
}

pub fn index(choice: WhenDone) -> usize {
    CHOICES.iter().position(|c| *c == choice).unwrap_or(0)
}

/// The countdown before quit, suspend or power off; its message has an Abort button, Escape aborts it too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Countdown {
    pub action: WhenDone,
    /// Seconds left.
    pub left: u32,
}

/// What one second of the countdown does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Wait,
    Fire(WhenDone),
}

impl Countdown {
    /// The countdown for `action` (`None` for the choices that need none).
    pub fn start(action: WhenDone, seconds: u32) -> Option<Self> {
        matches!(action, WhenDone::Quit | WhenDone::Suspend | WhenDone::PowerOff)
            .then_some(Self { action, left: seconds })
    }

    /// One second passed (also right at the start, so 0 seconds fires at once).
    pub fn tick(&mut self) -> Tick {
        if self.left == 0 {
            return Tick::Fire(self.action);
        }
        self.left = self.left.saturating_sub(1);
        Tick::Wait
    }

    /// "Suspending" (the message's title: it stays while the seconds run).
    pub const fn verb(&self) -> &'static str {
        match self.action {
            WhenDone::Quit => "Quitting RustCut",
            WhenDone::Suspend => "Suspending",
            WhenDone::PowerOff => "Powering off",
            WhenDone::Nothing | WhenDone::Notify => "Done",
        }
    }

    /// "The queue is done. Suspending in 5 s".
    pub fn text(&self) -> String {
        format!("The queue is done. {} in {} s", self.verb(), self.left)
    }
}

/// "3 jobs finished, 1 failed."
pub fn done_text(finished: u32, failed: u32) -> String {
    let jobs = |n: u32| {
        if n == 1 {
            "1 job".to_owned()
        } else {
            format!("{n} jobs")
        }
    };
    match (finished, failed) {
        (_, 0) => format!("{} finished.", jobs(finished)),
        (0, _) => format!("{} failed.", jobs(failed)),
        _ => format!("{} finished, {failed} failed.", jobs(finished)),
    }
}

/// Closing RustCut asks first while a job renders and `general.confirm_quit_while_rendering` is on.
pub const fn ask_before_quit(status: &QueueStatus, setting: bool) -> bool {
    setting && status.rendering > 0
}

/// The close question's text.
pub fn ask_text(status: &QueueStatus) -> String {
    let rendering = if status.rendering == 1 {
        "A job is rendering".to_owned()
    } else {
        format!("{} jobs are rendering", status.rendering)
    };
    let waiting = status.open.saturating_sub(status.rendering);
    let more = match waiting {
        0 => String::new(),
        1 => ", 1 more waits".to_owned(),
        n => format!(", {n} more wait"),
    };
    // Review 55 (P9.3): what is going on and the question; "Force quit" explains itself.
    format!("{rendering}{more}. What to do?")
}

/// The taskbar entry: the current job's or the run's progress (`shows`) in whole percent and the open jobs, or nothing
/// while the queue is idle.
pub fn badge(status: &QueueStatus, shows: TaskbarShows) -> Badge {
    if status.open == 0 {
        return Badge::NONE;
    }
    let fraction = match shows {
        TaskbarShows::CurrentJob => status.current,
        TaskbarShows::AllJobs => status.overall.map(|o| o.fraction),
    };
    Badge {
        progress: fraction.map(|f| (f * 100.0).round() / 100.0),
        count: u32::try_from(status.open).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::queue::Overall;

    fn status(rendering: usize, open: usize, fraction: Option<f64>) -> QueueStatus {
        QueueStatus {
            rendering,
            active: rendering,
            processing_paused: false,
            open,
            held: 0,
            overall: fraction.map(|fraction| Overall {
                fraction,
                done: 0,
                total: open,
                eta_secs: None,
                partial: false,
            }),
            current: None,
        }
    }

    #[test]
    fn the_countdown_fires_after_its_seconds_and_names_the_action() {
        assert_eq!(Countdown::start(WhenDone::Notify, 5), None);
        let mut c = Countdown::start(WhenDone::Suspend, 2).unwrap();
        assert_eq!(c.text(), "The queue is done. Suspending in 2 s");
        assert_eq!(
            (c.tick(), c.tick(), c.tick()),
            (Tick::Wait, Tick::Wait, Tick::Fire(WhenDone::Suspend))
        );
        let mut now = Countdown::start(WhenDone::PowerOff, 0).unwrap();
        assert_eq!(now.tick(), Tick::Fire(WhenDone::PowerOff));
        assert_eq!(CHOICES.map(index), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn texts_for_the_end_and_the_close_question() {
        assert_eq!(done_text(3, 0), "3 jobs finished.");
        assert_eq!(done_text(1, 1), "1 job finished, 1 failed.");
        assert_eq!(done_text(0, 2), "2 jobs failed.");
        let busy = status(1, 3, Some(0.426));
        assert!(ask_before_quit(&busy, true));
        assert!(!ask_before_quit(&busy, false));
        assert!(
            !ask_before_quit(&status(0, 2, None), true),
            "waiting jobs alone do not ask"
        );
        assert_eq!(ask_text(&busy), "A job is rendering, 2 more wait. What to do?");
        assert_eq!(
            badge(&busy, TaskbarShows::AllJobs),
            Badge {
                progress: Some(0.43),
                count: Some(3)
            }
        );
        let current = QueueStatus {
            current: Some(0.814),
            ..busy
        };
        assert_eq!(
            badge(&current, TaskbarShows::CurrentJob),
            Badge {
                progress: Some(0.81),
                count: Some(3)
            },
            "the current job's progress"
        );
        assert_eq!(badge(&status(0, 0, Some(1.0)), TaskbarShows::CurrentJob), Badge::NONE);
    }
}
