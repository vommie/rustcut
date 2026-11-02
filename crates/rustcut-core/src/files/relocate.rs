//! Moving one file to another path (P12 block 7, user decision Q-P12f). On the same file system it is a rename (atomic, never
//! replacing unless asked). Across file systems the file is copied to a hidden temp file next to the target while a BLAKE3
//! checksum is taken, the copy is flushed, read back and compared, the original is checked for changes during the copy, and
//! only then the copy takes the target's name and the original is deleted. A cancel or any failure deletes the temp file;
//! the original is never touched before the copy is verified. Blocking: runs on the blocking pool.
//!
//! No crate does this: `fs_extra` and `fs-more` copy with progress but neither reads the copy back, checks the original for
//! changes nor places it without replacing.

use std::fs::{File, FileTimes, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use rustix::fs::{Access, CWD, RenameFlags, accessat, renameat_with};
use rustix::io::Errno;

use crate::render::rename_no_replace;

/// The size of one read while copying and verifying.
const CHUNK: usize = 1 << 20;

/// How a file got to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Source and target are the same file: nothing happened.
    Unchanged,
    Renamed,
    /// Copied to another file system, verified, original deleted.
    Copied {
        bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelocateError {
    #[error("the file is not at {0} any more")]
    SourceMissing(PathBuf),
    #[error("{0} is not a regular file")]
    NotAFile(PathBuf),
    #[error("{0} exists already")]
    TargetExists(PathBuf),
    #[error("the original in {0} cannot be removed (no write permission); nothing was moved")]
    SourceLocked(PathBuf),
    #[error("the original changed while it was copied (another program writes to it); it stays where it is")]
    SourceChanged,
    #[error("the copy differs from the original when read back; the original stays where it is")]
    VerifyFailed,
    #[error("canceled")]
    Canceled,
    #[error("{0}")]
    Io(String),
}

fn io(context: &str, e: &std::io::Error) -> RelocateError {
    RelocateError::Io(format!("{context}: {e}"))
}

/// The hidden temp file a copy goes to before it takes the target's name.
pub fn temp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!(".{name}.rustcut-move"))
}

/// Whether moving `source` into `folder` needs a copy (another file system). The folder may not exist yet: its nearest
/// existing ancestor counts. `None` when it cannot be told (the source is missing).
pub fn needs_copy(source: &Path, folder: &Path) -> Option<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let from = std::fs::metadata(source).ok()?.dev();
    let to = folder.ancestors().find_map(|p| std::fs::metadata(p).ok())?.dev();
    Some(from != to)
}

/// Moves `source` to `target`, creating the target's folder. `overwrite` replaces an existing target. `progress` gets the
/// bytes copied and the total (only for a copy, at most once per chunk); `cancel` stops a copy between chunks.
pub fn relocate(
    source: &Path,
    target: &Path,
    overwrite: bool,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Method, RelocateError> {
    let meta = match std::fs::symlink_metadata(source) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RelocateError::SourceMissing(source.to_path_buf()));
        }
        Err(e) => return Err(io("the original cannot be read", &e)),
    };
    if !meta.is_file() {
        return Err(RelocateError::NotAFile(source.to_path_buf()));
    }
    if same_file(source, target) {
        if source == target {
            return Ok(Method::Unchanged);
        }
        // Only the case of the name changes on a file system that ignores case: a plain rename.
        std::fs::rename(source, target).map_err(|e| io("the file could not be renamed", &e))?;
        return Ok(Method::Renamed);
    }
    let folder = target
        .parent()
        .ok_or_else(|| RelocateError::Io("the target has no folder".into()))?;
    std::fs::create_dir_all(folder).map_err(|e| io("the target folder could not be created", &e))?;
    if !overwrite && target.symlink_metadata().is_ok() {
        return Err(RelocateError::TargetExists(target.to_path_buf()));
    }
    match rename(source, target, overwrite) {
        Ok(()) => {
            sync_folder(folder);
            Ok(Method::Renamed)
        }
        Err(Errno::XDEV) => copy_across(source, target, overwrite, &meta, progress, cancel),
        Err(Errno::EXIST) => Err(RelocateError::TargetExists(target.to_path_buf())),
        Err(e) => Err(io("the file could not be moved", &std::io::Error::from(e))),
    }
}

/// Whether both paths name the same existing file (the same path, or another spelling of it).
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    if crate::naming::same_path(a, b) {
        return true;
    }
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// A rename that replaces only with `overwrite`.
fn rename(source: &Path, target: &Path, overwrite: bool) -> Result<(), Errno> {
    if overwrite {
        return std::fs::rename(source, target).map_err(|e| Errno::from_io_error(&e).unwrap_or(Errno::IO));
    }
    match renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE) {
        Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => {
            // Without the flag (some network shares, FUSE): the hard link of the render's finalize.
            rename_no_replace(source, target).map_err(|e| match e {
                crate::render::FinalizeError::TargetExists(_) => Errno::EXIST,
                crate::render::FinalizeError::Io(_) => Errno::IO,
            })
        }
        other => other,
    }
}

fn sync_folder(folder: &Path) {
    if let Err(e) = File::open(folder).and_then(|f| f.sync_all()) {
        tracing::debug!(error = %e, "[Move] The folder could not be flushed");
    }
}

/// Copies, verifies, places the copy and deletes the original.
fn copy_across(
    source: &Path,
    target: &Path,
    overwrite: bool,
    meta: &std::fs::Metadata,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Method, RelocateError> {
    let source_folder = source.parent().unwrap_or_else(|| Path::new("/"));
    if accessat(CWD, source_folder, Access::WRITE_OK, rustix::fs::AtFlags::empty()).is_err() {
        return Err(RelocateError::SourceLocked(source_folder.to_path_buf()));
    }
    let temp = temp_path(target);
    let result = copy_verified(source, &temp, meta, progress, cancel).and_then(|bytes| {
        let placed = if overwrite {
            std::fs::rename(&temp, target).map_err(|e| io("the copy could not take its name", &e))
        } else {
            rename_no_replace(&temp, target).map_err(|e| match e {
                crate::render::FinalizeError::TargetExists(p) => RelocateError::TargetExists(p),
                crate::render::FinalizeError::Io(text) => RelocateError::Io(text),
            })
        };
        placed.map(|()| bytes)
    });
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
    };
    if let Some(folder) = target.parent() {
        sync_folder(folder);
    }
    if let Err(e) = std::fs::remove_file(source) {
        // The verified copy stays at the target; two files are better than none.
        tracing::warn!(error = %e, "[Move] The original could not be removed after the copy");
        return Err(io("copied, but the original could not be removed", &e));
    }
    sync_folder(source_folder);
    Ok(Method::Copied { bytes })
}

/// Copies `source` to `temp` (new file) with a checksum, keeps its permissions and times, reads the copy back and checks
/// that the original did not change meanwhile. Returns the bytes copied.
fn copy_verified(
    source: &Path,
    temp: &Path,
    meta: &std::fs::Metadata,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<u64, RelocateError> {
    let total = meta.len();
    let mut from = File::open(source).map_err(|e| io("the original cannot be read", &e))?;
    let mut to = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(|e| io("the copy cannot be created", &e))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut done = 0_u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(RelocateError::Canceled);
        }
        let read = from
            .read(&mut buffer)
            .map_err(|e| io("the original cannot be read", &e))?;
        let Some(chunk) = buffer.get(..read) else { break };
        if chunk.is_empty() {
            break;
        }
        hasher.update(chunk);
        to.write_all(chunk).map_err(|e| io("the copy cannot be written", &e))?;
        done = done.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        // Half the work is the read-back.
        progress(done / 2, total);
    }
    to.set_permissions(meta.permissions())
        .map_err(|e| io("the copy's permissions cannot be set", &e))?;
    let times = FileTimes::new()
        .set_accessed(meta.accessed().unwrap_or_else(|_| SystemTime::now()))
        .set_modified(meta.modified().unwrap_or_else(|_| SystemTime::now()));
    if let Err(e) = to.set_times(times) {
        tracing::debug!(error = %e, "[Move] The copy keeps the time of the copy");
    }
    to.sync_all().map_err(|e| io("the copy cannot be flushed", &e))?;
    drop(to);
    let written = hasher.finalize();

    let mut back = File::open(temp).map_err(|e| io("the copy cannot be read back", &e))?;
    let mut check = blake3::Hasher::new();
    let mut read_back = 0_u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(RelocateError::Canceled);
        }
        let read = back
            .read(&mut buffer)
            .map_err(|e| io("the copy cannot be read back", &e))?;
        let Some(chunk) = buffer.get(..read) else { break };
        if chunk.is_empty() {
            break;
        }
        check.update(chunk);
        read_back = read_back.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        progress(done - (done.saturating_sub(read_back)) / 2, total);
    }
    if check.finalize() != written || read_back != done {
        return Err(RelocateError::VerifyFailed);
    }
    let now = std::fs::metadata(source).map_err(|e| io("the original cannot be read", &e))?;
    if now.len() != meta.len() || now.len() != done || now.modified().ok() != meta.modified().ok() {
        return Err(RelocateError::SourceChanged);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn a_rename_moves_the_file_and_makes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("new/sub/b.mkv"));
        std::fs::write(&a, b"video").unwrap();
        assert_eq!(relocate(&a, &b, false, &mut |_, _| {}, &none()), Ok(Method::Renamed));
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"video");
        assert_eq!(relocate(&b, &b, false, &mut |_, _| {}, &none()), Ok(Method::Unchanged));
    }

    #[test]
    fn an_existing_target_is_replaced_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("b.mkv"));
        std::fs::write(&a, b"new").unwrap();
        std::fs::write(&b, b"old").unwrap();
        assert_eq!(
            relocate(&a, &b, false, &mut |_, _| {}, &none()),
            Err(RelocateError::TargetExists(b.clone()))
        );
        assert!(a.exists(), "a refusal changes nothing");
        assert_eq!(relocate(&a, &b, true, &mut |_, _| {}, &none()), Ok(Method::Renamed));
        assert_eq!(std::fs::read(&b).unwrap(), b"new");
    }

    #[test]
    fn a_missing_source_and_a_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.mkv");
        assert_eq!(
            relocate(&gone, &dir.path().join("x.mkv"), false, &mut |_, _| {}, &none()),
            Err(RelocateError::SourceMissing(gone))
        );
        assert!(matches!(
            relocate(dir.path(), &dir.path().join("x"), false, &mut |_, _| {}, &none()),
            Err(RelocateError::NotAFile(_))
        ));
    }

    #[test]
    fn a_copy_is_verified_keeps_the_time_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("out/b.mkv"));
        let data: Vec<u8> = (0..(3 * CHUNK + 17)).map(|i| u8::try_from(i % 251).unwrap()).collect();
        std::fs::write(&a, &data).unwrap();
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        File::options().write(true).open(&a).unwrap().set_modified(old).unwrap();
        let meta = std::fs::metadata(&a).unwrap();
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        let mut seen = Vec::new();
        let method = copy_across(
            &a,
            &b,
            false,
            &meta,
            &mut |done, total| seen.push((done, total)),
            &none(),
        );
        assert_eq!(
            method,
            Ok(Method::Copied {
                bytes: data.len() as u64
            })
        );
        assert!(!a.exists() && !temp_path(&b).exists());
        assert_eq!(std::fs::read(&b).unwrap(), data);
        assert_eq!(std::fs::metadata(&b).unwrap().modified().unwrap(), old);
        assert_eq!(seen.last(), Some(&(data.len() as u64, data.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "progress only grows");
    }

    #[test]
    fn a_canceled_copy_leaves_the_original_and_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("b.mkv"));
        std::fs::write(&a, vec![1_u8; 2 * CHUNK]).unwrap();
        let meta = std::fs::metadata(&a).unwrap();
        let cancel = AtomicBool::new(false);
        let result = copy_across(
            &a,
            &b,
            false,
            &meta,
            &mut |_, _| cancel.store(true, Ordering::Relaxed),
            &cancel,
        );
        assert_eq!(result, Err(RelocateError::Canceled));
        assert!(a.exists() && !b.exists() && !temp_path(&b).exists());
    }

    #[test]
    fn a_source_that_changes_during_the_copy_stays() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("b.mkv"));
        std::fs::write(&a, vec![1_u8; 2 * CHUNK]).unwrap();
        let meta = std::fs::metadata(&a).unwrap();
        let mut grown = false;
        let result = copy_across(
            &a,
            &b,
            false,
            &meta,
            &mut |_, _| {
                if !grown {
                    grown = true;
                    OpenOptions::new()
                        .append(true)
                        .open(&a)
                        .unwrap()
                        .write_all(b"more")
                        .unwrap();
                }
            },
            &none(),
        );
        assert!(
            matches!(result, Err(RelocateError::SourceChanged | RelocateError::VerifyFailed)),
            "{result:?}"
        );
        assert!(a.exists() && !b.exists() && !temp_path(&b).exists());
    }

    #[test]
    fn a_copy_never_replaces_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mkv"), dir.path().join("b.mkv"));
        std::fs::write(&a, b"new").unwrap();
        let meta = std::fs::metadata(&a).unwrap();
        std::fs::write(&b, b"appeared meanwhile").unwrap();
        assert!(matches!(
            copy_across(&a, &b, false, &meta, &mut |_, _| {}, &none()),
            Err(RelocateError::TargetExists(_))
        ));
        assert_eq!(std::fs::read(&b).unwrap(), b"appeared meanwhile");
        assert!(a.exists() && !temp_path(&b).exists());
    }
}
