//! Job rows and job logs.

use std::path::PathBuf;

use rusqlite::{OptionalExtension as _, Row, params};
use serde::{Deserialize, Serialize};

use super::{Store, StoreError};
use crate::job::{JOB_SPEC_VERSION, JobId, JobSpec, JobState, JobStateKind};

/// The spec of a row: decoded, or kept as it is when it cannot be read (written by a newer version, or damaged). Such a row
/// is shown and never rewritten (compatibility rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecSlot {
    Decoded(Box<JobSpec>),
    Unreadable { version: u32, error: String },
}

/// One row of the `jobs` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: JobId,
    pub position: i64,
    pub state: JobState,
    pub attempt: u32,
    /// Unix milliseconds.
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    /// A note for the user (e.g. "interrupted by a crash, waiting again").
    pub note: Option<String>,
    pub spec: SpecSlot,
    /// The RustView sync record (JSON owned by the sync actor).
    pub sync: Option<String>,
    /// A move of the target file that is not finished (JSON owned by the queue, P12 block 7).
    pub relocation: Option<String>,
}

/// Which log of a job a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    App,
    Ffmpeg,
    RustView,
}

impl LogStream {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Ffmpeg => "ffmpeg",
            Self::RustView => "rustview",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "ffmpeg" => Self::Ffmpeg,
            "rustview" => Self::RustView,
            _ => Self::App,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub at: i64,
    pub stream: LogStream,
    pub line: String,
}

const COLUMNS: &str = "id, position, state_detail, attempt, created_at, started_at, finished_at, source_path, target_path, \
                       note, spec_version, spec, sync, relocation";

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let detail: String = row.get(2)?;
    let state = serde_json::from_str(&detail).unwrap_or_else(|e| JobState::Failed {
        reason: format!("unreadable state ({e}): {detail}"),
    });
    let version: u32 = row.get(10)?;
    let spec_json: String = row.get(11)?;
    let spec = match serde_json::from_str::<JobSpec>(&spec_json) {
        Ok(spec) if version <= JOB_SPEC_VERSION => SpecSlot::Decoded(Box::new(spec)),
        Ok(_) => SpecSlot::Unreadable {
            version,
            error: format!("written by a newer RustCut (format {version})"),
        },
        Err(e) => SpecSlot::Unreadable {
            version,
            error: e.to_string(),
        },
    };
    Ok(JobRecord {
        id: JobId(row.get(0)?),
        position: row.get(1)?,
        state,
        attempt: row.get(3)?,
        created_at: row.get(4)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        source_path: PathBuf::from(row.get::<_, String>(7)?),
        target_path: PathBuf::from(row.get::<_, String>(8)?),
        note: row.get(9)?,
        spec,
        sync: row.get(12)?,
        relocation: row.get(13)?,
    })
}

impl Store {
    /// Adds a job at the end of the queue.
    pub fn insert_job(&mut self, spec: &JobSpec, state: &JobState) -> Result<JobRecord, StoreError> {
        let position: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(position), 0) + 1 FROM jobs", [], |r| r.get(0))?;
        self.conn.execute(
            "INSERT INTO jobs (position, state, state_detail, created_at, source_path, target_path, spec_version, spec) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                position,
                state.kind().as_str(),
                serde_json::to_string(state)?,
                now_millis(),
                spec.source.to_string_lossy(),
                spec.target_path().to_string_lossy(),
                JOB_SPEC_VERSION,
                serde_json::to_string(spec)?,
            ],
        )?;
        let id = JobId(self.conn.last_insert_rowid());
        self.job(id)?.ok_or(StoreError::NoSuchJob(id.0))
    }

    pub fn job(&self, id: JobId) -> Result<Option<JobRecord>, StoreError> {
        let sql = format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id.0], read_row).optional()?)
    }

    /// Every job in queue order.
    pub fn jobs(&self) -> Result<Vec<JobRecord>, StoreError> {
        let sql = format!("SELECT {COLUMNS} FROM jobs ORDER BY position, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], read_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Replaces the spec of a job (editing a waiting job, ADR-6 re-sync follows).
    pub fn update_spec(&mut self, id: JobId, spec: &JobSpec) -> Result<(), StoreError> {
        let n = self.conn.execute(
            "UPDATE jobs SET spec = ?2, spec_version = ?3, source_path = ?4, target_path = ?5 WHERE id = ?1",
            params![
                id.0,
                serde_json::to_string(spec)?,
                JOB_SPEC_VERSION,
                spec.source.to_string_lossy(),
                spec.target_path().to_string_lossy()
            ],
        )?;
        if n == 0 {
            Err(StoreError::NoSuchJob(id.0))
        } else {
            Ok(())
        }
    }

    /// Sets the state; entering Rendering stamps `started_at`, a done state stamps `finished_at`, going back to Waiting
    /// from a done state starts a new attempt (and clears both).
    pub fn set_state(&mut self, id: JobId, state: &JobState, note: Option<&str>) -> Result<(), StoreError> {
        let kind = state.kind();
        let now = now_millis();
        let previous: Option<String> = self
            .conn
            .query_row("SELECT state FROM jobs WHERE id = ?1", [id.0], |r| r.get(0))
            .optional()?;
        let Some(previous) = previous else {
            return Err(StoreError::NoSuchJob(id.0));
        };
        let was_done = previous.parse::<JobStateKind>().is_ok_and(JobStateKind::is_done);
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE jobs SET state = ?2, state_detail = ?3, note = ?4 WHERE id = ?1",
            params![id.0, kind.as_str(), serde_json::to_string(state)?, note],
        )?;
        match kind {
            JobStateKind::Rendering => {
                tx.execute(
                    "UPDATE jobs SET started_at = COALESCE(started_at, ?2) WHERE id = ?1",
                    params![id.0, now],
                )?;
            }
            k if k.is_done() => {
                tx.execute("UPDATE jobs SET finished_at = ?2 WHERE id = ?1", params![id.0, now])?;
            }
            JobStateKind::Waiting if was_done => {
                tx.execute(
                    "UPDATE jobs SET attempt = attempt + 1, started_at = NULL, finished_at = NULL WHERE id = ?1",
                    [id.0],
                )?;
            }
            _ => {}
        }
        tx.commit()?;
        Ok(())
    }

    /// Stores the queue order: `ids` in their new order get positions 1, 2, ...; jobs not listed keep theirs after them.
    pub fn set_order(&mut self, ids: &[JobId]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let offset = i64::try_from(ids.len()).unwrap_or(i64::MAX);
        tx.execute("UPDATE jobs SET position = position + ?1", [offset])?;
        {
            let mut stmt = tx.prepare("UPDATE jobs SET position = ?2 WHERE id = ?1")?;
            for (i, id) in (1_i64..).zip(ids) {
                stmt.execute(params![id.0, i])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes jobs with their logs; returns how many rows went.
    pub fn delete_jobs(&mut self, ids: &[JobId]) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare("DELETE FROM jobs WHERE id = ?1")?;
            for id in ids {
                n += stmt.execute([id.0])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// Stores (or with `None` clears) the unfinished move of a job's target file (JSON of the queue).
    pub fn set_relocation(&mut self, id: JobId, relocation: Option<&str>) -> Result<(), StoreError> {
        let n = self.conn.execute(
            "UPDATE jobs SET relocation = ?2 WHERE id = ?1",
            params![id.0, relocation],
        )?;
        if n == 0 {
            Err(StoreError::NoSuchJob(id.0))
        } else {
            Ok(())
        }
    }

    /// Stores the RustView sync record of a job (JSON of the sync actor).
    pub fn set_sync(&mut self, id: JobId, sync: Option<&str>) -> Result<(), StoreError> {
        let n = self
            .conn
            .execute("UPDATE jobs SET sync = ?2 WHERE id = ?1", params![id.0, sync])?;
        if n == 0 {
            Err(StoreError::NoSuchJob(id.0))
        } else {
            Ok(())
        }
    }

    /// Jobs that were rendering or paused when the app stopped go back to Waiting with `note` (ADR-9, PyCutRv bug 10).
    pub fn recover_interrupted(&mut self, note: &str) -> Result<Vec<JobId>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE state IN ('rendering', 'paused') ORDER BY position")?;
        let ids: Vec<JobId> = stmt.query_map([], |r| r.get(0).map(JobId))?.collect::<Result<_, _>>()?;
        drop(stmt);
        for &id in &ids {
            self.set_state(id, &JobState::Waiting, Some(note))?;
        }
        Ok(ids)
    }

    /// Appends lines to a job's log.
    pub fn append_log(&mut self, id: JobId, stream: LogStream, lines: &[String]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let now = now_millis();
            let mut stmt = tx.prepare("INSERT INTO job_logs (job_id, at, stream, line) VALUES (?1, ?2, ?3, ?4)")?;
            for line in lines {
                stmt.execute(params![id.0, now, stream.as_str(), line])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The last `limit` lines of a job's log, oldest first.
    pub fn log(&self, id: JobId, limit: usize) -> Result<Vec<LogLine>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT at, stream, line FROM (SELECT id, at, stream, line FROM job_logs WHERE job_id = ?1 ORDER BY id DESC LIMIT ?2) \
             ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id.0, limit], |r| {
            Ok(LogLine {
                at: r.get(0)?,
                stream: LogStream::parse(&r.get::<_, String>(1)?),
                line: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::sample_spec;

    fn waiting() -> JobState {
        JobState::Waiting
    }

    #[test]
    fn insert_read_update() {
        let mut store = Store::open_in_memory().unwrap();
        let spec = sample_spec();
        let a = store.insert_job(&spec, &waiting()).unwrap();
        let b = store.insert_job(&spec, &waiting()).unwrap();
        assert_eq!((a.position, b.position), (1, 2));
        assert_eq!(a.spec, SpecSlot::Decoded(Box::new(spec.clone())));
        assert_eq!(a.target_path, spec.target_path());
        let mut edited = spec.clone();
        edited.target.stem = "Other".into();
        store.update_spec(a.id, &edited).unwrap();
        assert_eq!(
            store.job(a.id).unwrap().unwrap().target_path,
            PathBuf::from("/videos/out/Other.mkv")
        );
        assert!(matches!(
            store.update_spec(JobId(999), &spec),
            Err(StoreError::NoSuchJob(999))
        ));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut store = Store::open_in_memory().unwrap();
        let spec = sample_spec();
        let a = store.insert_job(&spec, &waiting()).unwrap();
        let b = store.insert_job(&spec, &waiting()).unwrap();
        store.delete_jobs(&[b.id]).unwrap();
        let c = store.insert_job(&spec, &waiting()).unwrap();
        assert!(c.id > b.id && b.id > a.id);
    }

    #[test]
    fn state_timestamps_and_attempts() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_job(&sample_spec(), &waiting()).unwrap().id;
        store
            .set_state(id, &JobState::Rendering { pass: 1, passes: 1 }, None)
            .unwrap();
        let r = store.job(id).unwrap().unwrap();
        assert!(r.started_at.is_some() && r.finished_at.is_none());
        store
            .set_state(
                id,
                &JobState::Failed {
                    reason: "exit 1".into(),
                },
                None,
            )
            .unwrap();
        let r = store.job(id).unwrap().unwrap();
        assert_eq!(
            r.state,
            JobState::Failed {
                reason: "exit 1".into()
            }
        );
        assert!(r.finished_at.is_some());
        store.set_state(id, &waiting(), Some("retry")).unwrap();
        let r = store.job(id).unwrap().unwrap();
        assert_eq!(
            (r.attempt, r.started_at, r.finished_at, r.note.as_deref()),
            (2, None, None, Some("retry"))
        );
    }

    #[test]
    fn order_and_recovery() {
        let mut store = Store::open_in_memory().unwrap();
        let spec = sample_spec();
        let ids: Vec<JobId> = (0..4)
            .map(|_| store.insert_job(&spec, &waiting()).unwrap().id)
            .collect();
        store.set_order(&[ids[2], ids[0]]).unwrap();
        let order: Vec<JobId> = store.jobs().unwrap().iter().map(|r| r.id).collect();
        assert_eq!(order, vec![ids[2], ids[0], ids[1], ids[3]]);
        store
            .set_state(ids[1], &JobState::Rendering { pass: 1, passes: 2 }, None)
            .unwrap();
        store
            .set_state(ids[3], &JobState::Paused { pass: 2, passes: 2 }, None)
            .unwrap();
        let recovered = store.recover_interrupted("interrupted").unwrap();
        assert_eq!(recovered, vec![ids[1], ids[3]]);
        assert!(store.jobs().unwrap().iter().all(|r| r.state == JobState::Waiting));
    }

    #[test]
    fn logs_keep_order_and_limit_and_go_with_the_job() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_job(&sample_spec(), &waiting()).unwrap().id;
        let lines: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
        store.append_log(id, LogStream::Ffmpeg, &lines).unwrap();
        let tail = store.log(id, 3).unwrap();
        assert_eq!(
            tail.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(),
            vec!["line 7", "line 8", "line 9"]
        );
        assert!(tail.iter().all(|l| l.stream == LogStream::Ffmpeg));
        store.delete_jobs(&[id]).unwrap();
        assert!(store.log(id, 10).unwrap().is_empty());
    }

    #[test]
    fn unreadable_specs_are_kept_as_they_are() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.insert_job(&sample_spec(), &waiting()).unwrap().id;
        store
            .conn
            .execute("UPDATE jobs SET spec_version = 7 WHERE id = ?1", [id.0])
            .unwrap();
        assert!(matches!(
            store.job(id).unwrap().unwrap().spec,
            SpecSlot::Unreadable { version: 7, .. }
        ));
        store
            .conn
            .execute("UPDATE jobs SET spec = '{', spec_version = 1 WHERE id = ?1", [id.0])
            .unwrap();
        assert!(matches!(
            store.job(id).unwrap().unwrap().spec,
            SpecSlot::Unreadable { version: 1, .. }
        ));
    }

    #[test]
    fn a_database_on_disk_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        let id = {
            let mut store = Store::open(&path, &dir.path().join("backups")).unwrap();
            store.insert_job(&sample_spec(), &waiting()).unwrap().id
        };
        let store = Store::open(&path, &dir.path().join("backups")).unwrap();
        assert_eq!(store.job(id).unwrap().map(|r| r.id), Some(id));
        assert_eq!(store.path(), Some(path));
    }
}
