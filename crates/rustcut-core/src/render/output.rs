//! Putting a verified render onto its target, and removing what a job leaves behind (ADR-9). The rename happens only
//! after the verification; without "overwrite" it never replaces a file, also not one that appeared during the render.
//! A failure or a cancel deletes the temp file and the side files, never the target.

use std::path::{Path, PathBuf};

use rustix::fs::{CWD, RenameFlags, renameat_with};
use rustix::io::Errno;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FinalizeError {
    #[error("{0} exists already and overwriting was not chosen")]
    TargetExists(PathBuf),
    #[error("the render could not be moved onto its target: {0}")]
    Io(String),
}

/// Flushes `temp` to the drive and renames it onto `target` (replacing it only when `overwrite`). Runs on the blocking pool.
pub async fn finalize(temp: &Path, target: &Path, overwrite: bool) -> Result<(), FinalizeError> {
    let (temp, target) = (temp.to_path_buf(), target.to_path_buf());
    tokio::task::spawn_blocking(move || finalize_blocking(&temp, &target, overwrite))
        .await
        .map_err(|e| FinalizeError::Io(e.to_string()))?
}

fn finalize_blocking(temp: &Path, target: &Path, overwrite: bool) -> Result<(), FinalizeError> {
    let io = |e: std::io::Error| FinalizeError::Io(e.to_string());
    // Without this a crash right after the rename can leave an empty target on some file systems.
    std::fs::File::open(temp).and_then(|f| f.sync_all()).map_err(io)?;
    if overwrite {
        std::fs::rename(temp, target).map_err(io)?;
    } else {
        rename_no_replace(temp, target)?;
    }
    if let Some(folder) = target.parent()
        && let Err(e) = std::fs::File::open(folder).and_then(|f| f.sync_all())
    {
        tracing::debug!(folder = %folder.display(), error = %e, "[Render] Folder not flushed");
    }
    Ok(())
}

/// Renames `temp` onto `target` unless `target` exists (atomic where the file system allows: `RENAME_NOREPLACE`, then a
/// hard link; on FAT a check before the rename). Blocking.
pub(crate) fn rename_no_replace(temp: &Path, target: &Path) -> Result<(), FinalizeError> {
    let exists = || FinalizeError::TargetExists(target.to_path_buf());
    match renameat_with(CWD, temp, CWD, target, RenameFlags::NOREPLACE) {
        Ok(()) => return Ok(()),
        Err(Errno::EXIST) => return Err(exists()),
        // Some file systems (older network shares, FUSE) do not support the flag: a hard link is atomic too.
        Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => {}
        Err(e) => return Err(FinalizeError::Io(std::io::Error::from(e).to_string())),
    }
    match std::fs::hard_link(temp, target) {
        Ok(()) => {
            std::fs::remove_file(temp).map_err(|e| FinalizeError::Io(e.to_string()))?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(exists()),
        // Without hard links (FAT, exFAT) only a check before the rename is left.
        Err(_) => {
            if target.try_exists().unwrap_or(true) {
                return Err(exists());
            }
            std::fs::rename(temp, target).map_err(|e| FinalizeError::Io(e.to_string()))
        }
    }
}

/// The files and folders a job leaves behind: its temp file and its side files. Removed by [`Leftovers::remove`] when the
/// job ends; if the render is dropped halfway (the task was aborted), the drop removes them.
#[derive(Debug, Default)]
pub struct Leftovers {
    files: Vec<PathBuf>,
    folders: Vec<PathBuf>,
}

impl Leftovers {
    pub fn new(files: Vec<PathBuf>, folders: Vec<PathBuf>) -> Self {
        Self { files, folders }
    }

    /// Removes everything on the blocking pool; a missing path is fine, other failures are logged.
    pub async fn remove(mut self) {
        let files = std::mem::take(&mut self.files);
        let folders = std::mem::take(&mut self.folders);
        if let Err(e) = tokio::task::spawn_blocking(move || remove_all(&files, &folders)).await {
            tracing::warn!(error = %e, "[Render] Leftover files not removed");
        }
    }
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        // Only reached when the render future was dropped: unlinking a few files is quick.
        remove_all(&self.files, &self.folders);
    }
}

fn remove_all(files: &[PathBuf], folders: &[PathBuf]) {
    let report = |path: &Path, e: std::io::Error| {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), error = %e, "[Render] Not removed");
        }
    };
    for file in files {
        if let Err(e) = std::fs::remove_file(file) {
            report(file, e);
        }
    }
    for folder in folders {
        if let Err(e) = std::fs::remove_dir_all(folder) {
            report(folder, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_target_is_replaced_only_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let (temp, target) = (dir.path().join(".a.rustcut-part.mkv"), dir.path().join("a.mkv"));
        std::fs::write(&temp, "new").unwrap();
        finalize(&temp, &target, false).await.unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(!temp.exists());

        std::fs::write(&temp, "newer").unwrap();
        assert_eq!(
            finalize(&temp, &target, false).await,
            Err(FinalizeError::TargetExists(target.clone()))
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "new",
            "the existing file is untouched"
        );
        finalize(&temp, &target, true).await.unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newer");

        assert!(matches!(
            finalize(&temp, &target, true).await,
            Err(FinalizeError::Io(_))
        ));
    }

    #[tokio::test]
    async fn leftovers_go_on_remove_and_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("job-1.trf");
        let folder = dir.path().join("job-1");
        std::fs::write(&file, "x").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("pass-0.log"), "x").unwrap();
        Leftovers::new(vec![file.clone(), dir.path().join("missing")], vec![folder.clone()])
            .remove()
            .await;
        assert!(!file.exists() && !folder.exists());

        std::fs::write(&file, "x").unwrap();
        drop(Leftovers::new(vec![file.clone()], Vec::new()));
        assert!(!file.exists());
    }
}
