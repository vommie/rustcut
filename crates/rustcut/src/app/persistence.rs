//! Loading settings and UI state at startup and saving the UI state at quit (paths from `--home` or XDG).

use rustcut_core::settings::{AppPaths, LoadOutcome, Settings, UiState, VersionedFile, load, save};

/// What startup loaded; `ui_read_only` keeps a newer version's `ui-state.toml` from being overwritten.
#[derive(Debug)]
pub struct Loaded {
    pub paths: AppPaths,
    pub settings: Settings,
    pub ui_state: UiState,
    pub ui_read_only: bool,
}

/// Resolves the folders, creates them and loads both files. Never fails: problems are logged and defaults used.
pub fn load_all(paths: AppPaths) -> Loaded {
    if let Err(e) = paths.create_all() {
        tracing::warn!(error = %e, "[Settings] The profile folders could not be created");
    }
    let settings = load_file::<Settings>(&paths);
    let ui = load_file::<UiState>(&paths);
    let ui_read_only = ui.as_ref().is_some_and(LoadOutcome::is_read_only);
    if settings
        .as_ref()
        .is_some_and(|o| matches!(o, LoadOutcome::Migrated { .. }))
    {
        // A migrated file is saved in its new form right away (the backup holds the old one).
        if let Some(outcome) = &settings
            && let Err(e) = save(&paths.config_dir, outcome.value())
        {
            tracing::warn!(error = %e, "[Settings] The migrated settings could not be saved");
        }
    }
    Loaded {
        settings: settings.map(LoadOutcome::into_value).unwrap_or_default(),
        ui_state: ui.map(LoadOutcome::into_value).unwrap_or_default(),
        ui_read_only,
        paths,
    }
}

fn load_file<T: VersionedFile>(paths: &AppPaths) -> Option<LoadOutcome<T>> {
    match load::<T>(&paths.config_dir, &paths.config_backups()) {
        Ok(outcome) => {
            let kind = match &outcome {
                LoadOutcome::Fresh(_) => "defaults (no file yet)",
                LoadOutcome::Loaded { .. } => "loaded",
                LoadOutcome::Migrated { .. } => "migrated",
                LoadOutcome::Newer { .. } => "newer version, read-only",
                LoadOutcome::Damaged { .. } => "damaged, defaults in use",
            };
            tracing::info!(file = T::FILE_NAME, outcome = kind, "[Settings] Read");
            Some(outcome)
        }
        Err(e) => {
            tracing::error!(file = T::FILE_NAME, error = %e, "[Settings] Not read; defaults in use and the file is left alone");
            None
        }
    }
}

/// Saves the UI state unless it came from a newer version.
pub fn save_ui_state(loaded: &Loaded, state: &UiState) {
    if loaded.ui_read_only {
        tracing::warn!("[Settings] ui-state.toml is from a newer version; not overwritten");
        return;
    }
    match save(&loaded.paths.config_dir, state) {
        Ok(()) => tracing::debug!("[Settings] UI state saved"),
        Err(e) => tracing::warn!(error = %e, "[Settings] UI state could not be saved"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_start_then_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_all(AppPaths::below(dir.path()));
        assert_eq!(loaded.ui_state, UiState::default());
        let mut state = loaded.ui_state.clone();
        state.window.width = 1000;
        save_ui_state(&loaded, &state);
        let again = load_all(AppPaths::below(dir.path()));
        assert_eq!(again.ui_state.window.width, 1000);
        assert!(dir.path().join("data").is_dir() && dir.path().join("state").is_dir());
    }

    #[test]
    fn newer_ui_state_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::below(dir.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(paths.config_dir.join("ui-state.toml"), "version = 99\n").unwrap();
        let loaded = load_all(paths.clone());
        assert!(loaded.ui_read_only);
        save_ui_state(&loaded, &UiState::default());
        assert_eq!(
            std::fs::read_to_string(paths.config_dir.join("ui-state.toml")).unwrap(),
            "version = 99\n"
        );
    }
}
