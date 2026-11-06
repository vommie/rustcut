//! The live check of a job's target before it is saved (PyCutRv's three warnings, each behind its `warnings` switch):
//! the file exists already, another job in the queue writes the same file, files in the folder start with the same
//! name. And the ways out of a conflict that the conflict dialog offers, each with the file name it leads to.
//!
//! The folder is read once per check ([`scan_folder`], on the runtime); everything else is pure.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{NameError, Namer, file_name, next_free_variant, same_path, sanitize_stem};
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
    /// The folder could be read (a folder that does not exist yet reads as empty: it is made when a job writes into it).
    pub readable: bool,
    /// The folder does not exist yet.
    pub missing: bool,
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
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return FolderScan {
                readable: true,
                missing: true,
                names: HashSet::new(),
            };
        }
        Err(_) => return FolderScan::default(),
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
    FolderScan {
        readable: true,
        missing: false,
        names,
    }
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

    /// The job's name is not free for another job: it will write its target, or it can be rendered again (a failed or
    /// canceled job; a free name offered from it failed once it ran again, user report 2026-11-06). A finished job's
    /// file is in the folder itself.
    pub const fn claims(&self) -> bool {
        self.pending() || matches!(self.state, JobStateKind::Failed | JobStateKind::Canceled)
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

/// Whether a file name in `folder` is used: on the drive or claimed by a job of the queue ([`QueuedTarget::claims`]).
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
        self.queue.iter().any(|q| q.claims() && same_path(&q.target, &path))
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

/// A free file name the conflict dialog found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeName {
    pub stem: String,
    /// The base name that makes it; `None` when the template does not show the name and only this file's stem differs.
    pub name: Option<String>,
}

/// The first free `Name (n)`. The number belongs to the base name, so it stands before what the template adds ("Clip (1)
/// - 01"); a template without the name gets it behind the stem.
pub fn free_variant(namer: &Namer, container: Container, taken: &Taken<'_>) -> Option<FreeName> {
    if !namer.uses_name() {
        let stem = namer.stem(namer.name()).ok()?;
        let free = next_free_variant(&stem, |candidate| !taken.stem_free(candidate, container))?;
        return Some(FreeName { stem: free, name: None });
    }
    let is_free = |name: &str| namer.stem(name).is_ok_and(|stem| taken.stem_free(&stem, container));
    let name = next_free_variant(namer.name(), |candidate| !is_free(candidate))?;
    Some(FreeName {
        stem: namer.stem(&name).ok()?,
        name: Some(name),
    })
}

/// A name typed in the conflict dialog: a new base name (the template makes the stem), or the stem itself when the
/// template does not show the name; or why it cannot be used.
pub fn typed_name(
    text: &str,
    namer: &Namer,
    container: Container,
    taken: &Taken<'_>,
) -> Result<FreeName, TypedNameError> {
    let raw = text.trim();
    // A typed extension of the container is dropped (people type "Clip.mkv").
    let raw = raw.strip_suffix(&format!(".{}", container.extension())).unwrap_or(raw);
    let typed = sanitize_stem(raw).map_err(TypedNameError::Invalid)?;
    let free = if namer.uses_name() {
        FreeName {
            stem: namer.stem(&typed).map_err(TypedNameError::Invalid)?,
            name: Some(typed),
        }
    } else {
        FreeName {
            stem: typed,
            name: None,
        }
    };
    if taken.stem_free(&free.stem, container) {
        Ok(free)
    } else {
        Err(TypedNameError::Taken(file_name(&free.stem, container)))
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
    use super::super::{DEFAULT_TEMPLATE, NameContext};
    use super::*;

    fn scan(names: &[&str]) -> FolderScan {
        FolderScan {
            readable: true,
            missing: false,
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
            next_free_counter(1, Container::Mkv, &taken, |_| Err(NameError::Empty)),
            None
        );
        assert_eq!(
            next_free_counter_except(1, Container::Mkv, &taken, &[4, 5, 7], stem_for),
            Some((6, "Clip - 06".into())),
            "04 and 05 exist in other formats"
        );
    }

    fn namer(template: &str, name: &str) -> Namer {
        Namer::new(
            template,
            NameContext {
                name: name.into(),
                source: "VID".into(),
                counter: Some(1),
                ..NameContext::default()
            },
        )
    }

    fn free(stem: &str, name: Option<&str>) -> FreeName {
        FreeName {
            stem: stem.into(),
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn the_offered_number_is_the_first_free_one_and_never_a_taken_one() {
        // User report 2026-11-06: saving "1 - 01.mkv" offered "1 (2) - 01.mkv", which the render then found taken.
        let folder = scan(&["1 - 01.mkv", "1 (1) - 01.mkv", "1 (3) - 01.mkv"]);
        let with = |queue: &[QueuedTarget]| {
            let taken = Taken {
                folder: Path::new("/out"),
                scan: &folder,
                queue,
            };
            free_variant(&namer(DEFAULT_TEMPLATE, "1"), Container::Mkv, &taken)
        };
        assert_eq!(
            with(&[]),
            Some(free("1 (2) - 01", Some("1 (2)"))),
            "the gap between (1) and (3)"
        );
        // A job that can still write "1 (2) - 01.mkv" claims it, whatever state it is in; a finished one wrote its file.
        for state in [
            JobStateKind::Waiting,
            JobStateKind::Held,
            JobStateKind::Rendering,
            JobStateKind::Paused,
            JobStateKind::Failed,
            JobStateKind::Canceled,
        ] {
            let queue = [queued(5, "/out/1 (2) - 01.mkv", state)];
            assert_eq!(with(&queue), Some(free("1 (4) - 01", Some("1 (4)"))), "{state:?}");
        }
        let finished = [queued(5, "/out/1 (2) - 01.mkv", JobStateKind::Finished)];
        assert_eq!(
            with(&finished),
            Some(free("1 (2) - 01", Some("1 (2)"))),
            "its file is gone"
        );
        // The name of the session already carries a number ("1 (3)" after an earlier "(n)"): the first free one still.
        let taken = Taken {
            folder: Path::new("/out"),
            scan: &folder,
            queue: &[],
        };
        assert_eq!(
            free_variant(&namer(DEFAULT_TEMPLATE, "1 (3)"), Container::Mkv, &taken),
            Some(free("1 (2) - 01", Some("1 (2)")))
        );
    }

    #[test]
    fn the_number_of_a_variant_goes_into_the_base_name() {
        let folder = scan(&["Clip - 01.mkv", "Clip (1) - 01.mkv", "Clip - 01 (1).mkv"]);
        let taken = Taken {
            folder: Path::new("/out"),
            scan: &folder,
            queue: &[],
        };
        assert_eq!(
            free_variant(&namer(DEFAULT_TEMPLATE, "Clip"), Container::Mkv, &taken),
            Some(free("Clip (2) - 01", Some("Clip (2)"))),
            "before the counter, not behind it"
        );
        assert_eq!(
            free_variant(&namer(DEFAULT_TEMPLATE, "Clip (4)"), Container::Mkv, &taken),
            Some(free("Clip (2) - 01", Some("Clip (2)"))),
            "a name with a number gets the first free one"
        );
        assert_eq!(
            free_variant(&namer("{{ source }} {{ counter }}", "Clip"), Container::Mkv, &taken),
            Some(free("VID 1 (1)", None)),
            "a template without the name: behind the stem, and no new base name"
        );
    }

    #[test]
    fn a_typed_name_is_a_base_name() {
        let folder = scan(&["Clip - 01.mkv"]);
        let taken = Taken {
            folder: Path::new("/out"),
            scan: &folder,
            queue: &[],
        };
        let namer_ = namer(DEFAULT_TEMPLATE, "Old");
        assert_eq!(
            typed_name(" New.mkv ", &namer_, Container::Mkv, &taken),
            Ok(free("New - 01", Some("New")))
        );
        assert_eq!(
            typed_name("Clip", &namer_, Container::Mkv, &taken),
            Err(TypedNameError::Taken("Clip - 01.mkv".into()))
        );
        assert!(matches!(
            typed_name("  ", &namer_, Container::Mkv, &taken),
            Err(TypedNameError::Invalid(_))
        ));
        assert_eq!(
            typed_name("New", &namer("{{ source }}", "Old"), Container::Mkv, &taken),
            Ok(free("New", None)),
            "a template without the name: the text is the stem"
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
        let missing = scan_folder(&dir.path().join("missing")).await;
        assert!(
            missing.readable && missing.missing && missing.names.is_empty(),
            "a folder that does not exist yet reads as empty (it is made on the fly)"
        );
        std::fs::write(dir.path().join("a file"), b"x").unwrap();
        assert!(
            !scan_folder(&dir.path().join("a file")).await.readable,
            "a file is no folder"
        );
    }
}
