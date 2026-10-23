//! Versioned TOML files: load with migration (backup first), sanitize, save atomically. A file written by a newer RustCut
//! is read as far as it is understood and never overwritten; a damaged file is backed up and replaced by defaults.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// How many backups of one file are kept (the newest).
const KEEP_BACKUPS: usize = 10;

/// A settings-like file with a format version.
pub trait VersionedFile: Serialize + DeserializeOwned + Default {
    /// File name inside the config folder (`config.toml`).
    const FILE_NAME: &'static str;
    /// Format version this build writes (the `version` key).
    const VERSION: u32;

    /// Migration steps, each from `from` to `from + 1`, applied in order to the raw table.
    fn steps() -> &'static [MigrationStep] {
        &[]
    }

    /// Brings values into range after loading; returns a note per change (logged).
    fn sanitize(&mut self) -> Vec<String>;
}

/// One migration of a raw TOML table from version `from` to `from + 1`.
#[derive(Debug, Clone, Copy)]
pub struct MigrationStep {
    pub from: u32,
    /// Short reason, part of the backup file name (`rename-queue-keys`).
    pub reason: &'static str,
    pub apply: fn(&mut toml::Table) -> Result<(), String>,
}

/// What [`load`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadOutcome<T> {
    /// No file yet: defaults.
    Fresh(T),
    Loaded {
        value: T,
        notes: Vec<String>,
    },
    /// Migrated from `from`; the old file is in `backup`. The caller saves the new form.
    Migrated {
        value: T,
        from: u32,
        backup: PathBuf,
        notes: Vec<String>,
    },
    /// Written by a newer RustCut (`found`): read as far as known, and must not be saved over (`read_only`).
    Newer {
        value: T,
        found: u32,
    },
    /// Unreadable: backed up to `backup` (when possible), defaults in use.
    Damaged {
        value: T,
        error: String,
        backup: Option<PathBuf>,
    },
}

impl<T> LoadOutcome<T> {
    pub fn value(&self) -> &T {
        match self {
            Self::Fresh(v) | Self::Loaded { value: v, .. } | Self::Migrated { value: v, .. } => v,
            Self::Newer { value, .. } | Self::Damaged { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        match self {
            Self::Fresh(v) | Self::Loaded { value: v, .. } | Self::Migrated { value: v, .. } => v,
            Self::Newer { value, .. } | Self::Damaged { value, .. } => value,
        }
    }

    /// Saving would destroy data of a newer version.
    pub const fn is_read_only(&self) -> bool {
        matches!(self, Self::Newer { .. })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{0} could not be encoded: {1}")]
    Encode(&'static str, String),
    #[error("{file} could not be backed up before the migration, so it was not migrated: {error}")]
    Backup { file: &'static str, error: String },
    #[error("{file}: migration from version {from} failed: {error}")]
    Migration {
        file: &'static str,
        from: u32,
        error: String,
    },
}

/// Reads `dir/T::FILE_NAME`. Errors only when a needed backup or migration fails; everything else is an outcome.
pub fn load<T: VersionedFile>(dir: &Path, backup_dir: &Path) -> Result<LoadOutcome<T>, FileError> {
    let path = dir.join(T::FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LoadOutcome::Fresh(T::default())),
        Err(source) => return Err(FileError::Io { path, source }),
    };
    let mut table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return Ok(damaged::<T>(&path, backup_dir, e.to_string())),
    };
    let found = table
        .get("version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(1);
    if found > T::VERSION {
        let value = decode::<T>(table).unwrap_or_default();
        tracing::warn!(
            file = T::FILE_NAME,
            found,
            known = T::VERSION,
            "[Settings] File is from a newer version; not saved"
        );
        return Ok(LoadOutcome::Newer { value, found });
    }
    let mut backup = None;
    if found < T::VERSION {
        let reasons: Vec<&str> = T::steps()
            .iter()
            .filter(|s| s.from >= found)
            .map(|s| s.reason)
            .collect();
        let reason = if reasons.is_empty() {
            "migration".to_owned()
        } else {
            reasons.join("+")
        };
        backup = Some(backup_file(&path, backup_dir, &reason).map_err(|e| FileError::Backup {
            file: T::FILE_NAME,
            error: e.to_string(),
        })?);
        for step in T::steps().iter().filter(|s| s.from >= found && s.from < T::VERSION) {
            (step.apply)(&mut table).map_err(|error| FileError::Migration {
                file: T::FILE_NAME,
                from: step.from,
                error,
            })?;
        }
        table.insert("version".into(), toml::Value::Integer(i64::from(T::VERSION)));
    }
    let mut value = match decode::<T>(table) {
        Ok(v) => v,
        Err(e) => return Ok(damaged::<T>(&path, backup_dir, e)),
    };
    let notes = value.sanitize();
    for note in &notes {
        tracing::info!(file = T::FILE_NAME, note = %note, "[Settings] Value corrected");
    }
    Ok(match backup {
        Some(backup) => {
            tracing::info!(
                file = T::FILE_NAME,
                from = found,
                to = T::VERSION,
                "[Settings] Migrated"
            );
            LoadOutcome::Migrated {
                value,
                from: found,
                backup,
                notes,
            }
        }
        None => LoadOutcome::Loaded { value, notes },
    })
}

/// Writes `value` to `dir/T::FILE_NAME` atomically (temporary file + rename).
pub fn save<T: VersionedFile>(dir: &Path, value: &T) -> Result<(), FileError> {
    let path = dir.join(T::FILE_NAME);
    let io = |source| FileError::Io {
        path: path.clone(),
        source,
    };
    let mut table = toml::Table::try_from(value).map_err(|e| FileError::Encode(T::FILE_NAME, e.to_string()))?;
    table.insert("version".into(), toml::Value::Integer(i64::from(T::VERSION)));
    let text = toml::to_string_pretty(&table).map_err(|e| FileError::Encode(T::FILE_NAME, e.to_string()))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut file = atomic_write_file::AtomicWriteFile::open(&path).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.commit().map_err(io)?;
    Ok(())
}

/// Copies `path` to `backup_dir/<name>.<reason>.<time>.bak` and keeps the newest [`KEEP_BACKUPS`] of that file.
pub fn backup_file(path: &Path, backup_dir: &Path, reason: &str) -> std::io::Result<PathBuf> {
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    std::fs::create_dir_all(backup_dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let target = backup_dir.join(format!("{name}.{reason}.{stamp}.bak"));
    std::fs::copy(path, &target)?;
    prune_backups(backup_dir, &name);
    tracing::info!(backup = %target.display(), "[Settings] Backup written");
    Ok(target)
}

fn prune_backups(backup_dir: &Path, name: &str) {
    let prefix = format!("{name}.");
    let Ok(entries) = std::fs::read_dir(backup_dir) else {
        return;
    };
    let mut backups: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let ours = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix));
            ours && p.extension().is_some_and(|e| e == "bak")
        })
        .collect();
    // The time stamp sorts chronologically within the same reason; sort by modification time to mix reasons correctly.
    backups.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let excess = backups.len().saturating_sub(KEEP_BACKUPS);
    for old in backups.into_iter().take(excess) {
        if let Err(e) = std::fs::remove_file(&old) {
            tracing::warn!(backup = %old.display(), error = %e, "[Settings] Old backup could not be removed");
        }
    }
}

fn decode<T: VersionedFile>(table: toml::Table) -> Result<T, String> {
    toml::Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| e.to_string())
}

fn damaged<T: VersionedFile>(path: &Path, backup_dir: &Path, error: String) -> LoadOutcome<T> {
    tracing::warn!(file = T::FILE_NAME, error = %error, "[Settings] File is damaged; defaults are used");
    let backup = backup_file(path, backup_dir, "damaged").ok();
    LoadOutcome::Damaged {
        value: T::default(),
        error,
        backup,
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Demo {
        workers: u8,
        name: String,
    }

    impl VersionedFile for Demo {
        const FILE_NAME: &'static str = "demo.toml";
        const VERSION: u32 = 3;

        fn steps() -> &'static [MigrationStep] {
            &[
                MigrationStep {
                    from: 1,
                    reason: "rename-threads",
                    apply: |t| {
                        if let Some(v) = t.remove("threads") {
                            t.insert("workers".into(), v);
                        }
                        Ok(())
                    },
                },
                MigrationStep {
                    from: 2,
                    reason: "title-to-name",
                    apply: |t| {
                        if let Some(v) = t.remove("title") {
                            t.insert("name".into(), v);
                        }
                        Ok(())
                    },
                },
            ]
        }

        fn sanitize(&mut self) -> Vec<String> {
            if self.workers == 0 || self.workers > 8 {
                self.workers = self.workers.clamp(1, 8);
                vec![format!("workers set to {}", self.workers)]
            } else {
                Vec::new()
            }
        }
    }

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let config = t.path().join("config");
        let backups = t.path().join("backups");
        std::fs::create_dir_all(&config).unwrap();
        (t, config, backups)
    }

    #[test]
    fn missing_file_gives_defaults_and_save_round_trips() {
        let (_t, config, backups) = dirs();
        assert_eq!(
            load::<Demo>(&config, &backups).unwrap(),
            LoadOutcome::Fresh(Demo::default())
        );
        let demo = Demo {
            workers: 2,
            name: "x".into(),
        };
        save(&config, &demo).unwrap();
        let text = std::fs::read_to_string(config.join("demo.toml")).unwrap();
        assert!(text.contains("version = 3"), "{text}");
        assert_eq!(
            load::<Demo>(&config, &backups).unwrap(),
            LoadOutcome::Loaded {
                value: demo,
                notes: vec![]
            }
        );
        assert!(!backups.exists());
    }

    #[test]
    fn old_files_are_backed_up_then_migrated() {
        let (_t, config, backups) = dirs();
        std::fs::write(config.join("demo.toml"), "version = 1\nthreads = 4\ntitle = \"old\"\n").unwrap();
        let LoadOutcome::Migrated {
            value, from, backup, ..
        } = load::<Demo>(&config, &backups).unwrap()
        else {
            panic!("expected a migration")
        };
        assert_eq!(
            (value, from),
            (
                Demo {
                    workers: 4,
                    name: "old".into()
                },
                1
            )
        );
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("demo.toml.rename-threads+title-to-name.")
        );
        assert_eq!(
            std::fs::read_to_string(backup).unwrap(),
            "version = 1\nthreads = 4\ntitle = \"old\"\n"
        );
    }

    #[test]
    fn newer_files_are_read_but_read_only() {
        let (_t, config, backups) = dirs();
        std::fs::write(config.join("demo.toml"), "version = 9\nworkers = 3\nfuture = true\n").unwrap();
        let outcome = load::<Demo>(&config, &backups).unwrap();
        assert!(outcome.is_read_only());
        assert_eq!(outcome.value().workers, 3);
    }

    #[test]
    fn damaged_files_are_kept_and_replaced_by_defaults() {
        let (_t, config, backups) = dirs();
        std::fs::write(config.join("demo.toml"), "workers = [").unwrap();
        let LoadOutcome::Damaged { value, backup, .. } = load::<Demo>(&config, &backups).unwrap() else {
            panic!("expected damage")
        };
        assert_eq!(value, Demo::default());
        assert_eq!(std::fs::read_to_string(backup.unwrap()).unwrap(), "workers = [");
        std::fs::write(config.join("demo.toml"), "version = 3\nworkers = \"many\"\n").unwrap();
        assert!(matches!(
            load::<Demo>(&config, &backups).unwrap(),
            LoadOutcome::Damaged { .. }
        ));
    }

    #[test]
    fn values_are_sanitized_and_unknown_keys_ignored() {
        let (_t, config, backups) = dirs();
        std::fs::write(config.join("demo.toml"), "version = 3\nworkers = 50\nremoved_key = 1\n").unwrap();
        let LoadOutcome::Loaded { value, notes } = load::<Demo>(&config, &backups).unwrap() else {
            panic!()
        };
        assert_eq!(value.workers, 8);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let (_t, config, backups) = dirs();
        let path = config.join("demo.toml");
        std::fs::write(&path, "x").unwrap();
        for i in 0..(KEEP_BACKUPS + 3) {
            backup_file(&path, &backups, &format!("r{i}")).unwrap();
        }
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), KEEP_BACKUPS);
    }
}
