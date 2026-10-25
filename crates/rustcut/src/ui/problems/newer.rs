//! The dialog about a settings file from a newer RustCut (M6.5d, user decision Q-N10), without Slint: a tab per newer
//! file (`config.toml`, `ui-state.toml`), on each the four ways (continue without saving, back up then save with this
//! version, back up then start with defaults, quit), the settings this version does not know, and what confirming
//! does with both tabs' choices at once.

use std::path::PathBuf;

/// Which file a tab is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum File {
    Settings,
    UiState,
}

impl File {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Settings => "config.toml",
            Self::UiState => "ui-state.toml",
        }
    }
}

/// A file a newer RustCut wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerFile {
    pub file: File,
    pub path: PathBuf,
    /// The format it has and the one this version knows.
    pub found: u32,
    pub known: u32,
    /// Its settings this version does not know (dotted keys).
    pub unknown: Vec<String>,
}

/// A way out (in the dialog's order; the key that picks it is its index + 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Read what is known, write nothing (the default).
    Continue,
    /// Back the file up, then write it with this version (its unknown settings stay only in the copy).
    SaveThis,
    /// Back the file up, then go back to the defaults.
    Defaults,
    Quit,
}

impl Choice {
    pub const ALL: [Self; 4] = [Self::Continue, Self::SaveThis, Self::Defaults, Self::Quit];
    /// Preselected and marked "(Recommended)" (review 57/58, P9.3): nothing is written, the newer file stays.
    pub const RECOMMENDED: Self = Self::Continue;

    pub const fn title(self) -> &'static str {
        match self {
            Self::Continue => "Continue without saving",
            Self::SaveThis => "Back up the file, then save with this version",
            Self::Defaults => "Back up the file, then start with defaults",
            Self::Quit => "Quit RustCut",
        }
    }

    pub fn detail(self, newer: &NewerFile) -> String {
        let n = newer.unknown.len();
        let unknown = match n {
            0 => "the copy keeps the file as it is".to_owned(),
            1 => "the 1 unknown setting stays only in the copy".to_owned(),
            n => format!("the {n} unknown settings stay only in the copy"),
        };
        match (self, newer.file) {
            (Self::Continue, File::Settings) => {
                "Changes hold until RustCut quits; the newer file stays as it is.".to_owned()
            }
            (Self::Continue, File::UiState) => {
                "This run's window and panels are not saved; the newer file stays as it is.".to_owned()
            }
            (Self::SaveThis, File::Settings) => format!("Saved now and with every change; {unknown}."),
            (Self::SaveThis, File::UiState) => format!("Saved when RustCut quits; {unknown}."),
            (Self::Defaults, File::Settings) => {
                "Every setting goes back to its default now; the copy keeps the old ones.".to_owned()
            }
            (Self::Defaults, File::UiState) => "Windows and panels start from their defaults at the next start (this \
                                                run keeps its layout); the copy keeps the old ones."
                .to_owned(),
            (Self::Quit, _) => "To start the newer RustCut instead. Nothing is changed.".to_owned(),
        }
    }

    /// The button's words.
    const fn confirm(self) -> &'static str {
        match self {
            Self::Quit => "Quit",
            Self::Continue | Self::SaveThis | Self::Defaults => "Continue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerDialog {
    pub files: Vec<NewerFile>,
    /// The choice on each file's tab.
    pub chosen: Vec<Choice>,
    pub tab: usize,
    /// The list of unknown settings is open.
    pub unknown_open: bool,
}

impl NewerDialog {
    /// The dialog for `files` (none = no dialog).
    pub fn new(files: Vec<NewerFile>) -> Option<Self> {
        (!files.is_empty()).then(|| Self {
            chosen: vec![Choice::RECOMMENDED; files.len()],
            files,
            tab: 0,
            unknown_open: false,
        })
    }

    pub fn current(&self) -> Option<&NewerFile> {
        self.files.get(self.tab)
    }

    pub fn choice(&self) -> Choice {
        self.chosen.get(self.tab).copied().unwrap_or(Choice::Continue)
    }

    pub fn set_tab(&mut self, tab: usize) {
        if tab < self.files.len() {
            self.tab = tab;
            self.unknown_open = false;
        }
    }

    /// The choice with `index` on the current tab; Quit on one tab is Quit for both.
    pub fn choose(&mut self, index: usize) {
        let Some(choice) = Choice::ALL.get(index).copied() else {
            return;
        };
        if let Some(slot) = self.chosen.get_mut(self.tab) {
            *slot = choice;
        }
    }

    pub fn step(&mut self, delta: i32) {
        let at = Choice::ALL.iter().position(|c| *c == self.choice()).unwrap_or(0);
        let next = i64::try_from(at).unwrap_or(0) + i64::from(delta);
        let last = i64::try_from(Choice::ALL.len()).unwrap_or(1) - 1;
        self.choose(usize::try_from(next.clamp(0, last)).unwrap_or(0));
    }

    pub fn quits(&self) -> bool {
        self.chosen.contains(&Choice::Quit)
    }

    pub fn confirm_text(&self) -> &'static str {
        if self.quits() {
            Choice::Quit.confirm()
        } else {
            self.choice().confirm()
        }
    }

    /// The header line of the current tab.
    pub fn text(&self) -> String {
        let Some(newer) = self.current() else {
            return String::new();
        };
        // Review 57/58 (P9.3): the facts, then the request to decide.
        format!(
            "{} was written by a newer RustCut (format {}; this version knows format {}). Decide how to handle the \
             conflict.",
            newer.file.name(),
            newer.found,
            newer.known
        )
    }

    /// "There are 3 settings this version does not know" (empty when there are none).
    pub fn unknown_title(&self) -> String {
        match self.current().map_or(0, |f| f.unknown.len()) {
            0 => String::new(),
            1 => "There is 1 setting this version does not know".to_owned(),
            n => format!("There are {n} settings this version does not know"),
        }
    }

    /// What confirming does: each file with its choice.
    pub fn outcome(&self) -> Vec<(&NewerFile, Choice)> {
        self.files.iter().zip(self.chosen.iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(file: File, unknown: &[&str]) -> NewerFile {
        NewerFile {
            file,
            path: PathBuf::from(format!("/c/{}", file.name())),
            found: 3,
            known: 2,
            unknown: unknown.iter().map(|k| (*k).to_owned()).collect(),
        }
    }

    #[test]
    fn both_files_are_tabs_of_one_dialog() {
        assert!(NewerDialog::new(Vec::new()).is_none());
        let mut d = NewerDialog::new(vec![
            newer(File::Settings, &["future.x", "general.turbo"]),
            newer(File::UiState, &[]),
        ])
        .unwrap();
        assert_eq!(d.choice(), Choice::Continue, "the default");
        assert_eq!(d.unknown_title(), "There are 2 settings this version does not know");
        assert!(
            d.text()
                .starts_with("config.toml was written by a newer RustCut (format 3;")
        );
        assert!(d.text().ends_with("Decide how to handle the conflict."));
        d.choose(1);
        d.set_tab(1);
        assert_eq!(d.unknown_title(), "");
        d.step(1);
        assert_eq!(d.choice(), Choice::SaveThis);
        assert_eq!(d.confirm_text(), "Continue");
        let outcome: Vec<Choice> = d.outcome().into_iter().map(|(_, c)| c).collect();
        assert_eq!(outcome, [Choice::SaveThis, Choice::SaveThis]);
        d.choose(3);
        assert!(d.quits());
        assert_eq!(d.confirm_text(), "Quit");
        assert!(
            Choice::SaveThis
                .detail(&d.files[0])
                .contains("the 2 unknown settings stay only in the copy")
        );
    }
}
