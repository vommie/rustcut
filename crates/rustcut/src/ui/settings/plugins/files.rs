//! The file dialogs of the plugin pages: installing a plugin file, adding a development folder, saving and reading a
//! plugin's settings. Files are read and written on the runtime; the UI thread only awaits.

use std::path::PathBuf;
use std::rc::Weak;

use rustcut_core::plugins::{InstallError, PluginCommand, values};
use slint::ComponentHandle as _;

use super::SettingsBridge;
use crate::ui::notices::{self, Kind, Notice};

impl SettingsBridge {
    fn dialog(&self, title: &str) -> rfd::AsyncFileDialog {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        dialog
    }

    fn spawn_ui(future: impl Future<Output = ()> + 'static) {
        if let Err(e) = slint::spawn_local(future) {
            tracing::error!(error = %e, "[Plugins] The event loop is gone");
        }
    }

    pub(super) fn plugin_install_dialog(&self) {
        let ext = rustcut_core::plugins::package_extension();
        let picked = self
            .dialog("Install a plugin")
            .add_filter("RustCut plugin", &[ext])
            .pick_file();
        let weak = Weak::clone(&self.me);
        Self::spawn_ui(async move {
            let Some(file) = picked.await else { return };
            if let Some(b) = weak.upgrade() {
                b.plugin_install(file.path().to_path_buf(), false);
            }
        });
    }

    /// Installs or updates from a plugin file; an older version than the installed one asks first (a message with
    /// "Install anyway").
    pub(super) fn plugin_install(&self, file: PathBuf, allow_downgrade: bool) {
        let Some(plugins) = self.plugins() else { return };
        self.plugin_pages.installing.set(true);
        self.publish();
        let handle = plugins.service().handle.clone();
        let asked = file.clone();
        let installing = self.runtime.spawn(async move {
            handle
                .request(|reply| PluginCommand::Install {
                    file: asked,
                    allow_downgrade,
                    reply,
                })
                .await
                .unwrap_or(Err(InstallError::Gone))
        });
        let weak = Weak::clone(&self.me);
        Self::spawn_ui(async move {
            let result = installing.await.unwrap_or(Err(InstallError::Gone));
            let Some(b) = weak.upgrade() else { return };
            b.plugin_pages.installing.set(false);
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match result {
                Ok(done) => {
                    let detail = match &done.previous {
                        Some(previous) if *previous == done.version => "Installed again; its settings stay.".to_owned(),
                        Some(previous) => format!("Updated from {previous}; its settings stay."),
                        None => "Its settings are on its page below Plugins.".to_owned(),
                    };
                    let id = done.id.clone();
                    notices::post(
                        Notice::new(
                            Kind::PluginInstalled,
                            format!("{} {} installed", done.name, done.version),
                        )
                        .detail(detail)
                        .action("Open plugin", move || crate::ui::plugins::show_settings(&id)),
                    );
                    b.plugin_pages.available.borrow_mut().retain(|a| a.file != file);
                }
                Err(InstallError::Downgrade { installed, offered }) => {
                    let weak = Weak::clone(&b.me);
                    let again = file.clone();
                    notices::post(
                        Notice::new(
                            Kind::PluginInstallFailed,
                            format!("{name} is older than the installed plugin"),
                        )
                        .detail(format!("Installed: {installed}, this file: {offered}."))
                        .level(notices::Level::Warning)
                        .action("Install anyway", move || {
                            if let Some(b) = weak.upgrade() {
                                b.plugin_install(again.clone(), true);
                            }
                        }),
                    );
                }
                Err(err) => {
                    notices::post(
                        Notice::new(Kind::PluginInstallFailed, format!("{name} was not installed"))
                            .detail(err.to_string()),
                    );
                }
            }
            b.publish();
        });
    }

    pub(super) fn plugin_add_dev_dialog(&self) {
        let picked = self.dialog("Add a plugin's development folder").pick_folder();
        let weak = Weak::clone(&self.me);
        Self::spawn_ui(async move {
            let Some(folder) = picked.await else { return };
            let Some(b) = weak.upgrade() else { return };
            let folder = folder.path().to_path_buf();
            b.update(|s| {
                if !s.plugins.dev_folders.contains(&folder) {
                    tracing::info!("[Plugins] Development folder added: {}", folder.display());
                    s.plugins.dev_folders.push(folder);
                }
            });
        });
    }

    /// Saves the plugin's settings as TOML.
    pub(super) fn plugin_export(&self, id: &str) {
        let Some(info) = self.plugin_info(id) else { return };
        let stored = self.writer.current().plugins.entry(id).values;
        let text = match toml::to_string_pretty(&values::to_toml(&stored)) {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!("[Plugins] {id}: the settings cannot be written as TOML: {err}");
                return;
            }
        };
        let picked = self
            .dialog(&format!("Save the settings of {}", info.name()))
            .set_file_name(format!("{id}-settings.toml"))
            .add_filter("TOML", &["toml"])
            .save_file();
        let runtime = self.runtime.clone();
        Self::spawn_ui(async move {
            let Some(file) = picked.await else { return };
            let path = file.path().to_path_buf();
            let shown = path.display().to_string();
            let written = runtime.spawn_blocking(move || std::fs::write(&path, text)).await;
            match written {
                Ok(Ok(())) => notices::post(Notice::new(Kind::PluginMessage, "Plugin settings saved").detail(shown)),
                Ok(Err(err)) => notices::post(
                    Notice::new(Kind::PluginInstallFailed, "The plugin settings were not saved")
                        .detail(err.to_string()),
                ),
                Err(_) => {}
            }
        });
    }

    /// Reads settings saved before (unknown keys stay, unusable values fall back to their defaults).
    pub(super) fn plugin_import(&self, id: &str) {
        let Some(info) = self.plugin_info(id) else { return };
        let picked = self
            .dialog(&format!("Read settings for {}", info.name()))
            .add_filter("TOML", &["toml"])
            .pick_file();
        let runtime = self.runtime.clone();
        let weak = Weak::clone(&self.me);
        let id = id.to_owned();
        Self::spawn_ui(async move {
            let Some(file) = picked.await else { return };
            let path = file.path().to_path_buf();
            let read = runtime
                .spawn_blocking(move || {
                    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    text.parse::<toml::Table>().map_err(|e| e.message().to_owned())
                })
                .await;
            let Some(b) = weak.upgrade() else { return };
            match read {
                Ok(Ok(table)) => {
                    let values = values::from_toml(&table);
                    b.update(|s| s.plugins.entries.entry(id.clone()).or_default().values = values);
                    tracing::info!("[Plugins] {id}: settings read from a file");
                    notices::post(Notice::new(Kind::PluginMessage, "Plugin settings read"));
                }
                Ok(Err(err)) => notices::post(
                    Notice::new(Kind::PluginInstallFailed, "The file holds no plugin settings").detail(err),
                ),
                Err(_) => {}
            }
        });
    }
}
