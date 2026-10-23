//! The conflict dialog without Slint (user decision Q-X1: a modal dialog like PyCutRv's, with every way out in one
//! place): what stands in the way of the target, and the choices, each with the file name it leads to and whether it
//! is free. Keys 1-6 pick a choice, Up/Down move, Enter saves, Escape cancels.

use std::path::{Path, PathBuf};

use rustcut_core::job::JobId;
use rustcut_core::naming::{
    Conflicts, FolderScan, QueuedTarget, Taken, TypedNameError, file_name, free_variant, next_free_counter, typed_stem,
};
use rustcut_core::render::Container;

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
    outcome: Option<Outcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictDialog {
    pub folder: PathBuf,
    pub file: String,
    pub conflicts: Conflicts,
    pub options: Vec<DialogOption>,
    pub selected: usize,
    pub typed: String,
    pub dont_ask: bool,
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
}

impl ConflictDialog {
    pub fn new(input: DialogInput<'_>) -> Self {
        let container = input.facts.container;
        let mut dialog = Self {
            folder: input.folder.to_path_buf(),
            file: file_name(input.stem, container),
            conflicts: input.conflicts,
            options: Vec::new(),
            selected: 0,
            typed: input.stem.to_owned(),
            dont_ask: false,
            container,
            scan: input.scan,
            queue: input.queue,
        };
        dialog.options = dialog.build(input.stem, input.export, input.facts);
        dialog.selected = dialog.options.iter().position(|o| o.enabled).unwrap_or(0);
        dialog.update_rename();
        dialog
    }

    fn taken(&self) -> Taken<'_> {
        Taken {
            folder: &self.folder,
            scan: &self.scan,
            queue: &self.queue,
        }
    }

    fn build(&self, stem: &str, export: &ExportState, facts: &JobFacts) -> Vec<DialogOption> {
        let taken = self.taken();
        let mut options = Vec::new();
        if export.template_uses_counter() {
            let start = if export.counter_enabled { export.counter } else { 0 };
            let found = next_free_counter(start, self.container, &taken, |n| export.stem_with(facts, Some(n)));
            let detail = match found {
                Some((n, _)) if export.auto_increment => {
                    format!("The counter continues at {:02} for the next job.", n.saturating_add(1))
                }
                Some((n, _)) => format!("The counter becomes {n:02}."),
                None => "No free number was found.".to_owned(),
            };
            options.push(DialogOption {
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
            });
        }
        let variant = free_variant(stem, self.container, &taken);
        options.push(DialogOption {
            choice: Choice::Variant,
            title: "Add \"(n)\"".to_owned(),
            file: variant
                .as_deref()
                .map(|s| file_name(s, self.container))
                .unwrap_or_default(),
            detail: "Keeps the name and adds the next free number in brackets.".to_owned(),
            enabled: variant.is_some(),
            destructive: false,
            outcome: variant.map(|stem| Outcome {
                stem,
                counter: None,
                overwrite: false,
                remove: Vec::new(),
            }),
        });
        options.push(DialogOption {
            choice: Choice::Rename,
            title: "Another name".to_owned(),
            file: String::new(),
            detail: String::new(),
            enabled: false,
            destructive: false,
            outcome: None,
        });
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
            options.push(DialogOption {
                choice: Choice::Overwrite,
                title: "Overwrite".to_owned(),
                file: self.file.clone(),
                detail: format!("The existing file is replaced only after the new render is verified.{also}"),
                enabled: true,
                destructive: true,
                outcome: Some(same),
            });
        } else if queued > 0 {
            let running = self.conflicts.queued.iter().find(|q| q.state.is_active());
            let ids: Vec<JobId> = self.conflicts.queued.iter().map(|q| q.id).collect();
            options.push(DialogOption {
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
                    ..same.clone()
                }),
            });
            options.push(DialogOption {
                choice: Choice::KeepName,
                title: "Save anyway".to_owned(),
                file: self.file.clone(),
                detail: "Renders after the queued jobs and replaces their file.".to_owned(),
                enabled: true,
                destructive: false,
                outcome: Some(same),
            });
        }
        options
    }

    /// The text of "Don't ask again" for the conflicts shown.
    pub fn dont_ask_text(&self) -> &'static str {
        if self.conflicts.exists.is_some() {
            "Don't ask again: take a free name"
        } else {
            "Don't ask again: save anyway"
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = index;
        }
    }

    /// Up (`-1`) or down (`1`), skipping choices that cannot be taken; the typed name can always be chosen.
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
                self.selected = i;
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

    /// The text of the confirm button.
    pub fn confirm_text(&self) -> String {
        match self.current() {
            Some(o) if o.enabled && o.choice == Choice::Overwrite => format!("Overwrite {}", o.file),
            Some(o) if o.enabled && o.choice == Choice::RemoveQueued => "Remove and save".to_owned(),
            Some(o) if o.enabled => format!("Save as {}", o.file),
            _ => "Save".to_owned(),
        }
    }
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

#[cfg(test)]
mod tests {
    use rustcut_core::job::JobStateKind;
    use rustcut_core::naming::ExistingFile;
    use rustcut_core::render::builtin_profiles;
    use rustcut_core::settings::{ExportPanelState, ExportSettings};
    use rustcut_core::time::Timestamp;

    use super::*;

    fn export(counter_enabled: bool) -> ExportState {
        let mut st = ExportState::new(
            &ExportSettings::default(),
            &ExportPanelState {
                counter_enabled,
                counter: 1,
                ..ExportPanelState::default()
            },
        );
        st.file_opened(Path::new("/out/Clip.mp4"));
        st
    }

    fn facts() -> JobFacts {
        JobFacts::new(&builtin_profiles().remove(0), 1, Timestamp::from_secs(5))
    }

    fn dialog(st: &ExportState, conflicts: Conflicts, queue: Vec<QueuedTarget>) -> ConflictDialog {
        let stem = st.file(&facts()).unwrap().0;
        ConflictDialog::new(DialogInput {
            folder: Path::new("/out"),
            stem: &stem,
            conflicts,
            scan: FolderScan {
                readable: true,
                names: ["Clip - 01.mkv", "Clip - 02.mkv", "Clip.mkv", "Clip (1).mkv"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            },
            queue,
            export: st,
            facts: &facts(),
        })
    }

    fn exists() -> Conflicts {
        Conflicts {
            exists: Some(ExistingFile {
                bytes: 1,
                modified: None,
            }),
            ..Conflicts::default()
        }
    }

    #[test]
    fn an_existing_file_offers_numbers_names_and_overwrite() {
        let st = export(true);
        let mut d = dialog(&st, exists(), Vec::new());
        let choices: Vec<Choice> = d.options.iter().map(|o| o.choice).collect();
        assert_eq!(
            choices,
            vec![Choice::NextNumber, Choice::Variant, Choice::Rename, Choice::Overwrite]
        );
        assert_eq!(d.selected, 0);
        assert_eq!(d.options[0].file, "Clip - 03.mkv");
        assert!(d.options[0].detail.contains("04"), "{}", d.options[0].detail);
        assert_eq!(d.outcome().unwrap().counter, Some(3));
        assert_eq!(d.options[1].file, "Clip - 01 (1).mkv");
        assert_eq!(d.confirm_text(), "Save as Clip - 03.mkv");
        assert!(!d.options[2].enabled, "the typed name starts as the taken one");
        d.type_name("Clip - 02");
        assert!(d.options[2].detail.contains("taken"));
        d.type_name("Holiday");
        d.select(2);
        assert_eq!(d.outcome().unwrap().stem, "Holiday");
        d.step(1);
        assert_eq!(
            (d.current().unwrap().choice, d.confirm_text()),
            (Choice::Overwrite, "Overwrite Clip - 01.mkv".into())
        );
        assert!(d.outcome().unwrap().overwrite);
        d.step(1);
        assert_eq!(d.selected, 0, "wraps around");
        assert_eq!(d.dont_ask_text(), "Don't ask again: take a free name");
        assert_eq!(silent_outcome(&d, &st).unwrap().stem, "Clip - 03");
    }

    #[test]
    fn queued_jobs_offer_removal_and_save_anyway() {
        let mut st = export(false);
        st.counter_enabled = false;
        let queue = vec![QueuedTarget {
            id: JobId(4),
            target: PathBuf::from("/out/Clip.mkv"),
            state: JobStateKind::Waiting,
        }];
        let conflicts = Conflicts {
            queued: queue.clone(),
            ..Conflicts::default()
        };
        let d = dialog(&st, conflicts, queue.clone());
        let choices: Vec<Choice> = d.options.iter().map(|o| o.choice).collect();
        assert_eq!(
            choices,
            vec![
                Choice::NextNumber,
                Choice::Variant,
                Choice::Rename,
                Choice::RemoveQueued,
                Choice::KeepName
            ]
        );
        assert_eq!(d.options[0].title, "Add a number");
        assert_eq!(d.options[0].file, "Clip - 03.mkv", "01 and 02 are on the drive");
        assert_eq!(d.options[1].file, "Clip (2).mkv");
        assert_eq!(d.options[3].outcome.as_ref().unwrap().remove, vec![JobId(4)]);
        assert!(d.options[4].outcome.as_ref().unwrap().overwrite);
        assert_eq!(silent_outcome(&d, &st).unwrap().stem, "Clip (2)");
        let running = vec![QueuedTarget {
            state: JobStateKind::Rendering,
            ..queue[0].clone()
        }];
        let d = dialog(
            &st,
            Conflicts {
                queued: running.clone(),
                ..Conflicts::default()
            },
            running,
        );
        assert!(!d.options[3].enabled && d.options[3].detail.contains("rendering"));
        assert_eq!(d.dont_ask_text(), "Don't ask again: save anyway");
    }
}
