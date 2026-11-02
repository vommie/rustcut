//! Finding plugin folders: the installed ones and the development folders.

use std::path::{Path, PathBuf};

use rustcut_plugin_host::manifest::Manifest;

use super::Source;

/// A plugin folder and what its manifest says.
#[derive(Debug, Clone)]
pub(super) struct Found {
    pub id: String,
    pub dir: PathBuf,
    pub source: Source,
    pub manifest: Result<Manifest, String>,
}

/// Every installed plugin (folders below `installed` not starting with a dot) and every development folder.
pub(super) fn scan(installed: &Path, dev_folders: &[PathBuf]) -> Vec<Found> {
    let mut found = Vec::new();
    match std::fs::read_dir(installed) {
        Ok(entries) => {
            let mut dirs: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && !hidden(path))
                .collect();
            dirs.sort();
            found.extend(dirs.into_iter().map(|dir| read(dir, Source::Installed)));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!("[Plugins] cannot read {}: {err}", installed.display()),
    }
    found.extend(dev_folders.iter().map(|dir| read(dir.clone(), Source::Development)));
    found
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

/// Reads one folder's manifest; the id is the folder's name when it cannot be read.
pub(super) fn read(dir: PathBuf, source: Source) -> Found {
    let manifest = Manifest::read(&dir).map_err(|err| err.to_string());
    let id = match &manifest {
        Ok(manifest) => manifest.id.clone(),
        Err(_) => dir
            .file_name()
            .map_or_else(|| "plugin".to_owned(), |name| name.to_string_lossy().into_owned()),
    };
    if let Ok(manifest) = &manifest
        && source == Source::Installed
        && dir.file_name().is_some_and(|name| name != manifest.id.as_str())
    {
        return Found {
            id,
            dir,
            source,
            manifest: Err(format!("the folder's name is not the plugin's id \"{}\"", manifest.id)),
        };
    }
    Found {
        id,
        dir,
        source,
        manifest,
    }
}
