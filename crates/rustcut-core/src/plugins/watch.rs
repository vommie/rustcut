//! Development folders are reloaded when their manifest or their build output changes.

use std::path::PathBuf;

use notify::{RecursiveMode, Watcher};

use super::PluginCommand;
use crate::actor::Handle;

/// What is watched for one development plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Watched {
    pub id: String,
    pub files: Vec<PathBuf>,
}

/// Keeps the watcher alive.
pub(super) struct DevWatch {
    _watcher: notify::RecommendedWatcher,
}

/// Watches the folders that hold the files; a change of one of them sends [`PluginCommand::Touched`].
pub(super) fn watch(plugins: Vec<Watched>, actor: Handle<PluginCommand>) -> Option<DevWatch> {
    if plugins.is_empty() {
        return None;
    }
    let targets = plugins.clone();
    let handler = move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if !(event.kind.is_create() || event.kind.is_modify()) {
            return;
        }
        for plugin in &targets {
            if event
                .paths
                .iter()
                .any(|path| plugin.files.iter().any(|file| file == path))
            {
                let _ = actor.send(PluginCommand::Touched { id: plugin.id.clone() });
            }
        }
    };
    let mut watcher = match notify::recommended_watcher(handler) {
        Ok(watcher) => watcher,
        Err(err) => {
            tracing::warn!("[Plugins] development folders are not watched: {err}");
            return None;
        }
    };
    let mut folders: Vec<PathBuf> = plugins
        .iter()
        .flat_map(|p| p.files.iter().filter_map(|f| f.parent().map(PathBuf::from)))
        .collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        if let Err(err) = watcher.watch(&folder, RecursiveMode::NonRecursive) {
            tracing::warn!("[Plugins] cannot watch {}: {err}", folder.display());
        }
    }
    Some(DevWatch { _watcher: watcher })
}
