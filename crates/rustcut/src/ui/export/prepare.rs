//! Save's look at the target folder, off the UI thread: the folder's names, the target file, the conflicts and the
//! conflict dialog with its choices, all at once. Every name the dialog offers is also checked on the drive with the
//! render's own test (`naming::on_disk`), so a listing that misses a file can never offer its name (user report
//! 2026-11-15: a folder of more than 50 000 files offered "1 (1) - 01.mp4", which existed, and the render failed).

use std::path::PathBuf;

use rustcut_core::naming::{
    CheckInput, Conflicts, ExistingFile, QueuedTarget, Taken, check, existing_file_now, file_name, on_disk,
    scan_folder_now,
};
use rustcut_core::settings::{ConflictDialogSettings, WarningSettings};

use super::dialog::{ConflictDialog, DialogInput};
use super::state::{ExportState, JobFacts};

/// What the check needs, copied from the card on the UI thread.
pub(super) struct Prepare {
    pub folder: PathBuf,
    /// The stem the card shows.
    pub stem: String,
    pub facts: JobFacts,
    pub export: ExportState,
    pub warnings: WarningSettings,
    pub settings: ConflictDialogSettings,
    /// Every job's target, read before the folder: a job that finishes in between has its file in the listing.
    pub queue: Vec<QueuedTarget>,
}

/// What the check found.
pub(super) struct Prepared {
    pub conflicts: Conflicts,
    pub exists: Option<ExistingFile>,
    /// The dialog with its choices (shown when the target needs a decision, or used by the silent way).
    pub dialog: ConflictDialog,
    /// The file name asked about is free: not in the folder, not on the drive, not claimed by a job.
    pub chosen_free: bool,
}

impl Prepare {
    /// Reads the folder and decides (blocking: run it on the runtime's blocking pool). `chosen` is a file name the user
    /// picked in the dialog, checked once more.
    pub fn run(self, chosen: Option<&str>) -> Prepared {
        let mut scan = scan_folder_now(&self.folder);
        let target = self.folder.join(file_name(&self.stem, self.facts.container));
        let exists = existing_file_now(&target);
        let disk: &dyn Fn(&std::path::Path) -> bool = &on_disk;
        let chosen_free = chosen.is_none_or(|file| {
            !Taken {
                folder: &self.folder,
                scan: &scan,
                queue: &self.queue,
                disk: Some(disk),
            }
            .name(file)
        });
        // A chosen name the drive has but the listing missed counts for the dialog's typed name too (the UI thread
        // checks that one against the listing only).
        if let Some(file) = chosen.filter(|_| !chosen_free) {
            scan.names.insert(file.to_owned());
        }
        let conflicts = check(&CheckInput {
            target: &target,
            base: &self.export.name,
            source: self.export.source.as_deref(),
            scan: &scan,
            exists,
            queue: &self.queue,
            warnings: &self.warnings,
        });
        // The dialog shows the existing file even when its warning is off (the silent way needs its choices).
        let mut shown = conflicts.clone();
        shown.exists = exists;
        let dialog = ConflictDialog::new(DialogInput {
            folder: &self.folder,
            stem: &self.stem,
            conflicts: shown,
            scan,
            queue: self.queue,
            export: &self.export,
            facts: &self.facts,
            settings: &self.settings,
            disk: Some(disk),
        });
        Prepared {
            conflicts,
            exists,
            dialog,
            chosen_free,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rustcut_core::job::{JobId, JobStateKind};
    use rustcut_core::naming::{FolderScan, scan_folder_now};
    use rustcut_core::render::{Container, builtin_profiles};
    use rustcut_core::settings::{ExportPanelState, ExportSettings};
    use rustcut_core::time::Timestamp;

    use super::super::dialog::{Choice, silent_outcome};
    use super::*;

    /// The files the folder may hold besides the source "1.mp4".
    const NAMES: [&str; 6] = [
        "1 - 01.mp4",
        "1 (1) - 01.mp4",
        "1 (2) - 01.mp4",
        "1 (3) - 01.mp4",
        "1 - 02.mp4",
        "1 - 03.mp4",
    ];

    fn card(source: &Path) -> (ExportState, JobFacts) {
        let panel = ExportPanelState {
            counter_enabled: true,
            counter: 1,
            auto_increment: true,
            ..ExportPanelState::default()
        };
        let mut st = ExportState::new(&ExportSettings::default(), &panel);
        st.file_opened(source);
        let mp4 = builtin_profiles()
            .into_iter()
            .find(|p| p.container == Container::Mp4)
            .unwrap();
        (st, JobFacts::new(&mp4, 1, Timestamp::from_secs(5)))
    }

    /// Every name the dialog lets the user take without overwriting.
    fn offered(dialog: &ConflictDialog) -> Vec<String> {
        dialog
            .options
            .iter()
            .filter(|o| o.enabled && matches!(o.choice, Choice::NextNumber | Choice::Variant | Choice::Rename))
            .map(|o| o.file.clone())
            .collect()
    }

    #[test]
    fn an_offered_name_always_passes_the_renders_check() {
        // User report 2026-11-15: "1 (1) - 01.mp4" was offered although it existed, and the render failed on it. For
        // every mix of files, a listing that misses some of them, and jobs in every state: what the dialog offers (and
        // the silent way takes) is free on the drive and claimed by no job.
        for mask in 0_u32..64 {
            for state in [
                None,
                Some(JobStateKind::Waiting),
                Some(JobStateKind::Failed),
                Some(JobStateKind::Finished),
            ] {
                let dir = tempfile::tempdir().unwrap();
                std::fs::File::create(dir.path().join("1.mp4")).unwrap();
                for (i, name) in NAMES.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        std::fs::File::create(dir.path().join(name)).unwrap();
                    }
                }
                let queue: Vec<QueuedTarget> = state
                    .map(|state| {
                        ["1 (4) - 01.mp4", "1 - 04.mp4"]
                            .iter()
                            .zip(1..)
                            .map(|(file, id)| QueuedTarget {
                                id: JobId(id),
                                target: dir.path().join(file),
                                state,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let claimed = |file: &str| queue.iter().any(|q| q.claims() && q.target == dir.path().join(file));
                let (st, facts) = card(&dir.path().join("1.mp4"));
                // The listing misses every second file (cut short, changed since it was read).
                let mut partial = scan_folder_now(dir.path());
                for name in NAMES.iter().step_by(2) {
                    partial.names.remove(*name);
                }
                let disk: &dyn Fn(&Path) -> bool = &on_disk;
                let settings = ConflictDialogSettings::default();
                let dialog = ConflictDialog::new(DialogInput {
                    folder: dir.path(),
                    stem: "1 - 01",
                    conflicts: Conflicts::default(),
                    scan: partial,
                    queue: queue.clone(),
                    export: &st,
                    facts: &facts,
                    settings: &settings,
                    disk: Some(disk),
                });
                let found = Prepare {
                    folder: dir.path().to_path_buf(),
                    stem: "1 - 01".into(),
                    facts: facts.clone(),
                    export: st.clone(),
                    warnings: WarningSettings::default(),
                    settings: settings.clone(),
                    queue: queue.clone(),
                }
                .run(None);
                let silent = silent_outcome(&found.dialog, &st).map(|o| file_name(&o.stem, Container::Mp4));
                for file in offered(&dialog).into_iter().chain(offered(&found.dialog)).chain(silent) {
                    assert!(
                        !on_disk(&dir.path().join(&file)) && !claimed(&file),
                        "{file} offered with files {mask:06b} and a job {state:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_name_taken_after_the_dialog_opened_is_found_at_save() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1.mp4", "1 - 01.mp4"] {
            std::fs::File::create(dir.path().join(name)).unwrap();
        }
        let (st, facts) = card(&dir.path().join("1.mp4"));
        let prepare = || Prepare {
            folder: dir.path().to_path_buf(),
            stem: "1 - 01".into(),
            facts: facts.clone(),
            export: st.clone(),
            warnings: WarningSettings::default(),
            settings: ConflictDialogSettings::default(),
            queue: Vec::new(),
        };
        assert!(prepare().run(Some("1 (1) - 01.mp4")).chosen_free);
        std::fs::File::create(dir.path().join("1 (1) - 01.mp4")).unwrap();
        let again = prepare().run(Some("1 (1) - 01.mp4"));
        assert!(!again.chosen_free);
        assert!(
            again.dialog.options.iter().all(|o| o.file != "1 (1) - 01.mp4"),
            "the fresh choices skip it"
        );
        assert!(FolderScan::default().names.is_empty());
    }
}
