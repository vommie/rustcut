//! A sidecar goes wherever its video goes (rule 6). Each helper is called right **after** the video's own operation
//! succeeded; a sidecar left behind by a failure is an orphan that a later sweep sends to the trash.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rustix::fs::{CWD, RenameFlags};
use rustix::io::Errno;
use tracing::{debug, info};

use crate::file::{self, Content, Error, Time, sidecar_of};
use crate::hash::Md5Hex;
use crate::name;

/// A temporary file of an interrupted sidecar write counts as left behind only after this long, so a sweep never takes
/// it from a write that is still going on.
pub const LEFTOVER_AGE: Duration = Duration::from_mins(10);

/// What happened to a video's sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// The video had no sidecar (or, for a copy, none that counts).
    Nothing,
    /// The sidecar was renamed along (same file system).
    Moved,
    /// The sidecar was written at the new place with its old modification time (a copy, or another file system).
    Copied,
    /// The new place already had a fresh sidecar with the same hash; it stays.
    Kept,
    /// The sidecar was invalid and went to the trash instead of along.
    Trashed,
    /// The sidecar was deleted for good together with its video.
    Removed,
}

/// The video was moved or renamed from `from_video` to `to_video`: its sidecar follows (renamed on the same file
/// system, written anew with its old time on another one, then the old one removed). An invalid sidecar goes to the
/// trash instead. A sidecar already at the new place belongs to no video there and goes to the trash first, unless it
/// is fresh and says the same.
pub fn move_along(
    from_video: &Path,
    to_video: &Path,
    trash: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<Carried, Error> {
    let from = sidecar_of(from_video)?;
    let to = sidecar_of(to_video)?;
    if from == to {
        return Ok(Carried::Nothing);
    }
    let (hash, modified) = match file::read(&from) {
        Content::Missing => return Ok(Carried::Nothing),
        Content::Invalid(why) => {
            trash_logged(&from, &format!("invalid ({why})"), trash)?;
            return Ok(Carried::Trashed);
        }
        Content::Valid(hash, modified) => (hash, modified),
    };
    if holds_fresh(&to, to_video, hash) {
        remove_logged(&from, "the new place already has it")?;
        return Ok(Carried::Kept);
    }
    clear_orphan(&to, trash)?;
    match rename_no_replace(&from, &to) {
        Ok(()) => {
            info!("[Sidecar] Moved {} -> {}", from.display(), to.display());
            Ok(Carried::Moved)
        }
        Err(Errno::XDEV) => {
            file::commit(&to, hash, Time::Exactly(modified))?;
            info!("[Sidecar] Copied {} -> {}", from.display(), to.display());
            remove_logged(&from, "copied to the video's new place")?;
            Ok(Carried::Copied)
        }
        Err(errno) => Err(Error::Io {
            path: from,
            source: errno.into(),
        }),
    }
}

/// The video was copied from `from_video` to `to_video`: a valid sidecar is written next to the copy with its old
/// modification time (so it stays fresh when the copy kept the video's time). An invalid one is not copied. A sidecar
/// already at the new place goes to the trash first, unless it is fresh and says the same.
pub fn copy_along(
    from_video: &Path,
    to_video: &Path,
    trash: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<Carried, Error> {
    let from = sidecar_of(from_video)?;
    let to = sidecar_of(to_video)?;
    if from == to {
        return Ok(Carried::Nothing);
    }
    let Content::Valid(hash, modified) = file::read(&from) else {
        debug!("[Sidecar] {} has no valid sidecar to copy", from_video.display());
        return Ok(Carried::Nothing);
    };
    if holds_fresh(&to, to_video, hash) {
        return Ok(Carried::Kept);
    }
    clear_orphan(&to, trash)?;
    file::commit(&to, hash, Time::Exactly(modified))?;
    info!("[Sidecar] Copied {} -> {}", from.display(), to.display());
    Ok(Carried::Copied)
}

/// The video went to the trash: so does its sidecar.
pub fn trash_along(video: &Path, trash: &mut dyn FnMut(&Path) -> io::Result<()>) -> Result<Carried, Error> {
    let sidecar = sidecar_of(video)?;
    if !exists(&sidecar)? {
        return Ok(Carried::Nothing);
    }
    trash_logged(&sidecar, "its video went to the trash", trash)?;
    Ok(Carried::Trashed)
}

/// The video was deleted for good (for example the source of a finished move by copy): so is its sidecar.
pub fn remove_along(video: &Path) -> Result<Carried, Error> {
    let sidecar = sidecar_of(video)?;
    if !exists(&sidecar)? {
        return Ok(Carried::Nothing);
    }
    remove_logged(&sidecar, "its video was removed")?;
    Ok(Carried::Removed)
}

/// Whether `sidecar` has a sidecar's name and its video is gone. Orphans go to the trash, never away for good.
pub fn is_orphan(sidecar: &Path) -> Result<bool, Error> {
    let Some(video) = name::video_for(sidecar) else {
        return Ok(false);
    };
    Ok(!exists(&video)?)
}

/// The orphans in one folder (not its subfolders), sorted: sidecars whose video is gone, and temporary files of
/// interrupted sidecar writes older than [`LEFTOVER_AGE`].
pub fn orphans_in(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(Error::io(dir))? {
        let entry = entry.map_err(Error::io(dir))?;
        let file_name = entry.file_name();
        let path = entry.path();
        let orphan = if name::is_sidecar_name(&file_name) {
            is_orphan(&path)?
        } else if name::is_write_leftover(&file_name) {
            is_old(&path)?
        } else {
            false
        };
        if orphan {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

fn is_old(path: &Path) -> Result<bool, Error> {
    let modified = fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(Error::io(path))?;
    Ok(SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= LEFTOVER_AGE))
}

/// Whether `sidecar` (of `video`) is valid, fresh and holds `hash`.
fn holds_fresh(sidecar: &Path, video: &Path, hash: Md5Hex) -> bool {
    let Ok(video_modified) = fs::metadata(video).and_then(|meta| meta.modified()) else {
        return false;
    };
    file::judge(file::read(sidecar), video_modified) == file::Lookup::Fresh(hash)
}

/// A sidecar where a video's sidecar is about to go belongs to no video there: to the trash.
fn clear_orphan(sidecar: &Path, trash: &mut dyn FnMut(&Path) -> io::Result<()>) -> Result<(), Error> {
    if exists(sidecar)? {
        trash_logged(sidecar, "an orphan where the video's sidecar goes", trash)?;
    }
    Ok(())
}

fn trash_logged(path: &Path, why: &str, trash: &mut dyn FnMut(&Path) -> io::Result<()>) -> Result<(), Error> {
    trash(path).map_err(Error::io(path))?;
    info!("[Sidecar] Moved {} to the trash: {why}", path.display());
    Ok(())
}

fn remove_logged(path: &Path, why: &str) -> Result<(), Error> {
    fs::remove_file(path).map_err(Error::io(path))?;
    info!("[Sidecar] Removed {}: {why}", path.display());
    Ok(())
}

fn exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::Io {
            path: path.to_path_buf(),
            source: error,
        }),
    }
}

/// `renameat2(RENAME_NOREPLACE)`; on a file system without it (some FUSE ones) a plain rename after an existence check.
fn rename_no_replace(from: &Path, to: &Path) -> Result<(), Errno> {
    match rustix::fs::renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => {
            if to.symlink_metadata().is_ok() {
                return Err(Errno::EXIST);
            }
            fs::rename(from, to).map_err(|error| Errno::from_io_error(&error).unwrap_or(Errno::IO))
        }
        other => other,
    }
}
