//! The dialog about an unusable queue database (M6.5d, user decision Q-N10), without Slint: what is wrong, the ways
//! out with the one that fits preselected (newer -> temporary queue, damaged -> rescue, no access -> try again), the
//! button's words, and the line the queue panel shows while the queue is temporary.

use std::path::PathBuf;

use rustcut_core::store::Problem;

/// A way out (in the dialog's order; the key that picks it is its index + 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Rescue,
    StartEmpty,
    Temporary,
    Retry,
    Quit,
}

impl Choice {
    pub const ALL: [Self; 5] = [Self::Rescue, Self::StartEmpty, Self::Temporary, Self::Retry, Self::Quit];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Rescue => "Rescue the readable jobs",
            Self::StartEmpty => "Back up the file and start an empty queue",
            Self::Temporary => "Continue with a temporary queue",
            Self::Retry => "Try again",
            Self::Quit => "Quit RustCut",
        }
    }

    pub const fn confirm(self) -> &'static str {
        match self {
            Self::Rescue => "Rescue",
            Self::StartEmpty => "Start empty",
            Self::Temporary => "Continue",
            Self::Retry => "Try again",
            Self::Quit => "Quit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseDialog {
    pub problem: Problem,
    /// The store's own words.
    pub error: String,
    pub path: PathBuf,
    pub selected: Choice,
    /// A way runs on the runtime (the buttons wait).
    pub busy: bool,
    /// The last way that did not work, in words.
    pub failed: String,
}

/// The way that fits the problem: preselected and marked "(Recommended)" (review 59, P9.3).
const fn recommended(problem: &Problem) -> Choice {
    match problem {
        Problem::Newer { .. } => Choice::Temporary,
        Problem::Damaged => Choice::Rescue,
        Problem::NoAccess => Choice::Retry,
    }
}

impl DatabaseDialog {
    pub const fn recommended(&self) -> Choice {
        recommended(&self.problem)
    }

    pub fn new(problem: Problem, error: String, path: PathBuf) -> Self {
        Self {
            selected: recommended(&problem),
            problem,
            error,
            path,
            busy: false,
            failed: String::new(),
        }
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| "The queue database".to_owned(), |n| n.to_string_lossy().into_owned())
    }

    pub fn title(&self) -> &'static str {
        match self.problem {
            Problem::Newer { .. } => "The queue is from a newer RustCut",
            Problem::Damaged => "The queue cannot be opened",
            Problem::NoAccess => "The queue cannot be reached",
        }
    }

    pub fn text(&self) -> String {
        let name = self.file_name();
        let error = self.error.trim_start_matches("database error: ").trim_end_matches('.');
        let what = match &self.problem {
            Problem::Newer { found, known } => {
                format!("{name} was written by a newer RustCut (format {found}; this version knows {known}).")
            }
            Problem::Damaged => format!("{name} is damaged: {error}."),
            Problem::NoAccess => format!("{name} cannot be opened: {error}."),
        };
        format!("{what} Jobs saved now would be lost when RustCut quits.")
    }

    /// Whether a way can be taken for this problem (a newer or unreachable file has nothing to rescue).
    pub const fn enabled(&self, choice: Choice) -> bool {
        match choice {
            Choice::Rescue => matches!(self.problem, Problem::Damaged),
            Choice::StartEmpty | Choice::Temporary | Choice::Retry | Choice::Quit => true,
        }
    }

    pub fn detail(&self, choice: Choice) -> String {
        let name = self.file_name();
        match choice {
            Choice::Rescue => {
                "Copies every job that can still be read into a new queue; the old file is kept in the backup folder."
                    .to_owned()
            }
            Choice::StartEmpty => match self.problem {
                Problem::Newer { .. } => {
                    format!("{name} moves into the backup folder (the newer RustCut no longer finds its jobs there).")
                }
                Problem::Damaged | Problem::NoAccess => {
                    format!("{name} moves into the backup folder as {name}.damaged-<date>.")
                }
            },
            Choice::Temporary => "Jobs of this session are lost when RustCut quits.".to_owned(),
            Choice::Retry => {
                "After fixing the access rights, mounting the drive or closing the other program.".to_owned()
            }
            Choice::Quit => "Nothing is changed.".to_owned(),
        }
    }

    pub fn select(&mut self, index: usize) {
        if let Some(choice) = Choice::ALL.get(index).copied()
            && self.enabled(choice)
        {
            self.selected = choice;
        }
    }

    /// Up/Down: the next usable way.
    pub fn step(&mut self, delta: i32) {
        let at = Choice::ALL.iter().position(|c| *c == self.selected).unwrap_or(0);
        let mut index = i64::try_from(at).unwrap_or(0);
        let last = i64::try_from(Choice::ALL.len()).unwrap_or(1) - 1;
        loop {
            index += i64::from(delta.signum());
            if !(0..=last).contains(&index) {
                return;
            }
            let Some(choice) = usize::try_from(index).ok().and_then(|i| Choice::ALL.get(i).copied()) else {
                return;
            };
            if self.enabled(choice) {
                self.selected = choice;
                return;
            }
        }
    }

    /// "Copy details": what a bug report needs.
    pub fn details(&self) -> String {
        format!(
            "RustCut {}: the queue database cannot be used\nFile: {}\nProblem: {:?}\nError: {}",
            rustcut_core::app_identity::VERSION,
            self.path.display(),
            self.problem,
            self.error
        )
    }
}

/// The queue panel's line while the queue lives only in memory.
pub fn temporary_line(dialog: &DatabaseDialog) -> String {
    format!(
        "Temporary queue: {} cannot be used. Jobs are lost when RustCut quits.",
        dialog.file_name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(problem: Problem) -> DatabaseDialog {
        DatabaseDialog::new(
            problem,
            "database disk image is malformed".into(),
            "/d/queue.sqlite".into(),
        )
    }

    #[test]
    fn the_way_that_fits_is_preselected() {
        assert_eq!(dialog(Problem::Damaged).selected, Choice::Rescue);
        assert_eq!(dialog(Problem::NoAccess).selected, Choice::Retry);
        let mut newer = dialog(Problem::Newer { found: 3, known: 1 });
        assert_eq!(newer.selected, Choice::Temporary);
        assert!(!newer.enabled(Choice::Rescue));
        newer.select(0);
        assert_eq!(
            newer.selected,
            Choice::Temporary,
            "a way that does not fit cannot be picked"
        );
        newer.step(-1);
        newer.step(-1);
        assert_eq!(newer.selected, Choice::StartEmpty, "Up skips the rescue");
        assert!(newer.text().contains("format 3; this version knows 1"));
        assert_eq!(
            dialog(Problem::Damaged).text(),
            "queue.sqlite is damaged: database disk image is malformed. Jobs saved now would be lost when RustCut quits."
        );
        assert!(temporary_line(&newer).starts_with("Temporary queue: queue.sqlite cannot be used"));
    }
}
