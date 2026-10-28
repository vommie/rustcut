//! The picture store (P11.2, user decision Q-FP2): its own SQLite file `<data>/pictures.sqlite` (not the queue's),
//! one row per picture keyed by the BLAKE3 hash of its bytes. Pictures nobody uses are removed by [`PictureStore::retain`]
//! (a picture added in the last minutes stays: the Settings page may not have set it yet).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _, params};
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::import::{PictureError, PictureKind, Prepared, prepare};

/// Schema version this build writes.
pub const SCHEMA_VERSION: usize = 1;

/// A picture this young is never removed by `retain` (seconds).
pub const KEEP_NEW_SECS: i64 = 600;

const V1: &str = r"
CREATE TABLE pictures (
    hash    TEXT    PRIMARY KEY,
    kind    TEXT    NOT NULL,
    mono    INTEGER NOT NULL,
    width   INTEGER NOT NULL,
    height  INTEGER NOT NULL,
    added   INTEGER NOT NULL,
    data    BLOB    NOT NULL
) STRICT;
";

/// A stored picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPicture {
    pub hash: String,
    pub kind: PictureKind,
    pub data: Vec<u8>,
}

pub struct PictureStore {
    conn: Connection,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for PictureStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn store_error(e: impl std::fmt::Display) -> PictureError {
    PictureError::Store(e.to_string())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl PictureStore {
    /// Opens (or makes) the file; a newer schema is refused and left alone, an older one is backed up into `backups`
    /// first.
    pub fn open(path: &Path, backups: &Path) -> Result<Self, PictureError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(store_error)?;
        }
        let mut conn = Connection::open(path).map_err(store_error)?;
        migrate(&mut conn, Some(backups))?;
        Ok(Self {
            conn,
            path: Some(path.to_path_buf()),
        })
    }

    pub fn open_in_memory() -> Result<Self, PictureError> {
        let mut conn = Connection::open_in_memory().map_err(store_error)?;
        migrate(&mut conn, None)?;
        Ok(Self { conn, path: None })
    }

    /// Prepares `data` and stores it (a picture with the same hash is stored once).
    pub fn import(&self, data: &[u8]) -> Result<StoredPicture, PictureError> {
        let prepared = prepare(data)?;
        self.insert(&prepared)?;
        Ok(StoredPicture {
            hash: prepared.hash,
            kind: prepared.kind,
            data: prepared.data,
        })
    }

    fn insert(&self, p: &Prepared) -> Result<(), PictureError> {
        let mono = matches!(p.kind, PictureKind::Svg { mono: true });
        self.conn
            .execute(
                "INSERT INTO pictures (hash, kind, mono, width, height, added, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (hash) DO UPDATE SET added = excluded.added",
                params![p.hash, p.kind.id(), mono, p.width, p.height, now(), p.data],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn get(&self, hash: &str) -> Result<Option<StoredPicture>, PictureError> {
        self.conn
            .query_row(
                "SELECT kind, mono, data FROM pictures WHERE hash = ?1",
                params![hash],
                |row| {
                    let kind: String = row.get(0)?;
                    let mono: bool = row.get(1)?;
                    let data: Vec<u8> = row.get(2)?;
                    Ok((kind, mono, data))
                },
            )
            .optional()
            .map_err(store_error)
            .map(|found| {
                found.and_then(|(kind, mono, data)| {
                    let kind = match kind.as_str() {
                        "svg" => PictureKind::Svg { mono },
                        "webp" => PictureKind::WebP,
                        // A kind a newer RustCut wrote: not shown here.
                        _ => return None,
                    };
                    Some(StoredPicture {
                        hash: hash.to_owned(),
                        kind,
                        data,
                    })
                })
            })
    }

    /// Removes every picture not in `keep` that is older than [`KEEP_NEW_SECS`]; returns how many went.
    pub fn retain(&self, keep: &HashSet<String>) -> Result<usize, PictureError> {
        self.retain_at(keep, now())
    }

    pub(super) fn retain_at(&self, keep: &HashSet<String>, at: i64) -> Result<usize, PictureError> {
        let mut statement = self
            .conn
            .prepare("SELECT hash FROM pictures WHERE added < ?1")
            .map_err(store_error)?;
        let old: Vec<String> = statement
            .query_map(params![at.saturating_sub(KEEP_NEW_SECS)], |row| row.get(0))
            .map_err(store_error)?
            .collect::<Result<_, _>>()
            .map_err(store_error)?;
        let mut removed = 0;
        for hash in old.iter().filter(|h| !keep.contains(*h)) {
            removed += self
                .conn
                .execute("DELETE FROM pictures WHERE hash = ?1", params![hash])
                .map_err(store_error)?;
        }
        Ok(removed)
    }

    pub fn count(&self) -> Result<usize, PictureError> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM pictures", [], |row| row.get(0))
            .map_err(store_error)?;
        Ok(usize::try_from(count).unwrap_or_default())
    }
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1)])
}

/// Brings the database to [`SCHEMA_VERSION`]; `backups` = the backup folder of a file on disk.
fn migrate(conn: &mut Connection, backups: Option<&Path>) -> Result<(), PictureError> {
    let migrations = migrations();
    let current = match migrations.current_version(conn).map_err(store_error)? {
        SchemaVersion::NoneSet => 0,
        SchemaVersion::Inside(v) => v.get(),
        SchemaVersion::Outside(v) => {
            return Err(PictureError::Store(format!(
                "pictures.sqlite is from a newer RustCut (schema {}, this one knows {SCHEMA_VERSION})",
                v.get()
            )));
        }
    };
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0
        && let Some(backups) = backups
    {
        std::fs::create_dir_all(backups).map_err(store_error)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let target = backups.join(format!("pictures.sqlite.v{current}.{stamp}.bak"));
        conn.backup(rusqlite::MAIN_DB, &target, None).map_err(store_error)?;
        tracing::info!(backup = %target.display(), "[Pictures] Database backed up before the migration");
    }
    migrations.to_latest(conn).map_err(store_error)?;
    Ok(())
}
