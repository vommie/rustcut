//! Loading settings and UI state at startup and saving the UI state at quit (paths from `--home` or XDG).

use rustcut_core::settings::{AppPaths, LoadOutcome, Settings, UiState, VersionedFile, load, save};

/// What startup loaded; `ui_read_only` keeps a newer version's `ui-state.toml` from being overwritten, and
/// `settings_read_only` a `config.toml` that is newer or could not be read.
#[derive(Debug)]
pub struct Loaded {
    pub paths: AppPaths,
    pub settings: Settings,
    pub ui_state: UiState,
    pub ui_read_only: bool,
    pub settings_read_only: bool,
}

/// Resolves the folders, creates them and loads both files. Never fails: problems are logged and defaults used.
pub fn load_all(paths: AppPaths) -> Loaded {
    if let Err(e) = paths.create_all() {
        tracing::warn!(error = %e, "[Settings] The profile folders could not be created");
    }
    let settings = load_file::<Settings>(&paths);
    let ui = load_file::<UiState>(&paths);
    let ui_read_only = ui.as_ref().is_some_and(LoadOutcome::is_read_only);
    let settings_read_only = settings.as_ref().is_none_or(LoadOutcome::is_read_only);
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
        settings_read_only,
        paths,
    }
}

/// Writes `config.toml` on the runtime whenever the app changed the settings (the Render Settings panel's profiles, the
/// export panel's target folders and warnings), one save after the other; the latest settings win when several changes
/// come quickly. Every bridge changes the one shared copy, so no bridge overwrites another one's change. UI thread only.
#[derive(Clone)]
pub struct SettingsWriter {
    sender: Option<tokio::sync::watch::Sender<Option<Settings>>>,
    current: std::rc::Rc<std::cell::RefCell<Settings>>,
    listeners: std::rc::Rc<std::cell::RefCell<Vec<Listener>>>,
}

/// Called with the settings before and after a change (UI thread).
type Listener = std::rc::Rc<dyn Fn(&Settings, &Settings)>;

impl std::fmt::Debug for SettingsWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsWriter")
            .field("read_only", &self.sender.is_none())
            .field("listeners", &self.listeners.borrow().len())
            .finish_non_exhaustive()
    }
}

impl SettingsWriter {
    /// A writer into `config_dir` starting from `settings`; `read_only` (a newer or unreadable file) makes every save a
    /// logged no-op.
    pub fn start(
        runtime: &tokio::runtime::Handle,
        config_dir: std::path::PathBuf,
        read_only: bool,
        settings: &Settings,
    ) -> Self {
        let current = std::rc::Rc::new(std::cell::RefCell::new(settings.clone()));
        let listeners = std::rc::Rc::default();
        if read_only {
            return Self {
                sender: None,
                current,
                listeners,
            };
        }
        let (sender, mut receiver) = tokio::sync::watch::channel(None::<Settings>);
        runtime.spawn(async move {
            while receiver.changed().await.is_ok() {
                let Some(settings) = receiver.borrow_and_update().clone() else {
                    continue;
                };
                let dir = config_dir.clone();
                match tokio::task::spawn_blocking(move || save(&dir, &settings)).await {
                    Ok(Ok(())) => tracing::debug!("[Settings] config.toml saved"),
                    Ok(Err(e)) => tracing::warn!(error = %e, "[Settings] config.toml could not be saved"),
                    Err(e) => tracing::warn!(error = %e, "[Settings] Saving config.toml did not finish"),
                }
            }
        });
        Self {
            sender: Some(sender),
            current,
            listeners,
        }
    }

    /// Runs `listener` after every change that changed something, with the settings before and after (the bridges that
    /// apply a setting while RustCut runs). A listener may change the settings itself.
    pub fn subscribe(&self, listener: impl Fn(&Settings, &Settings) + 'static) {
        self.listeners.borrow_mut().push(std::rc::Rc::new(listener));
    }

    /// Whether `config.toml` is written (not a newer or unreadable file).
    pub fn writable(&self) -> bool {
        self.sender.is_some()
    }

    /// Changes the settings with `change` and saves them soon (on the runtime). `false` when `config.toml` must not be
    /// written (the change still holds for this run).
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> bool {
        let (before, settings) = {
            let mut current = self.current.borrow_mut();
            let before = current.clone();
            change(&mut current);
            (before, current.clone())
        };
        if before != settings {
            // Cloned out, so a listener may subscribe or change the settings again.
            let listeners: Vec<Listener> = self.listeners.borrow().clone();
            for listener in listeners {
                listener(&before, &settings);
            }
        }
        let Some(sender) = &self.sender else {
            tracing::warn!("[Settings] config.toml is from a newer version or unreadable; not overwritten");
            return false;
        };
        sender.send_replace(Some(self.current()));
        true
    }

    /// The settings as the app changed them so far.
    pub fn current(&self) -> Settings {
        self.current.borrow().clone()
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
    fn the_writer_saves_in_the_background_and_respects_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let writer = SettingsWriter::start(runtime.handle(), dir.path().to_path_buf(), false, &Settings::default());
        assert!(writer.update(|s| s.queue.workers = 3));
        // A second bridge's change keeps the first one's.
        let second = writer.clone();
        assert!(second.update(|s| s.render.nice = 5));
        assert_eq!((writer.current().queue.workers, writer.current().render.nice), (3, 5));
        runtime.block_on(async {
            for _ in 0..100 {
                if dir.path().join("config.toml").exists() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
        let read = load::<Settings>(dir.path(), &dir.path().join("b"))
            .unwrap()
            .into_value();
        assert_eq!((read.queue.workers, read.render.nice), (3, 5));

        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = std::rc::Rc::clone(&seen);
        writer.subscribe(move |before, after| log.borrow_mut().push((before.queue.workers, after.queue.workers)));
        assert!(writer.update(|s| s.queue.workers = 4));
        assert!(
            writer.update(|s| s.queue.workers = 4),
            "saved again, no change to report"
        );
        assert_eq!(*seen.borrow(), vec![(3, 4)]);

        let other = tempfile::tempdir().unwrap();
        let locked = SettingsWriter::start(runtime.handle(), other.path().to_path_buf(), true, &Settings::default());
        assert!(!locked.update(|s| s.queue.workers = 2));
        assert_eq!(locked.current().queue.workers, 2, "the change holds for this run");
        assert!(!other.path().join("config.toml").exists());
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
