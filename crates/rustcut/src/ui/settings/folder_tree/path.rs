//! The path of a target folder typed or picked in the Target Folders page's details (P9.7, the user's wishes; P11.3): the text is
//! checked against Linux's rules at once (`folders::check_path`; a refusal keeps the field open with a bubble), then
//! off the UI thread against the file system: a file of that name is refused with a bubble and the path stays as it
//! was. A folder that does not exist yet is fine: it is made on the fly when a job writes into it (the render engine),
//! and the field shows a sign until then.

use std::path::{Path, PathBuf};
use std::rc::Weak;
use std::time::Duration;

use rustcut_core::folders::{EditError, FolderId};
use rustcut_core::settings::Settings;

use super::super::handlers::{bubble_key, post_refused};
use super::super::{SettingsBridge, folders};
use crate::ui::notices::{self, TYPING};

/// The option key of the details' path field (its bubble anchor is "settings:" + this).
pub(super) const PATH_KEY: &str = "folder.path";

fn path_key() -> String {
    PATH_KEY.to_owned()
}

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// The checked path for folder `id`, or why it is refused (Linux's rules, another folder of the tree).
fn checked(settings: &Settings, id: FolderId, text: &str) -> Result<PathBuf, &'static str> {
    let path = folders::check_path(text, home().as_deref())?;
    let taken = settings.export.folders.find_path(&path).is_some_and(|n| n.id != id);
    if taken {
        return Err("This folder is in the list already.");
    }
    Ok(path)
}

/// Whether `path` can be a target folder: a folder, or nothing yet (blocking: runs on the runtime).
fn usable(path: &Path) -> Result<(), &'static str> {
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => Err("This is a file, not a folder."),
        _ => Ok(()),
    }
}

impl SettingsBridge {
    /// A typed path for folder `id` (Enter or leaving the field): `false` = refused, the field stays open.
    pub(super) fn set_folder_path(&self, id: FolderId, text: &str) -> bool {
        let key = path_key();
        let path = match checked(&self.writer.current(), id, text) {
            Ok(path) => path,
            Err(why) => {
                self.errors.borrow_mut().insert(key.clone(), why.to_owned());
                post_refused(&key, why, Duration::ZERO);
                return false;
            }
        };
        if self.errors.borrow_mut().remove(&key).is_some() {
            notices::clear(&bubble_key(&key));
        }
        let task = {
            let path = path.clone();
            self.runtime.spawn_blocking(move || usable(&path))
        };
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(prepared) = task.await else { return };
            let Some(bridge) = weak.upgrade() else { return };
            match prepared {
                Ok(()) => {
                    let mut result = Ok(false);
                    bridge.update(|s| result = s.export.folders.set_path(id, path.clone()));
                    match result {
                        Ok(true) => tracing::info!(folder = %path.display(), "[Settings] Target folder path changed"),
                        Ok(false) => {}
                        Err(EditError::DuplicatePath) => {
                            post_refused(&key, "This folder is in the list already.", Duration::ZERO);
                        }
                        Err(e) => tracing::debug!(error = %e, "[Settings] Target folder path not changed"),
                    }
                    bridge.publish();
                }
                Err(why) => {
                    tracing::warn!(folder = %path.display(), why, "[Settings] Target folder refused");
                    post_refused(&key, why, Duration::ZERO);
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; the folder is not checked");
        }
        true
    }

    /// The path field of folder `id` with a refused text was typed in: checked again after the typing grace time.
    pub(super) fn folder_path_typed(&self, id: FolderId, text: &str) {
        let key = path_key();
        if !self.errors.borrow().contains_key(&key) {
            return;
        }
        match checked(&self.writer.current(), id, text) {
            Ok(_) => {
                self.errors.borrow_mut().remove(&key);
                notices::clear_soon(&bubble_key(&key), TYPING);
            }
            Err(why) => post_refused(&key, why, TYPING),
        }
    }

    /// The path field closed without keeping a text.
    pub(super) fn folder_path_canceled(&self) {
        let key = path_key();
        if self.errors.borrow_mut().remove(&key).is_some() {
            notices::clear(&bubble_key(&key));
        }
    }

    /// The folder button of folder `id`: the portal's folder dialog, then the path as if typed.
    pub(super) fn pick_folder_path(&self, id: FolderId) {
        let start = self
            .writer
            .current()
            .export
            .folders
            .find(id)
            .and_then(|f| f.path.clone());
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose the Target Folder");
        if let Some(start) = start.filter(|p| p.is_absolute()) {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&slint::ComponentHandle::window(window).window_handle());
        }
        let picked = dialog.pick_folder();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(folder) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                bridge.set_folder_path(id, &folder.path().display().to_string());
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no folder dialog");
        }
    }
}
