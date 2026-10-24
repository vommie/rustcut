//! The conflict dialog without Slint (user decisions Q-X1, Q-X6 and the requests of 2026-10-24): what stands in the way
//! of the target, the choices (each with the file name it leads to and whether it is free) and the tabs. The first tab,
//! "Decision", holds every way out: the primary ones first (a free name and overwrite, or the queued jobs' ways), then
//! the other free name and a typed one; "Similar files" lists the numbered series of the name and the other names that
//! start like it, video files only (`media::is_video_file`; settings `dialogs.conflict`); "Queue" the jobs that write the file when it also exists. Keys 1-6 pick
//! a choice (the Decision tab shows), Up/Down move, Tab/Shift+Tab switch tabs, Enter saves, Escape cancels. Whether it asks at all is a warning
//! setting (`warnings.target_exists`, `warnings.target_queued`); the dialog itself has no "Don't ask again" (user
//! request 2026-10-24: it makes no sense for this question).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rustcut_core::job::JobId;
use rustcut_core::media::is_video_file;
use rustcut_core::naming::{
    Conflicts, FolderScan, MAX_SIMILAR, QueuedTarget, SeriesFile, Taken, TypedNameError, file_name, free_variant,
    next_free_counter_except, series, typed_stem,
};
use rustcut_core::render::Container;
use rustcut_core::settings::{ConflictDialogSettings, ConflictPreselect};

use super::state::{ExportState, JobFacts};

/// One way out of the conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The counter jumps to the next number whose name is free (or is switched on at the first free one).
    NextNumber,
    /// `Name (n)`.
    Variant,
    /// A name typed in the dialog.
    Rename,
    /// Replace the existing file after a verified render.
    Overwrite,
    /// Remove the queued jobs that write the same file, then save.
    RemoveQueued,
    /// Save with the same name; the job renders after the queued ones and replaces their file.
    KeepName,
}

/// A tab of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Decision,
    Similar,
    Queue,
}

impl Tab {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Decision => "Decision",
            Self::Similar => "Similar files",
            Self::Queue => "Queue",
        }
    }
}

/// What saving does after the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub stem: String,
    /// The counter value the name uses when the choice set it.
    pub counter: Option<u32>,
    pub overwrite: bool,
    pub remove: Vec<JobId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogOption {
    pub choice: Choice,
    pub title: String,
    /// The file name the choice leads to ("" while a typed name is missing).
    pub file: String,
    pub detail: String,
    pub enabled: bool,
    /// Replaces or removes something (drawn as a warning).
    pub destructive: bool,
    pub(super) outcome: Option<Outcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictDialog {
    pub folder: PathBuf,
    pub file: String,
    pub conflicts: Conflicts,
    /// In tab order: the Decision tab's choices first.
    pub options: Vec<DialogOption>,
    pub selected: usize,
    pub tab: Tab,
    pub typed: String,
    /// Every file of the name's numbered series in the folder (the target too, when it is one).
    pub series: Vec<SeriesFile>,
    /// Other files whose names start with the card's name (not in the series, not the target).
    pub others: Vec<String>,
    /// The free name listed first: the next number, or "(n)" (no counter, or chosen in the settings).
    primary_free: Choice,
    container: Container,
    scan: FolderScan,
    queue: Vec<QueuedTarget>,
}

/// What the dialog is built from.
pub struct DialogInput<'a> {
    pub folder: &'a Path,
    pub stem: &'a str,
    pub conflicts: Conflicts,
    pub scan: FolderScan,
    pub queue: Vec<QueuedTarget>,
    pub export: &'a ExportState,
    pub facts: &'a JobFacts,
    pub settings: &'a ConflictDialogSettings,
}

impl ConflictDialog {
    pub fn new(input: DialogInput<'_>) -> Self {
        let container = input.facts.container;
        let file = file_name(input.stem, container);
        // The series counts for the next free number even when it is not listed; only video files are a series.
        let mut series = if input.export.template_uses_counter() {
            let videos = input.scan.names.iter().filter(|n| is_video_file(n));
            series(videos, |n| input.export.stem_with(input.facts, Some(n)).ok())
        } else {
            Vec::new()
        };
        let used: Vec<u32> = series.iter().map(|f| f.counter).collect();
        if !input.settings.show_series {
            series.clear();
        }
        let others = if input.settings.show_similar {
            similar_names(&input.scan, &input.export.name, &file, &series)
        } else {
            Vec::new()
        };
        let mut dialog = Self {
            folder: input.folder.to_path_buf(),
            file,
            conflicts: input.conflicts,
            options: Vec::new(),
            selected: 0,
            tab: Tab::Decision,
            typed: input.stem.to_owned(),
            series,
            others,
            primary_free: Choice::NextNumber,
            container,
            scan: input.scan,
            queue: input.queue,
        };
        dialog.options = dialog.build(input.stem, input.export, input.facts, input.settings.preselect, &used);
        dialog.update_rename();
        dialog.selected = dialog.preselected(input.settings.preselect);
        dialog
    }

    fn taken(&self) -> Taken<'_> {
        Taken {
            folder: &self.folder,
            scan: &self.scan,
            queue: &self.queue,
        }
    }

    fn build(
        &mut self,
        stem: &str,
        export: &ExportState,
        facts: &JobFacts,
        preselect: ConflictPreselect,
        used: &[u32],
    ) -> Vec<DialogOption> {
        let taken = self.taken();
        let next = export.template_uses_counter().then(|| {
            let start = if export.counter_enabled { export.counter } else { 0 };
            let found = next_free_counter_except(start, self.container, &taken, used, |n| {
                export.stem_with(facts, Some(n))
            });
            let detail = match found {
                Some((n, _)) if export.auto_increment => {
                    format!("The counter continues at {:02} for the next job.", n.saturating_add(1))
                }
                Some((n, _)) => format!("The counter becomes {n:02}."),
                None => "No free number was found.".to_owned(),
            };
            DialogOption {
                choice: Choice::NextNumber,
                title: if export.counter_enabled {
                    "Next free number"
                } else {
                    "Add a number"
                }
                .to_owned(),
                file: found
                    .as_ref()
                    .map(|(_, s)| file_name(s, self.container))
                    .unwrap_or_default(),
                detail,
                enabled: found.is_some(),
                destructive: false,
                outcome: found.map(|(n, stem)| Outcome {
                    stem,
                    counter: Some(n),
                    overwrite: false,
                    remove: Vec::new(),
                }),
            }
        });
        let found = free_variant(stem, self.container, &taken);
        let variant = DialogOption {
            choice: Choice::Variant,
            title: "Add \"(n)\"".to_owned(),
            file: found
                .as_deref()
                .map(|s| file_name(s, self.container))
                .unwrap_or_default(),
            detail: "Keeps the name and adds the next free number in brackets.".to_owned(),
            enabled: found.is_some(),
            destructive: false,
            outcome: found.map(|stem| Outcome {
                stem,
                counter: None,
                overwrite: false,
                remove: Vec::new(),
            }),
        };
        let rename = DialogOption {
            choice: Choice::Rename,
            title: "Own name".to_owned(),
            file: String::new(),
            detail: String::new(),
            enabled: false,
            destructive: false,
            outcome: None,
        };
        let (primary, secondary) = match next {
            Some(next) if preselect != ConflictPreselect::Variant => (next, Some(variant)),
            Some(next) => (variant, Some(next)),
            None => (variant, None),
        };
        self.primary_free = primary.choice;
        let mut options = vec![primary];
        options.extend(self.decision_options(stem));
        options.extend(secondary);
        options.push(rename);
        options
    }

    /// Overwrite, or the ways of the queued jobs.
    fn decision_options(&self, stem: &str) -> Vec<DialogOption> {
        let queued = self.conflicts.queued.len();
        let same = Outcome {
            stem: stem.to_owned(),
            counter: None,
            overwrite: true,
            remove: Vec::new(),
        };
        if self.conflicts.exists.is_some() {
            let also = match queued {
                0 => String::new(),
                1 => " A queued job writes it too.".to_owned(),
                n => format!(" {n} queued jobs write it too."),
            };
            return vec![DialogOption {
                choice: Choice::Overwrite,
                title: "Overwrite".to_owned(),
                file: self.file.clone(),
                detail: format!("The existing file is replaced only after the new render is verified.{also}"),
                enabled: true,
                destructive: true,
                outcome: Some(same),
            }];
        }
        if queued == 0 {
            return Vec::new();
        }
        let running = self.conflicts.queued.iter().find(|q| q.state.is_active());
        let ids: Vec<JobId> = self.conflicts.queued.iter().map(|q| q.id).collect();
        vec![
            DialogOption {
                choice: Choice::KeepName,
                title: "Save anyway".to_owned(),
                file: self.file.clone(),
                detail: "Renders after the queued jobs and replaces their file.".to_owned(),
                enabled: true,
                destructive: false,
                outcome: Some(same.clone()),
            },
            DialogOption {
                choice: Choice::RemoveQueued,
                title: if queued == 1 {
                    "Remove the queued job".to_owned()
                } else {
                    format!("Remove the {queued} queued jobs")
                },
                file: self.file.clone(),
                detail: running.map_or_else(
                    || "They leave the queue; this job takes their place at the end.".to_owned(),
                    |r| format!("Job {} is rendering; cancel it in the queue first.", r.id),
                ),
                enabled: running.is_none(),
                destructive: true,
                outcome: Some(Outcome {
                    remove: ids,
                    overwrite: false,
                    ..same
                }),
            },
        ]
    }

    /// The choice the dialog opens with: the one the settings name when it can be taken, else the first free name.
    fn preselected(&self, preselect: ConflictPreselect) -> usize {
        let want = match preselect {
            ConflictPreselect::NextNumber => self.primary_free,
            ConflictPreselect::Variant => Choice::Variant,
            ConflictPreselect::Overwrite => Choice::Overwrite,
        };
        let find = |test: &dyn Fn(&DialogOption) -> bool| self.options.iter().position(|o| o.enabled && test(o));
        find(&|o| o.choice == want)
            .or_else(|| find(&|o| !o.destructive))
            .or_else(|| find(&|_| true))
            .unwrap_or(0)
    }

    /// The tabs with content, in order.
    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Decision];
        if !self.series.is_empty() || !self.others.is_empty() {
            tabs.push(Tab::Similar);
        }
        if self.conflicts.exists.is_some() && !self.conflicts.queued.is_empty() {
            tabs.push(Tab::Queue);
        }
        tabs
    }

    /// The choices `tab` shows with their index in [`Self::options`] (the key that picks them is the index + 1): all
    /// on the Decision tab, none elsewhere.
    pub fn options_of(&self, tab: Tab) -> Vec<(usize, &DialogOption)> {
        if tab == Tab::Decision {
            self.options.iter().enumerate().collect()
        } else {
            Vec::new()
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if self.tabs().contains(&tab) {
            self.tab = tab;
        }
    }

    /// The next (`1`) or previous (`-1`) tab, wrapping around.
    pub fn cycle_tab(&mut self, delta: i32) {
        let tabs = self.tabs();
        let at = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        let next = if delta < 0 {
            at.checked_sub(1).unwrap_or(tabs.len().saturating_sub(1))
        } else {
            (at + 1) % tabs.len().max(1)
        };
        if let Some(tab) = tabs.get(next) {
            self.tab = *tab;
        }
    }

    /// Picks the choice at `index`; the Decision tab shows it.
    pub fn select(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = index;
            self.tab = Tab::Decision;
        }
    }

    /// Up (`-1`) or down (`1`), skipping choices that cannot be taken (the typed name can always be chosen); the
    /// Decision tab shows the choice.
    pub fn step(&mut self, delta: i32) {
        let count = self.options.len();
        let mut i = self.selected;
        for _ in 0..count {
            i = if delta < 0 {
                i.checked_sub(1).unwrap_or(count - 1)
            } else {
                (i + 1) % count
            };
            if self
                .options
                .get(i)
                .is_some_and(|o| o.enabled || o.choice == Choice::Rename)
            {
                self.select(i);
                return;
            }
        }
    }

    /// The typed name changed: its file name and whether it is free.
    pub fn type_name(&mut self, text: &str) {
        text.clone_into(&mut self.typed);
        self.update_rename();
    }

    fn update_rename(&mut self) {
        let result = typed_stem(&self.typed, self.container, &self.taken());
        let container = self.container;
        let Some(option) = self.options.iter_mut().find(|o| o.choice == Choice::Rename) else {
            return;
        };
        match result {
            Ok(stem) => {
                option.file = file_name(&stem, container);
                "Free.".clone_into(&mut option.detail);
                option.enabled = true;
                option.outcome = Some(Outcome {
                    stem,
                    counter: None,
                    overwrite: false,
                    remove: Vec::new(),
                });
            }
            Err(e) => {
                option.file = String::new();
                option.detail = match e {
                    TypedNameError::Taken(file) => format!("{file} is taken too."),
                    TypedNameError::Invalid(e) => format!("Not usable: {e}."),
                };
                option.enabled = false;
                option.outcome = None;
            }
        }
    }

    pub fn current(&self) -> Option<&DialogOption> {
        self.options.get(self.selected)
    }

    /// What Save does with the selected choice (`None` while it cannot be taken).
    pub fn outcome(&self) -> Option<Outcome> {
        self.current().filter(|o| o.enabled).and_then(|o| o.outcome.clone())
    }

    /// The text of the confirm button (without the file name, which the chosen row shows: a long name would crowd
    /// the button, user request 2026-10-24).
    pub fn confirm_text(&self) -> &'static str {
        match self.current() {
            Some(o) if o.enabled && o.choice == Choice::Overwrite => "Overwrite",
            Some(o) if o.enabled && o.choice == Choice::RemoveQueued => "Remove and save",
            _ => "Save",
        }
    }
}

/// Video files whose names start with `base` (ignoring case), except the target, hidden files and the series; sorted, at
/// most [`MAX_SIMILAR`].
fn similar_names(scan: &FolderScan, base: &str, target: &str, series: &[SeriesFile]) -> Vec<String> {
    let base = base.trim().to_lowercase();
    if base.is_empty() {
        return Vec::new();
    }
    let in_series: HashSet<&str> = series.iter().map(|f| f.name.as_str()).collect();
    let mut names: Vec<String> = scan
        .names
        .iter()
        .filter(|n| {
            n.as_str() != target
                && !n.starts_with('.')
                && is_video_file(n)
                && !in_series.contains(n.as_str())
                && n.to_lowercase().starts_with(&base)
        })
        .cloned()
        .collect();
    names.sort();
    names.truncate(MAX_SIMILAR);
    names
}

/// The way out without asking (the warning is switched off): the next free number when the counter is on, else
/// `Name (n)`. Never overwrites.
pub fn silent_outcome(dialog: &ConflictDialog, export: &ExportState) -> Option<Outcome> {
    let preferred = if export.counter_enabled {
        Choice::NextNumber
    } else {
        Choice::Variant
    };
    dialog
        .options
        .iter()
        .filter(|o| o.enabled)
        .find(|o| o.choice == preferred)
        .or_else(|| dialog.options.iter().find(|o| o.enabled && o.choice == Choice::Variant))
        .and_then(|o| o.outcome.clone())
}
