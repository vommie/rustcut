//! An unusable queue database (M6.5d, user decision Q-N10): what is wrong with it, and the two ways out that touch the
//! file. "Rescue" moves the file aside (it is kept, with its WAL) and copies every job row that can still be read, with
//! its log, into a fresh database at the old place; "start empty" only moves the file aside. Nothing is ever deleted:
//! the moved file lies in the backup folder as `queue.sqlite.damaged-<time>`.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use super::{Store, StoreError};

/// Why the database cannot be used; it decides which way the dialog suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Written by a newer RustCut (its schema is left alone).
    Newer { found: usize, known: usize },
    /// The file is no database or its pages are broken.
    Damaged,
    /// The file or its folder cannot be opened or written (rights, a missing drive, another program holds it).
    NoAccess,
}

impl Problem {
    pub fn of(error: &StoreError) -> Self {
        match error {
            StoreError::NewerSchema { found, known } => Self::Newer {
                found: *found,
                known: *known,
            },
            StoreError::Sqlite(e) => match e.sqlite_error_code() {
                Some(
                    ErrorCode::CannotOpen
                    | ErrorCode::PermissionDenied
                    | ErrorCode::ReadOnly
                    | ErrorCode::DatabaseBusy
                    | ErrorCode::DatabaseLocked
                    | ErrorCode::SystemIoFailure
                    | ErrorCode::DiskFull,
                ) => Self::NoAccess,
                _ => Self::Damaged,
            },
            StoreError::Backup(_) => Self::NoAccess,
            StoreError::Migration(_) | StoreError::NoSuchJob(_) | StoreError::Encode(_) => Self::Damaged,
        }
    }
}

/// What a rescue did.
#[derive(Debug)]
pub struct Rescue {
    /// The fresh database at the old place, holding the rescued jobs.
    pub store: Store,
    pub jobs: usize,
    /// Rows that could not be read.
    pub lost: usize,
    /// The old file could be opened at all (else nothing was copied).
    pub opened: bool,
    /// Where the old file lies now.
    pub kept: PathBuf,
}

/// The job columns of schema 1 (the ones a rescue copies).
const JOB_COLUMNS: &str = "id, position, state, state_detail, attempt, created_at, started_at, finished_at, source_path, \
                           target_path, note, spec_version, spec, sync";

/// Moves the database at `path` aside and copies every readable job into a fresh one there.
pub fn rescue(path: &Path, backups: &Path) -> Result<Rescue, StoreError> {
    let kept = move_aside(path, backups)?;
    let store = Store::open(path, backups)?;
    let uri = format!("file:{}?mode=ro", kept.display());
    if let Err(e) = store.conn.execute("ATTACH DATABASE ?1 AS old", [&uri]) {
        tracing::warn!(error = %e, kept = %kept.display(), "[Store] The old database cannot be opened; nothing was rescued");
        return Ok(Rescue {
            store,
            jobs: 0,
            lost: 0,
            opened: false,
            kept,
        });
    }
    let ids = old_ids(&store.conn);
    let (mut jobs, mut lost) = (0, 0);
    for id in ids {
        let copied = store.conn.execute(
            &format!("INSERT INTO main.jobs ({JOB_COLUMNS}) SELECT {JOB_COLUMNS} FROM old.jobs WHERE id = ?1"),
            [id],
        );
        match copied {
            Ok(1) => {
                jobs += 1;
                let logs = store.conn.execute(
                    "INSERT INTO main.job_logs (job_id, at, stream, line) SELECT job_id, at, stream, line FROM \
                     old.job_logs WHERE job_id = ?1 ORDER BY id",
                    [id],
                );
                if let Err(e) = logs {
                    tracing::warn!(job = id, error = %e, "[Store] The log of a rescued job could not be read");
                }
            }
            Ok(_) => {}
            Err(e) => {
                lost += 1;
                tracing::warn!(job = id, error = %e, "[Store] A job could not be rescued");
            }
        }
    }
    // Job ids are never reused: the fresh table counts on from the old one's highest number.
    if let Ok(seq) = store
        .conn
        .query_row("SELECT seq FROM old.sqlite_sequence WHERE name = 'jobs'", [], |r| {
            r.get::<_, i64>(0)
        })
    {
        store.conn.execute(
            "UPDATE main.sqlite_sequence SET seq = max(seq, ?1) WHERE name = 'jobs'",
            [seq],
        )?;
        store.conn.execute(
            "INSERT INTO main.sqlite_sequence (name, seq) SELECT 'jobs', ?1 WHERE NOT EXISTS \
             (SELECT 1 FROM main.sqlite_sequence WHERE name = 'jobs')",
            [seq],
        )?;
    }
    store.conn.execute("DETACH DATABASE old", [])?;
    tracing::info!(jobs, lost, kept = %kept.display(), "[Store] Readable jobs rescued into a fresh database");
    Ok(Rescue {
        store,
        jobs,
        lost,
        opened: true,
        kept,
    })
}

/// Moves the database at `path` aside and starts a fresh, empty one there; returns it and where the old file lies.
pub fn start_empty(path: &Path, backups: &Path) -> Result<(Store, PathBuf), StoreError> {
    let kept = move_aside(path, backups)?;
    let store = Store::open(path, backups)?;
    tracing::info!(kept = %kept.display(), "[Store] The database was moved aside; the queue starts empty");
    Ok((store, kept))
}

/// The ids of the old jobs: from the table, or (an unreadable index) every number up to the last one handed out.
fn old_ids(conn: &Connection) -> Vec<i64> {
    let listed = conn
        .prepare("SELECT id FROM old.jobs ORDER BY position, id")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        });
    match listed {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e, "[Store] The job list is unreadable; trying every job number");
            let last = conn
                .query_row("SELECT seq FROM old.sqlite_sequence WHERE name = 'jobs'", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap_or(0);
            (1..=last.min(1_000_000)).collect()
        }
    }
}

/// Moves the file and its WAL and shared-memory files into `backups` as `<name>.damaged-<time>` (keeping the
/// suffixes SQLite looks for).
fn move_aside(path: &Path, backups: &Path) -> Result<PathBuf, StoreError> {
    let name = path
        .file_name()
        .map_or_else(|| "queue.sqlite".into(), |n| n.to_string_lossy().into_owned());
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    std::fs::create_dir_all(backups).map_err(|e| StoreError::Backup(e.to_string()))?;
    let kept = backups.join(format!("{name}.damaged-{stamp}"));
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", path.display()));
        if !from.exists() {
            continue;
        }
        let to = PathBuf::from(format!("{}{suffix}", kept.display()));
        if std::fs::rename(&from, &to).is_err() {
            // Another file system: copy, then remove the original.
            std::fs::copy(&from, &to).map_err(|e| StoreError::Backup(e.to_string()))?;
            std::fs::remove_file(&from).map_err(|e| StoreError::Backup(e.to_string()))?;
        }
    }
    Ok(kept)
}

/// Opens `path` read-only to tell whether it is a database at all (tests and the dialog's details).
pub fn readable(path: &Path) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobState, sample_spec};

    fn database_with_jobs(dir: &Path, n: usize) -> PathBuf {
        let path = dir.join("queue.sqlite");
        let mut store = Store::open(&path, &dir.join("backups")).unwrap();
        for _ in 0..n {
            store.insert_job(&sample_spec(), &JobState::Waiting).unwrap();
        }
        let last = store.jobs().unwrap().last().unwrap().id;
        store
            .append_log(last, crate::store::LogStream::App, &["queued".into()])
            .unwrap();
        // Ids are never reused: the third job is deleted, the next id stays 4.
        store.delete_jobs(&[last]).unwrap();
        path
    }

    #[test]
    fn problems_are_told_apart() {
        assert_eq!(
            Problem::of(&StoreError::NewerSchema { found: 3, known: 1 }),
            Problem::Newer { found: 3, known: 1 }
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        std::fs::write(
            &path,
            b"this is not a database at all, but long enough to have a header....",
        )
        .unwrap();
        let e = Store::open(&path, &dir.path().join("backups")).unwrap_err();
        assert_eq!(Problem::of(&e), Problem::Damaged, "{e}");
        let e = Store::open(&dir.path().join("missing/folder/queue.sqlite"), dir.path()).unwrap_err();
        assert_eq!(Problem::of(&e), Problem::NoAccess, "{e}");
    }

    #[test]
    fn a_rescue_keeps_the_file_and_copies_the_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        let path = database_with_jobs(dir.path(), 3);
        let mut rescued = rescue(&path, &backups).unwrap();
        assert_eq!((rescued.jobs, rescued.lost), (2, 0));
        assert!(readable(&rescued.kept), "the old file is kept");
        assert!(rescued.kept.starts_with(&backups));
        let jobs = rescued.store.jobs().unwrap();
        assert_eq!(jobs.iter().map(|j| j.id.0).collect::<Vec<_>>(), [1, 2]);
        let next = rescued.store.insert_job(&sample_spec(), &JobState::Waiting).unwrap();
        assert_eq!(next.id.0, 4, "ids are never reused");
    }

    #[test]
    fn starting_empty_moves_the_file_aside() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        let path = database_with_jobs(dir.path(), 2);
        let (store, kept) = start_empty(&path, &backups).unwrap();
        assert!(store.jobs().unwrap().is_empty());
        assert!(readable(&kept) && path.exists());
    }
}
