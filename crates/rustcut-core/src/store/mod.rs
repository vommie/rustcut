//! The SQLite store of the queue, the job history and the job logs (ADR-5). [`Store`] is the synchronous core; one dedicated
//! thread owns it ([`actor`]) and everything else talks to that thread through a [`StoreHandle`].

mod actor;
mod jobs;
mod schema;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub use actor::{StoreCommand, StoreHandle, spawn_store};
pub use jobs::{JobRecord, LogLine, LogStream, SpecSlot};
pub use schema::SCHEMA_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error(
        "the database was written by a newer RustCut (schema {found}, this version knows {known}); it is left untouched"
    )]
    NewerSchema { found: usize, known: usize },
    #[error("the database could not be backed up before the migration, so it was not migrated: {0}")]
    Backup(String),
    #[error("job {0} does not exist")]
    NoSuchJob(i64),
    #[error("a job could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
}

/// The open database.
pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.conn.path()).finish()
    }
}

impl Store {
    /// Opens (or creates) the database at `path` and migrates it to [`SCHEMA_VERSION`]. An existing database that needs a
    /// migration is copied into `backup_dir` first; when the copy fails, nothing is migrated.
    pub fn open(path: &Path, backup_dir: &Path) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        schema::migrate(&mut conn, Some((path, backup_dir)))?;
        tracing::info!(schema = SCHEMA_VERSION, "[Store] Database ready");
        Ok(Self { conn })
    }

    /// A private in-memory database (tests, `--home` dry runs).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut conn = Connection::open_in_memory()?;
        configure(&conn)?;
        schema::migrate(&mut conn, None)?;
        Ok(Self { conn })
    }

    /// File of the database (`None` in memory).
    pub fn path(&self) -> Option<PathBuf> {
        self.conn.path().filter(|p| !p.is_empty()).map(PathBuf::from)
    }
}

fn configure(conn: &Connection) -> Result<(), rusqlite::Error> {
    // WAL: readers never block the writer; NORMAL sync is safe with WAL (a power cut loses at most the last commit).
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}
