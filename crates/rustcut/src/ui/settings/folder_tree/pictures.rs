//! What the details do with an entry's picture (P12 3.2): the choice, the icon, the own picture from the dialog or
//! dropped from the file manager, and the held delete. Files dropped on the page's tree join it as folders.

use std::path::PathBuf;
use std::rc::Weak;

use rustcut_core::folders::pictures::PictureCommand;
use rustcut_core::folders::{FolderId, LookKind};
use slint::ComponentHandle as _;

use super::actions::tell;
use super::details;
use crate::FolderTreeUi;
use crate::ui::settings::SettingsBridge;

impl SettingsBridge {
    /// The choice changes what shows and which part the details show; nothing kept is lost, and no dialog opens.
    pub(super) fn folder_picture_kind(&self, kind: i32) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_picture_kind(id, details::kind_of(kind));
        });
        self.folders_changed();
    }

    pub(super) fn folder_pick_icon(&self, name: &str) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_icon(id, name);
        });
        self.folders_changed();
    }

    /// The held X on the own picture: it is deleted, the choice stays and the letter shows.
    pub(super) fn folder_delete_image(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_image(id, None);
        });
        tracing::info!(id = id.0, "[Settings] Own picture of a target folder entry deleted");
        self.folders_changed();
    }

    /// An own image or SVG: the file dialog, then the picture store prepares it (off the UI thread).
    pub(super) fn folder_pick_file(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose a Picture").add_filter(
            "Images and SVGs",
            &[
                "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "ico", "qoi", "svg",
            ],
        );
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = dialog.pick_file();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(file) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                bridge.folder_import_for(id, file.path().to_path_buf());
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no file dialog");
        }
    }

    /// The selected entry has "Own" chosen: a dropped file is its picture.
    pub(super) fn folder_takes_picture(&self) -> bool {
        self.folder_page.selected.get().is_some_and(|id| {
            self.writer
                .current()
                .export
                .folders
                .entry(id)
                .is_some_and(|e| e.look().kind == LookKind::Own)
        })
    }

    pub(super) fn folder_import(&self, path: PathBuf) {
        if let Some(id) = self.folder_page.selected.get() {
            self.folder_import_for(id, path);
        }
    }

    /// The picture store prepares the file off the UI thread; it becomes the entry's picture.
    pub(super) fn folder_import_for(&self, id: FolderId, path: PathBuf) {
        let store = self.pictures.store().clone();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let answer = store.request(|reply| PictureCommand::Import { path, reply }).await;
            let Some(bridge) = weak.upgrade() else { return };
            match answer {
                Ok(Ok(stored)) => {
                    bridge.update(|s| {
                        let _ = s.export.folders.set_image(id, Some(stored.hash.clone()));
                    });
                    bridge.folders_changed();
                }
                Ok(Err(e)) => tell("The picture was not taken", &e.to_string()),
                Err(e) => tell("The picture was not taken", &e.to_string()),
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; the picture is not taken");
        }
    }

    pub(super) fn folder_file_over(&self, on: bool) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.global::<FolderTreeUi<'_>>().set_file_over(on);
        }
    }

    /// Files dropped on the Settings window while the page shows: the folders among them join the tree (checked on
    /// the runtime); while "Own" is chosen a file is the entry's picture.
    pub(super) fn folders_dropped(&self, paths: Vec<PathBuf>) {
        let task = self.runtime.spawn_blocking(move || {
            let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| p.is_dir());
            (folders, files)
        });
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let (Ok((folders, files)), Some(bridge)) = (task.await, weak.upgrade()) else {
                return;
            };
            if !folders.is_empty() {
                bridge.folder_add_paths(folders);
            } else if let (Some(file), true) = (files.first(), bridge.folder_takes_picture()) {
                bridge.folder_import(file.clone());
            } else {
                tell(
                    "Only folders can be dropped here",
                    "Files are not target folders. An image can be dropped while \"Own\" is chosen.",
                );
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; the dropped folders are not added");
        }
    }
}
