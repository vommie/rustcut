//! The folder pictures' store (P11.2): `pictures.sqlite` in the data folder on its own thread, opened at the start
//! (like the queue's database, before the window shows), told which pictures the tree uses, stopped at quit.

use std::thread::JoinHandle;

use rustcut_core::actor::Handle;
use rustcut_core::folders::FolderTree;
use rustcut_core::folders::pictures::{self, PictureCommand, PictureHandle, PictureStore, spawn_picture_store};
use rustcut_core::settings::AppPaths;

/// Opens the store (a file that cannot be used: an empty one in memory, logged; the tree's pictures then show their
/// first letters) and removes the pictures the tree no longer uses.
pub(super) fn start(paths: &AppPaths, tree: &FolderTree) -> (PictureHandle, Option<JoinHandle<()>>) {
    let path = paths.data_dir.join(pictures::FILE_NAME);
    let store = PictureStore::open(&path, &paths.data_backups()).or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "[Pictures] The picture store cannot be used; pictures are kept in memory");
        PictureStore::open_in_memory()
    });
    let started = store
        .map_err(|e| e.to_string())
        .and_then(|store| spawn_picture_store(store).map_err(|e| e.to_string()));
    match started {
        Ok((handle, thread)) => {
            retain(&handle, tree);
            (handle, Some(thread))
        }
        Err(e) => {
            tracing::error!(error = %e, "[Pictures] The picture store did not start");
            (Handle::closed(), None)
        }
    }
}

/// Removes the stored pictures `tree` does not use (fire and forget).
pub fn retain(handle: &PictureHandle, tree: &FolderTree) {
    let keep = tree.stored_pictures();
    if handle.send(PictureCommand::Retain { keep }).is_err() {
        tracing::debug!("[Pictures] The picture store has stopped");
    }
}

/// Closes the store after the commands before it and joins its thread.
pub(super) fn stop(handle: &PictureHandle, thread: Option<JoinHandle<()>>) {
    if handle
        .blocking_request(|reply| PictureCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The picture store had already stopped");
    }
    if let Some(thread) = thread
        && thread.join().is_err()
    {
        tracing::warn!("[App] The picture store's thread ended with a panic");
    }
}
