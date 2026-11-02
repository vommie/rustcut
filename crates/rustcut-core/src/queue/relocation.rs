//! What moving a job's target is (P12 block 7, user decision Q-P12f): the request, the journal stored with the job while
//! a move is unfinished, the progress the status bar shows and the outcome of each job.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::job::{JobId, JobSpec, TargetNaming};
use crate::naming::same_path;

/// One job's new target, as the dialog decided it (conflicts resolved: a "(1)" is already in `stem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetMove {
    pub id: JobId,
    pub folder: PathBuf,
    /// The new name without extension (the container keeps the extension).
    pub stem: String,
    /// The user chose to replace a file at the new target.
    pub overwrite: bool,
}

impl TargetMove {
    /// The spec with this target; `None` when it is the job's target already. A changed name drops the name parts the
    /// Export card had (the editor then shows the new stem).
    pub fn apply(&self, spec: &JobSpec) -> Option<JobSpec> {
        let mut next = spec.clone();
        next.target.folder.clone_from(&self.folder);
        if next.target.stem != self.stem {
            next.target.stem.clone_from(&self.stem);
            next.target.naming = None::<TargetNaming>;
        }
        next.target.overwrite = self.overwrite;
        (!same_path(&next.target_path(), &spec.target_path()) || next.target.overwrite != spec.target.overwrite)
            .then_some(next)
    }
}

/// The journal of a move that is not finished (JSON in the job's `relocation` column). Written before anything happens,
/// cleared when the move is done or given up, so a crash in between is finished or undone at the next start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relocation {
    /// Where the file is (a rendering job: where its render puts it).
    pub from: PathBuf,
    pub to: PathBuf,
    #[serde(default)]
    pub overwrite: bool,
    /// The job renders: the spec has the new target already, the file moves once the render is verified.
    #[serde(default)]
    pub after_render: bool,
    /// The job's "overwrite" before the move, given back when a waiting move is taken back.
    #[serde(default)]
    pub restore_overwrite: bool,
}

impl Relocation {
    /// The journal in a job's column; an unreadable one is dropped with a warning (the move is not resumed).
    pub fn from_column(column: Option<&str>) -> Option<Self> {
        let json = column?;
        match serde_json::from_str(json) {
            Ok(relocation) => Some(relocation),
            Err(e) => {
                tracing::warn!(error = %e, "[Move] A job's unfinished move cannot be read; it is not resumed");
                None
            }
        }
    }

    pub fn to_column(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

/// What the status bar shows about the moves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelocationProgress {
    /// The file being moved now: its job, name, and the bytes done of a copy (0 of 0 for a rename).
    pub current: Option<(JobId, String)>,
    /// Moves of this run that ended, and all of this run (the run ends when nothing is left).
    pub finished: usize,
    pub total: usize,
    /// Bytes of this run's copies across file systems (renames count nothing).
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Jobs whose file moves once their render is verified.
    pub waiting: Vec<JobId>,
}

impl RelocationProgress {
    pub fn is_running(&self) -> bool {
        self.finished < self.total
    }
}

/// How one job's move ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocationOutcome {
    pub id: JobId,
    pub from: PathBuf,
    pub to: PathBuf,
    pub result: Result<Relocated, String>,
    /// The RustView record could not follow (the user moves it in RustView; the warning dialog names it).
    pub record_error: Option<String>,
}

/// What happened to a job whose move succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relocated {
    /// Only the target changed (nothing rendered yet, or the render waits).
    TargetChanged,
    /// The file was renamed on its file system.
    Renamed,
    /// The file was copied to another file system and verified, the original deleted.
    Copied,
    /// The file moves once its render is verified.
    AfterRender,
    /// The move was canceled (×); the job keeps its target.
    Canceled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::sample_spec;

    fn spec() -> JobSpec {
        let mut spec = sample_spec();
        spec.target.folder = "/out".into();
        spec.target.stem = "Clip - 01".into();
        spec.target.naming = Some(TargetNaming {
            name: "Clip".into(),
            counter: Some(1),
        });
        spec
    }

    #[test]
    fn a_new_folder_keeps_the_name_parts_a_new_name_drops_them() {
        let old = spec();
        let to_folder = TargetMove {
            id: JobId(1),
            folder: "/videos".into(),
            stem: "Clip - 01".into(),
            overwrite: false,
        };
        let moved = to_folder.apply(&old).unwrap();
        assert_eq!(
            moved.target_path(),
            PathBuf::from("/videos").join(old.target_path().file_name().unwrap())
        );
        assert_eq!(moved.target.naming, old.target.naming);
        let renamed = TargetMove {
            stem: "Beach".into(),
            ..to_folder.clone()
        }
        .apply(&old)
        .unwrap();
        assert_eq!(renamed.target.stem, "Beach");
        assert_eq!(renamed.target.naming, None);
        assert_eq!(
            TargetMove {
                folder: "/out/".into(),
                ..to_folder
            }
            .apply(&old),
            None,
            "the same target is no move"
        );
    }

    #[test]
    fn the_journal_round_trips_and_old_fields_default() {
        let journal = Relocation {
            from: "/out/a.mkv".into(),
            to: "/videos/a.mkv".into(),
            overwrite: false,
            after_render: true,
            restore_overwrite: false,
        };
        assert_eq!(Relocation::from_column(journal.to_column().as_deref()), Some(journal));
        assert_eq!(
            Relocation::from_column(Some(r#"{"from":"/a","to":"/b"}"#)),
            Some(Relocation {
                from: "/a".into(),
                to: "/b".into(),
                overwrite: false,
                after_render: false,
                restore_overwrite: false,
            })
        );
        assert_eq!(Relocation::from_column(Some("garbage")), None);
        assert_eq!(Relocation::from_column(None), None);
    }
}
