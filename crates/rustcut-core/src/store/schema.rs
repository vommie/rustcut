//! Versioned schema (rusqlite_migration, `PRAGMA user_version`). A migration is never edited once released; changes are new
//! steps with a test that feeds them a database of the previous version.

use std::path::Path;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::StoreError;

/// Schema version this build writes.
pub const SCHEMA_VERSION: usize = 1;

const V1: &str = r"
CREATE TABLE jobs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    position      INTEGER NOT NULL,
    state         TEXT    NOT NULL,
    state_detail  TEXT    NOT NULL,
    attempt       INTEGER NOT NULL DEFAULT 1,
    created_at    INTEGER NOT NULL,
    started_at    INTEGER,
    finished_at   INTEGER,
    source_path   TEXT    NOT NULL,
    target_path   TEXT    NOT NULL,
    note          TEXT,
    spec_version  INTEGER NOT NULL,
    spec          TEXT    NOT NULL,
    sync          TEXT
) STRICT;
CREATE INDEX jobs_state ON jobs (state);
CREATE INDEX jobs_position ON jobs (position);
CREATE INDEX jobs_target ON jobs (target_path);
CREATE INDEX jobs_source ON jobs (source_path);

CREATE TABLE job_logs (
    id      INTEGER PRIMARY KEY,
    job_id  INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    at      INTEGER NOT NULL,
    stream  TEXT    NOT NULL,
    line    TEXT    NOT NULL
) STRICT;
CREATE INDEX job_logs_job ON job_logs (job_id, id);
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1)])
}

/// Brings the database to [`SCHEMA_VERSION`]. `file` = (database path, backup folder) for databases on disk.
pub(super) fn migrate(conn: &mut Connection, file: Option<(&Path, &Path)>) -> Result<(), StoreError> {
    let migrations = migrations();
    let current = match migrations.current_version(conn)? {
        SchemaVersion::NoneSet => 0,
        SchemaVersion::Inside(v) => v.get(),
        SchemaVersion::Outside(v) => {
            return Err(StoreError::NewerSchema {
                found: v.get(),
                known: SCHEMA_VERSION,
            });
        }
    };
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0
        && let Some((path, backup_dir)) = file
    {
        backup(conn, path, backup_dir, current)?;
    }
    migrations.to_latest(conn)?;
    tracing::info!(from = current, to = SCHEMA_VERSION, "[Store] Schema migrated");
    Ok(())
}

/// Copies the live database (SQLite online backup, consistent with WAL) to `<backup_dir>/<file>.v<from>.<time>.bak`.
fn backup(conn: &Connection, path: &Path, backup_dir: &Path, from: usize) -> Result<(), StoreError> {
    let name = path
        .file_name()
        .map_or_else(|| "queue.sqlite".into(), |n| n.to_string_lossy().into_owned());
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let target = backup_dir.join(format!("{name}.v{from}.{stamp}.bak"));
    std::fs::create_dir_all(backup_dir).map_err(|e| StoreError::Backup(e.to_string()))?;
    conn.backup(rusqlite::MAIN_DB, &target, None)
        .map_err(|e| StoreError::Backup(e.to_string()))?;
    tracing::info!(backup = %target.display(), "[Store] Database backed up before the migration");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }

    #[test]
    fn a_newer_database_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", 99).unwrap();
        }
        let mut conn = Connection::open(&path).unwrap();
        let err = migrate(&mut conn, Some((&path, dir.path()))).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::NewerSchema {
                    found: 99,
                    known: SCHEMA_VERSION
                }
            ),
            "{err}"
        );
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, 99);
    }

    #[test]
    fn up_to_date_databases_are_not_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        let backups = dir.path().join("backups");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, Some((&path, &backups))).unwrap();
        migrate(&mut conn, Some((&path, &backups))).unwrap();
        assert!(
            !backups.exists(),
            "a fresh database needs no backup, an up-to-date one neither"
        );
    }

    #[test]
    fn backup_copies_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (42);")
            .unwrap();
        backup(&conn, &path, &dir.path().join("backups"), 1).unwrap();
        let copies: Vec<_> = std::fs::read_dir(dir.path().join("backups")).unwrap().collect();
        assert_eq!(copies.len(), 1);
        let copy = Connection::open(copies[0].as_ref().unwrap().path()).unwrap();
        let x: i64 = copy.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 42);
    }
}
