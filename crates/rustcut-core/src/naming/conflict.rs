//! The live check of a job's target before it is saved (PyCutRv's three warnings, each behind its `warnings` switch):
//! the file exists already, another job in the queue writes the same file, files in the folder start with the same
//! name. And the ways out of a conflict that the conflict dialog offers, each with the file name it leads to.
//!
//! The folder is read once per check ([`scan_folder`], on the runtime); everything else is pure.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{NameError, file_name, next_free_variant, same_path, sanitize_stem};
use crate::job::{JobId, JobStateKind};
use crate::render::Container;
use crate::settings::WarningSettings;

/// Folder entries read at most (a huge download folder must not stall the check).
pub const MAX_SCANNED: usize = 50_000;
/// Similar names listed at most.
pub const MAX_SIMILAR: usize = 50;
/// Counter values tried when looking for the next free number.
const MAX_COUNTER_TRIES: u32 = 10_000;

/// What a target folder holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderScan {
    /// The folder exists and could be read.
    pub readable: bool,
    /// File names in the folder (not sorted; at most [`MAX_SCANNED`]).
    pub names: HashSet<String>,
}

/// An existing target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingFile {
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

/// Reads the names in `folder` (never fails: an unreadable folder is reported as such).
pub async fn scan_folder(folder: &Path) -> FolderScan {
    let folder = folder.to_path_buf();
    // One blocking pass (tokio's `read_dir` hops to the blocking pool every 32 entries): names only, no `stat` per
    // entry, so the kernel answers from its directory cache without touching the drive.
    tokio::task::spawn_blocking(move || scan_folder_now(&folder))
        .await
        .unwrap_or_default()
}

/// [`scan_folder`] on the calling thread.
pub fn scan_folder_now(folder: &Path) -> FolderScan {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return FolderScan::default();
    };
    let mut names = HashSet::new();
    for entry in entries {
        if names.len() >= MAX_SCANNED {
            break;
        }
        match entry {
            Ok(entry) => {
                if let Some(name) = entry.file_name().to_str() {
                    names.insert(name.to_owned());
                }
            }
            Err(e) => {
                tracing::debug!(folder = %folder.display(), error = %e, "[Export] Folder scan stopped");
                break;
            }
        }
    }
    FolderScan { readable: true, names }
}

/// Size and time of an existing file (`None` when there is none).
pub async fn existing_file(path: &Path) -> Option<ExistingFile> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    Some(ExistingFile {
        bytes: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// A job of the queue that writes into the target folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedTarget {
    pub id: JobId,
    pub target: PathBuf,
    pub state: JobStateKind,
}

impl QueuedTarget {
    /// The job will still write its target (finished jobs wrote theirs; failed and canceled ones never will).
    pub const fn pending(&self) -> bool {
        matches!(
            self.state,
            JobStateKind::Waiting | JobStateKind::Held | JobStateKind::Rendering | JobStateKind::Paused
        )
    }
}

/// What stands in the way of a target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conflicts {
    /// The target file exists (warning `target_exists`).
    pub exists: Option<ExistingFile>,
    /// Jobs that will write the same file (warning `target_queued`).
    pub queued: Vec<QueuedTarget>,
    /// Other files in the folder whose names start with the typed name (warning `base_name_in_folder`), sorted.
    pub similar: Vec<String>,
}

impl Conflicts {
    /// Saving needs a decision (the similar names are only a hint).
    pub fn needs_decision(&self) -> bool {
        self.exists.is_some() || !self.queued.is_empty()
    }

    /// Some of the queued jobs render right now (they cannot be removed).
    pub fn queued_running(&self) -> bool {
        self.queued.iter().any(|q| q.state.is_active())
    }
}

/// Everything the check looks at.
#[derive(Debug, Clone, Copy)]
pub struct CheckInput<'a> {
    pub target: &'a Path,
    /// The typed name (before the counter), for the similar names.
    pub base: &'a str,
    /// The video being cut: never one of the similar names (it lies in its own folder; user decision Q-P8).
    pub source: Option<&'a Path>,
    pub scan: &'a FolderScan,
    pub exists: Option<ExistingFile>,
    pub queue: &'a [QueuedTarget],
    pub warnings: &'a WarningSettings,
}

pub fn check(input: &CheckInput<'_>) -> Conflicts {
    let target_name = input.target.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let exists = input.exists.filter(|_| input.warnings.target_exists);
    let queued = if input.warnings.target_queued {
        input
            .queue
            .iter()
            .filter(|q| q.pending() && same_path(&q.target, input.target))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let mut similar: Vec<String> = if input.warnings.base_name_in_folder && !input.base.trim().is_empty() {
        let base = input.base.trim().to_lowercase();
        let folder = input.target.parent();
        let is_source = |name: &str| {
            input
                .source
                .zip(folder)
                .is_some_and(|(source, folder)| same_path(&folder.join(name), source))
        };
        input
            .scan
            .names
            .iter()
            .filter(|n| {
                n.as_str() != target_name && !n.starts_with('.') && n.to_lowercase().starts_with(&base) && !is_source(n)
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    similar.sort();
    similar.truncate(MAX_SIMILAR);
    Conflicts {
        exists,
        queued,
        similar,
    }
}

/// Whether a file name in `folder` is used: on the drive or by a job of the queue that will still write it.
pub struct Taken<'a> {
    pub folder: &'a Path,
    pub scan: &'a FolderScan,
    pub queue: &'a [QueuedTarget],
}

impl Taken<'_> {
    pub fn name(&self, file: &str) -> bool {
        if self.scan.names.contains(file) {
            return true;
        }
        let path = self.folder.join(file);
        self.queue.iter().any(|q| q.pending() && same_path(&q.target, &path))
    }

    /// `stem.ext` is free.
    pub fn stem_free(&self, stem: &str, container: Container) -> bool {
        !self.name(&file_name(stem, container))
    }
}

/// The first counter after `counter` whose name (`stem_for(n)`) is free; `None` when the template fails or nothing is
/// free within ten thousand numbers.
pub fn next_free_counter(
    counter: u32,
    container: Container,
    taken: &Taken<'_>,
    stem_for: impl Fn(u32) -> Result<String, NameError>,
) -> Option<(u32, String)> {
    next_free_counter_except(counter, container, taken, &[], stem_for)
}

/// [`next_free_counter`] that also passes over the numbers in `used` (the name's numbered series in other formats:
/// "Clip - 02.mp4" makes 02 taken for an MKV job too, so a series never gets the same number twice).
pub fn next_free_counter_except(
    counter: u32,
    container: Container,
    taken: &Taken<'_>,
    used: &[u32],
    stem_for: impl Fn(u32) -> Result<String, NameError>,
) -> Option<(u32, String)> {
    (counter.saturating_add(1)..counter.saturating_add(MAX_COUNTER_TRIES))
        .filter(|n| !used.contains(n))
        .map(|n| stem_for(n).map(|stem| (n, stem)))
        .find(|candidate| match candidate {
            Ok((_, stem)) => taken.stem_free(stem, container),
            Err(_) => true,
        })
        .and_then(Result::ok)
}

/// The free `Name (n)` variant of `stem`.
pub fn free_variant(stem: &str, container: Container, taken: &Taken<'_>) -> Option<String> {
    next_free_variant(stem, |candidate| !taken.stem_free(candidate, container))
}

/// A name typed in the conflict dialog: its stem, or why it cannot be used.
pub fn typed_stem(text: &str, container: Container, taken: &Taken<'_>) -> Result<String, TypedNameError> {
    let raw = text.trim();
    // A typed extension of the container is dropped (people type "Clip.mkv").
    let raw = raw.strip_suffix(&format!(".{}", container.extension())).unwrap_or(raw);
    let stem = sanitize_stem(raw).map_err(TypedNameError::Invalid)?;
    if taken.stem_free(&stem, container) {
        Ok(stem)
    } else {
        Err(TypedNameError::Taken(file_name(&stem, container)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypedNameError {
    #[error(transparent)]
    Invalid(NameError),
    #[error("{0} is taken too")]
    Taken(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(names: &[&str]) -> FolderScan {
        FolderScan {
            readable: true,
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    fn queued(id: i64, target: &str, state: JobStateKind) -> QueuedTarget {
        QueuedTarget {
            id: JobId(id),
            target: PathBuf::from(target),
            state,
        }
    }

    #[test]
    fn the_three_warnings_follow_their_switches() {
        let folder = scan(&[
            "Clip - 01.mkv",
            "clip - 02.mp4",
            ".Clip - 03.rustcut-part.mkv",
            "Other.mkv",
            "Clip.mp4",
        ]);
        let queue = [
            queued(4, "/out/Clip - 01.mkv", JobStateKind::Waiting),
            queued(5, "/out/./Clip - 01.mkv", JobStateKind::Rendering),
            queued(6, "/out/Clip - 01.mkv", JobStateKind::Finished),
            queued(7, "/out/Clip - 01.mkv", JobStateKind::Failed),
        ];
        let warnings = WarningSettings::default();
        let input = CheckInput {
            target: Path::new("/out/Clip - 01.mkv"),
            base: "Clip",
            source: Some(Path::new("/out/Clip.mp4")),
            scan: &folder,
            exists: Some(ExistingFile {
                bytes: 10,
                modified: None,
            }),
            queue: &queue,
            warnings: &warnings,
        };
        let found = check(&input);
        assert!(found.exists.is_some() && found.needs_decision() && found.queued_running());
        assert_eq!(
            found.queued.iter().map(|q| q.id.0).collect::<Vec<_>>(),
            vec![4, 5],
            "done jobs never write again"
        );
        assert_eq!(
            found.similar,
            vec!["clip - 02.mp4"],
            "case-insensitive, the target, hidden files and the source video left out"
        );
        let elsewhere = check(&CheckInput {
            source: Some(Path::new("/videos/Clip.mp4")),
            ..input
        });
        assert_eq!(
            elsewhere.similar,
            vec!["Clip.mp4", "clip - 02.mp4"],
            "a file of the same name next to the target is no source"
        );
        let off = WarningSettings {
            target_exists: false,
            target_queued: false,
            base_name_in_folder: false,
            ..WarningSettings::default()
        };
        let quiet = check(&CheckInput {
            warnings: &off,
            ..input
        });
        assert_eq!(quiet, Conflicts::default());
    }

    #[test]
    fn ways_out_lead_to_free_names() {
        let folder = scan(&["Clip - 01.mkv", "Clip - 02.mkv", "Clip - 01 (1).mkv"]);
        let queue = [queued(3, "/out/Clip - 03.mkv", JobStateKind::Waiting)];
        let taken = Taken {
            folder: Path::new("/out"),
            scan: &folder,
            queue: &queue,
        };
        let stem_for = |n: u32| Ok(format!("Clip - {n:02}"));
        assert_eq!(
            next_free_counter(1, Container::Mkv, &taken, stem_for),
            Some((4, "Clip - 04".into())),
            "02 is on the drive, 03 queued"
        );
        assert_eq!(
            free_variant("Clip - 01", Container::Mkv, &taken).as_deref(),
            Some("Clip - 01 (2)")
        );
        assert_eq!(typed_stem(" New.mkv ", Container::Mkv, &taken), Ok("New".into()));
        assert_eq!(
            typed_stem("Clip - 02", Container::Mkv, &taken),
            Err(TypedNameError::Taken("Clip - 02.mkv".into()))
        );
        assert!(matches!(
            typed_stem("  ", Container::Mkv, &taken),
            Err(TypedNameError::Invalid(_))
        ));
        assert_eq!(
            next_free_counter(1, Container::Mkv, &taken, |_| Err(NameError::Empty)),
            None
        );
        assert_eq!(
            next_free_counter_except(1, Container::Mkv, &taken, &[4, 5, 7], stem_for),
            Some((6, "Clip - 06".into())),
            "04 and 05 exist in other formats"
        );
    }

    #[tokio::test]
    async fn scanning_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mkv"), b"12345").unwrap();
        let found = scan_folder(dir.path()).await;
        assert!(found.readable && found.names.contains("a.mkv"));
        assert_eq!(existing_file(&dir.path().join("a.mkv")).await.map(|f| f.bytes), Some(5));
        assert_eq!(existing_file(&dir.path().join("b.mkv")).await, None);
        assert!(!scan_folder(&dir.path().join("missing")).await.readable);
    }
}
