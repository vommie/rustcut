//! `tl_thumbs.db`, the timeline thumbnails of job sources (user decision Q-P12e). Purely job-based: a source's row goes
//! when its last job goes, no limit and no age, and the whole file empties when no job is left. A file is known by its path
//! and nothing else. The thread here is the only owner of the database (ADR-4).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::set::Thumb;
use crate::actor::{Handle, Reply, mailbox};

const SCHEMA_VERSION: usize = 1;

const V1: &str = r"
CREATE TABLE sources (
    id      INTEGER PRIMARY KEY,
    path    BLOB    NOT NULL UNIQUE
) STRICT;
CREATE TABLE thumbs (
    source  INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    time_ms INTEGER NOT NULL,
    exact   INTEGER NOT NULL,
    width   INTEGER NOT NULL,
    height  INTEGER NOT NULL,
    data    BLOB    NOT NULL,
    PRIMARY KEY (source, time_ms)
) STRICT;
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1)])
}

#[derive(Debug, thiserror::Error)]
pub enum ThumbStoreError {
    #[error("tl_thumbs.db is from a newer RustCut (schema {found}, this one knows {known})")]
    NewerSchema { found: usize, known: usize },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] rusqlite_migration::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// What the Settings tooltip shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThumbStats {
    pub sources: u64,
    pub thumbs: u64,
    pub bytes: u64,
}

pub struct ThumbStore {
    conn: Connection,
}

fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes()
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

impl ThumbStore {
    /// Opens (or makes) the file. A newer schema is refused and left alone, an older one is backed up into `backups` first.
    pub fn open(path: &Path, backups: &Path) -> Result<Self, ThumbStoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path)?;
        migrate(&mut conn, Some(backups))?;
        Self::ready(conn)
    }

    pub fn open_in_memory() -> Result<Self, ThumbStoreError> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn, None)?;
        Self::ready(conn)
    }

    fn ready(conn: Connection) -> Result<Self, ThumbStoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // A lost write costs a thumbnail, never a job.
        conn.pragma_update(None, "synchronous", "OFF")?;
        Ok(Self { conn })
    }

    pub fn load(&self, source: &Path) -> Result<Vec<Thumb>, ThumbStoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT time_ms, exact, width, height, data FROM thumbs
             WHERE source = (SELECT id FROM sources WHERE path = ?1) ORDER BY time_ms",
        )?;
        let rows = statement.query_map(params![path_bytes(source)], |row| {
            Ok(Thumb {
                time_ms: unsigned(row.get(0)?),
                exact: row.get(1)?,
                width: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                height: u32::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                webp: Arc::from(row.get::<_, Vec<u8>>(4)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn put(&mut self, source: &Path, thumbs: &[Thumb]) -> Result<(), ThumbStoreError> {
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO sources (path) VALUES (?1)",
            params![path_bytes(source)],
        )?;
        let id: i64 = transaction.query_row(
            "SELECT id FROM sources WHERE path = ?1",
            params![path_bytes(source)],
            |row| row.get(0),
        )?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT OR REPLACE INTO thumbs (source, time_ms, exact, width, height, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for thumb in thumbs {
                statement.execute(params![
                    id,
                    signed(thumb.time_ms),
                    thumb.exact,
                    thumb.width,
                    thumb.height,
                    thumb.webp.as_ref()
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Removes every source that is not in `keep`; the number removed.
    pub fn retain(&mut self, keep: &HashSet<PathBuf>) -> Result<usize, ThumbStoreError> {
        let known: Vec<(i64, Vec<u8>)> = {
            let mut statement = self.conn.prepare("SELECT id, path FROM sources")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let keep: HashSet<&[u8]> = keep.iter().map(|path| path_bytes(path)).collect();
        let mut removed = 0;
        for (id, path) in known {
            if !keep.contains(path.as_slice()) {
                self.conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
                removed += 1;
            }
        }
        if removed > 0 {
            self.conn.execute_batch("VACUUM")?;
        }
        Ok(removed)
    }

    /// Empties everything, orphans included, and gives the space back.
    pub fn wipe(&self) -> Result<(), ThumbStoreError> {
        self.conn.execute_batch("DELETE FROM thumbs; DELETE FROM sources; VACUUM;")?;
        Ok(())
    }

    pub fn stats(&self) -> Result<ThumbStats, ThumbStoreError> {
        let count = |sql: &str| self.conn.query_row(sql, [], |row| row.get::<_, i64>(0)).map(unsigned);
        let pages = count("PRAGMA page_count")?;
        let page_size = count("PRAGMA page_size")?;
        Ok(ThumbStats {
            sources: count("SELECT count(*) FROM sources")?,
            thumbs: count("SELECT count(*) FROM thumbs")?,
            bytes: pages.saturating_mul(page_size),
        })
    }
}

fn migrate(conn: &mut Connection, backups: Option<&Path>) -> Result<(), ThumbStoreError> {
    let migrations = migrations();
    let current = match migrations.current_version(conn)? {
        SchemaVersion::NoneSet => 0,
        SchemaVersion::Inside(version) => version.get(),
        SchemaVersion::Outside(version) => {
            return Err(ThumbStoreError::NewerSchema {
                found: version.get(),
                known: SCHEMA_VERSION,
            });
        }
    };
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0
        && let Some(backups) = backups
    {
        std::fs::create_dir_all(backups)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let target = backups.join(format!("tl_thumbs.db.v{current}.{stamp}.bak"));
        conn.backup(rusqlite::MAIN_DB, &target, None)?;
        tracing::info!(backup = %target.display(), "[Thumbs] Database backed up before the migration");
    }
    migrations.to_latest(conn)?;
    Ok(())
}

#[derive(Debug)]
pub enum ThumbStoreCommand {
    Load {
        source: PathBuf,
        reply: Reply<Vec<Thumb>>,
    },
    Put {
        source: PathBuf,
        thumbs: Vec<Thumb>,
    },
    Retain {
        keep: HashSet<PathBuf>,
    },
    Wipe,
    Stats {
        reply: Reply<ThumbStats>,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

pub type ThumbStoreHandle = Handle<ThumbStoreCommand>;

/// Starts the thread with an open store. A failing write only costs a cached picture, so errors are logged.
pub fn spawn_thumb_store(mut store: ThumbStore) -> std::io::Result<(ThumbStoreHandle, JoinHandle<()>)> {
    let (handle, mut rx) = mailbox::<ThumbStoreCommand>();
    let thread = std::thread::Builder::new()
        .name("rustcut-thumbs".into())
        .spawn(move || {
            while let Some(command) = rx.blocking_recv() {
                match command {
                    ThumbStoreCommand::Load { source, reply } => {
                        let thumbs = store.load(&source).unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "[Thumbs] Cached thumbnails not read");
                            Vec::new()
                        });
                        let _ = reply.send(thumbs);
                    }
                    ThumbStoreCommand::Put { source, thumbs } => {
                        if let Err(e) = store.put(&source, &thumbs) {
                            tracing::warn!(error = %e, "[Thumbs] Thumbnails not cached");
                        }
                    }
                    ThumbStoreCommand::Retain { keep } => match store.retain(&keep) {
                        Ok(0) => {}
                        Ok(removed) => tracing::info!(removed, "[Thumbs] Thumbnails of removed jobs deleted"),
                        Err(e) => tracing::warn!(error = %e, "[Thumbs] Thumbnails of removed jobs not deleted"),
                    },
                    ThumbStoreCommand::Wipe => match store.wipe() {
                        Ok(()) => tracing::info!("[Thumbs] Thumbnail cache emptied"),
                        Err(e) => tracing::warn!(error = %e, "[Thumbs] Thumbnail cache not emptied"),
                    },
                    ThumbStoreCommand::Stats { reply } => {
                        let _ = reply.send(store.stats().unwrap_or_default());
                    }
                    ThumbStoreCommand::Shutdown { reply } => {
                        drop(store);
                        let _ = reply.send(());
                        return;
                    }
                }
            }
        })?;
    Ok((handle, thread))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb(time_ms: u64) -> Thumb {
        Thumb {
            time_ms,
            exact: time_ms.is_multiple_of(2),
            width: 320,
            height: 180,
            webp: Arc::from(vec![1_u8; 2000]),
        }
    }

    #[test]
    fn thumbnails_come_back_as_stored() {
        let mut store = ThumbStore::open_in_memory().unwrap();
        let source = Path::new("/videos/ä.mkv");
        let thumbs = vec![thumb(100), thumb(201)];
        store.put(source, &thumbs).unwrap();
        store.put(source, &thumbs).unwrap();
        assert_eq!(store.load(source).unwrap(), thumbs);
        assert!(store.load(Path::new("/other.mkv")).unwrap().is_empty());
    }

    #[test]
    fn retain_drops_what_no_job_needs() {
        let mut store = ThumbStore::open_in_memory().unwrap();
        store.put(Path::new("/a.mkv"), &[thumb(1)]).unwrap();
        store.put(Path::new("/b.mkv"), &[thumb(1), thumb(2)]).unwrap();
        let keep = HashSet::from([PathBuf::from("/b.mkv")]);
        assert_eq!(store.retain(&keep).unwrap(), 1);
        assert!(store.load(Path::new("/a.mkv")).unwrap().is_empty());
        assert_eq!(store.load(Path::new("/b.mkv")).unwrap().len(), 2);
        assert_eq!(store.stats().unwrap().thumbs, 2);
    }

    #[test]
    fn wipe_leaves_nothing_and_gives_the_space_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tl_thumbs.db");
        let mut store = ThumbStore::open(&path, dir.path()).unwrap();
        let many: Vec<Thumb> = (0..500).map(thumb).collect();
        store.put(Path::new("/a.mkv"), &many).unwrap();
        let full = store.stats().unwrap();
        assert_eq!((full.sources, full.thumbs), (1, 500));
        store.wipe().unwrap();
        let empty = store.stats().unwrap();
        assert_eq!((empty.sources, empty.thumbs), (0, 0));
        assert!(empty.bytes < full.bytes / 10);
    }

    #[test]
    fn a_newer_database_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tl_thumbs.db");
        drop(ThumbStore::open(&path, dir.path()).unwrap());
        Connection::open(&path).unwrap().pragma_update(None, "user_version", 9).unwrap();
        assert!(matches!(
            ThumbStore::open(&path, dir.path()),
            Err(ThumbStoreError::NewerSchema { found: 9, .. })
        ));
    }
}
