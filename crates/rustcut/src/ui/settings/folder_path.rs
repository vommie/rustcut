//! The path of a target folder typed or picked on the Export & Naming page (P9.7, the user's wishes): the text is
//! checked against Linux's rules at once (`folders::check_path`; a refusal keeps the field open with a bubble), then
//! off the UI thread against the file system: a file of that name is refused with a bubble and the path stays as it
//! was. A folder that does not exist yet is fine: it is made on the fly when a job writes into it (the render engine),
//! and the field shows a sign until then.

use std::path::{Path, PathBuf};
use std::rc::Weak;
use std::time::Duration;

use rustcut_core::settings::Settings;

use super::handlers::{bubble_key, post_refused};
use super::{SettingsBridge, folders};
use crate::ui::notices::{self, TYPING};

/// The option key of the path field of folder `index` (its bubble anchor is "settings:" + this).
fn path_key(index: usize) -> String {
    format!("folder.{index}.path")
}

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// The checked path for folder `index`, or why it is refused (Linux's rules, another folder of the list).
fn checked(settings: &Settings, index: usize, text: &str) -> Result<PathBuf, &'static str> {
    let path = folders::check_path(text, home().as_deref())?;
    let own = settings.export.folders.entries.get(index).map(|n| n.id);
    let taken = settings
        .export
        .folders
        .find_path(&path)
        .is_some_and(|n| Some(n.id) != own);
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
    /// A typed path for folder `index` (Enter or leaving the field): `false` = refused, the field stays open.
    pub(super) fn set_folder_path(&self, index: usize, text: &str) -> bool {
        let key = path_key(index);
        let path = match checked(&self.writer.current(), index, text) {
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
                    bridge.update(|s| result = folders::set_path(&mut s.export.folders, index, path.clone()));
                    match result {
                        Ok(true) => tracing::info!(folder = %path.display(), "[Settings] Target folder path changed"),
                        Ok(false) => {}
                        Err(why) => post_refused(&key, why, Duration::ZERO),
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

    /// The path field of folder `index` with a refused text was typed in: checked again after the typing grace time.
    pub(super) fn folder_path_typed(&self, index: usize, text: &str) {
        let key = path_key(index);
        if !self.errors.borrow().contains_key(&key) {
            return;
        }
        match checked(&self.writer.current(), index, text) {
            Ok(_) => {
                self.errors.borrow_mut().remove(&key);
                notices::clear_soon(&bubble_key(&key), TYPING);
            }
            Err(why) => post_refused(&key, why, TYPING),
        }
    }

    /// The path field closed without keeping a text.
    pub(super) fn folder_path_canceled(&self, index: usize) {
        let key = path_key(index);
        if self.errors.borrow_mut().remove(&key).is_some() {
            notices::clear(&bubble_key(&key));
        }
    }

    /// The folder button of row `index`: the portal's folder dialog, then the path as if typed.
    pub(super) fn pick_folder_path(&self, index: usize) {
        let start = self
            .writer
            .current()
            .export
            .folders
            .entries
            .get(index)
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
                bridge.set_folder_path(index, &folder.path().display().to_string());
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no folder dialog");
        }
    }
}
