//! Where RustCut keeps its files: the XDG folders, or one folder per kind below `--home <dir>` (test environment).

use std::path::{Path, PathBuf};

use crate::app_identity::BINARY_NAME;

/// The folders of one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// `~/.config/rustcut`: `config.toml`, `ui-state.toml`, `backups/`.
    pub config_dir: PathBuf,
    /// `~/.local/share/rustcut`: `queue.sqlite`, `deshake/`, `passlog/`, `backups/`.
    pub data_dir: PathBuf,
    /// `~/.local/state/rustcut`: `logs/`.
    pub state_dir: PathBuf,
    /// `~/.cache/rustcut`: thumbnails and other things that can be rebuilt.
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// The XDG folders of the user, or the folders below `home` when given. `None` when no home directory is known.
    pub fn resolve(home: Option<&Path>) -> Option<Self> {
        if let Some(home) = home {
            return Some(Self::below(home));
        }
        let dirs = directories::ProjectDirs::from("", "", BINARY_NAME)?;
        let state = dirs
            .state_dir()
            .map_or_else(|| dirs.data_local_dir().join("state"), Path::to_path_buf);
        Some(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
            state_dir: state,
            cache_dir: dirs.cache_dir().to_path_buf(),
        })
    }

    /// `home/config`, `home/data`, `home/state`, `home/cache`.
    pub fn below(home: &Path) -> Self {
        Self {
            config_dir: home.join("config"),
            data_dir: home.join("data"),
            state_dir: home.join("state"),
            cache_dir: home.join("cache"),
        }
    }

    /// Creates the folders (explicit I/O at startup).
    pub fn create_all(&self) -> std::io::Result<()> {
        for dir in [&self.config_dir, &self.data_dir, &self.state_dir, &self.cache_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn database(&self) -> PathBuf {
        self.data_dir.join("queue.sqlite")
    }

    /// The timeline thumbnails of job sources: a cache that can always be made again.
    pub fn thumbs_database(&self) -> PathBuf {
        self.cache_dir.join("tl_thumbs.db")
    }

    pub fn config_backups(&self) -> PathBuf {
        self.config_dir.join("backups")
    }

    pub fn data_backups(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// vid.stab transform files (`job-<id>.trf`).
    pub fn deshake_dir(&self) -> PathBuf {
        self.data_dir.join("deshake")
    }

    /// Two-pass statistics (`job-<id>`).
    pub fn passlog_dir(&self) -> PathBuf {
        self.data_dir.join("passlog")
    }

    /// Installed plugins, one folder per plugin id (P12 block 8).
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    /// The compiled code of plugins: rebuilt from the plugins when missing.
    pub fn plugin_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("plugins")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_puts_everything_below_it() {
        let p = AppPaths::resolve(Some(Path::new("/t/profile"))).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/t/profile/config"));
        assert_eq!(p.database(), PathBuf::from("/t/profile/data/queue.sqlite"));
        assert_eq!(p.log_dir(), PathBuf::from("/t/profile/state/logs"));
        for dir in [
            p.config_backups(),
            p.data_backups(),
            p.deshake_dir(),
            p.passlog_dir(),
            p.cache_dir,
        ] {
            assert!(dir.starts_with("/t/profile"));
        }
    }

    #[test]
    fn xdg_folders_end_in_the_binary_name() {
        if let Some(p) = AppPaths::resolve(None) {
            assert!(p.config_dir.ends_with(BINARY_NAME));
            assert!(p.data_dir.ends_with(BINARY_NAME));
            assert!(p.state_dir.ends_with(BINARY_NAME) || p.state_dir.ends_with("state"));
        }
    }

    #[test]
    fn create_all_makes_the_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = AppPaths::below(dir.path());
        p.create_all().unwrap();
        assert!(p.config_dir.is_dir() && p.data_dir.is_dir() && p.state_dir.is_dir() && p.cache_dir.is_dir());
    }
}
