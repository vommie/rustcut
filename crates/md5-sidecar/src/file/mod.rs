//! Reading, judging and writing a video's sidecar (rules 3, 4 and 5).

use std::fs::{File, Metadata};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use atomic_write_file::AtomicWriteFile;
use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use tracing::{debug, info, warn};

use crate::hash::{self, Invalid, Md5Hex};
use crate::name::path_for;

/// [`crate::MAX_LEN`] plus one byte, so a longer file is noticed without reading all of it.
const READ_LIMIT: u64 = 4097;

/// What went wrong with a sidecar operation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The video path has no file name (`/`, `..`), so it has no sidecar.
    #[error("{} has no file name", .0.display())]
    NoFileName(PathBuf),
    /// A file system call failed.
    #[error("{}: {source}", .path.display())]
    Io {
        /// The file or folder the call was about.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: io::Error,
    },
    /// The video's size or time changed between the hashing and the write, so the hash may describe old bytes.
    #[error("{} changed while it was hashed, so no sidecar was written", .0.display())]
    VideoChanged(PathBuf),
    /// The hash cannot belong to the video: only zero bytes give the zero-byte MD5.
    #[error("{} has {len} bytes, so {hash} cannot be its MD5", .video.display())]
    ImpossibleHash {
        /// The video.
        video: PathBuf,
        /// Its size.
        len: u64,
        /// The hash that was handed over.
        hash: Md5Hex,
    },
}

impl Error {
    pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// The sidecar path of `video`, or [`Error::NoFileName`].
pub(crate) fn sidecar_of(video: &Path) -> Result<PathBuf, Error> {
    path_for(video).ok_or_else(|| Error::NoFileName(video.to_path_buf()))
}

/// A video's size and modification time. Take it **before** the video's bytes are hashed and hand it to [`write`],
/// which writes only when the video still looks the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Size in bytes.
    pub len: u64,
    /// Modification time.
    pub modified: SystemTime,
}

impl Stamp {
    /// The stamp of the video at `video` (symbolic links are followed).
    pub fn of(video: &Path) -> Result<Self, Error> {
        let meta = std::fs::metadata(video).map_err(Error::io(video))?;
        Self::from_metadata(&meta).map_err(Error::io(video))
    }

    /// The stamp from metadata the caller already has.
    pub fn from_metadata(meta: &Metadata) -> io::Result<Self> {
        Ok(Self {
            len: meta.len(),
            modified: meta.modified()?,
        })
    }
}

/// Rule 4: a valid sidecar is fresh when it is not older than its video; otherwise the video is hashed again.
pub fn is_fresh(sidecar_modified: SystemTime, video_modified: SystemTime) -> bool {
    sidecar_modified >= video_modified
}

/// What a video's sidecar says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// There is none.
    Missing,
    /// There is one, but it does not count (treated as missing).
    Invalid(Invalid),
    /// It holds a hash but is older than the video, so the video may have changed since: hash again.
    Stale(Md5Hex),
    /// It holds a hash and is not older than the video: trust it.
    Fresh(Md5Hex),
}

impl Lookup {
    /// The hash to trust instead of reading the video, if any.
    pub fn fresh(self) -> Option<Md5Hex> {
        match self {
            Self::Fresh(hash) => Some(hash),
            _ => None,
        }
    }
}

/// What `video`'s sidecar says (rules 3 and 4). An invalid sidecar is logged at info level. Fails only when the video
/// itself cannot be inspected.
pub fn lookup(video: &Path) -> Result<Lookup, Error> {
    lookup_at(video, Stamp::of(video)?.modified)
}

/// Like [`lookup`], with the video's modification time already known (from a walk's metadata).
pub fn lookup_at(video: &Path, video_modified: SystemTime) -> Result<Lookup, Error> {
    let sidecar = sidecar_of(video)?;
    let found = judge(read(&sidecar), video_modified);
    match found {
        Lookup::Invalid(why) => info!("[Sidecar] Ignoring {}: {why}", sidecar.display()),
        Lookup::Stale(_) => debug!("[Sidecar] {} is older than its video; hashing again", sidecar.display()),
        Lookup::Missing | Lookup::Fresh(_) => {}
    }
    Ok(found)
}

/// A sidecar file's content, before it is compared with its video.
pub(crate) enum Content {
    Missing,
    Invalid(Invalid),
    Valid(Md5Hex, SystemTime),
}

/// Reads the sidecar at `path` without following a symbolic link and without blocking on a pipe (rule 3).
pub(crate) fn read(path: &Path) -> Content {
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let mut file = match rustix::fs::open(path, flags, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT | Errno::NOTDIR) => return Content::Missing,
        Err(Errno::LOOP) => return Content::Invalid(Invalid::NotAFile),
        Err(errno) => return unreadable(path, &io::Error::from(errno)),
    };
    let meta = match file.metadata() {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return Content::Invalid(Invalid::NotAFile),
        Err(error) => return unreadable(path, &error),
    };
    let modified = match meta.modified() {
        Ok(modified) => modified,
        Err(error) => return unreadable(path, &error),
    };
    let mut content = Vec::with_capacity(64);
    if let Err(error) = (&mut file).take(READ_LIMIT).read_to_end(&mut content) {
        return unreadable(path, &error);
    }
    match hash::parse(&content) {
        Ok(hash) => Content::Valid(hash, modified),
        Err(why) => Content::Invalid(why),
    }
}

fn unreadable(path: &Path, error: &io::Error) -> Content {
    warn!("[Sidecar] Cannot read {}: {error}", path.display());
    Content::Invalid(Invalid::Unreadable)
}

pub(crate) fn judge(content: Content, video_modified: SystemTime) -> Lookup {
    match content {
        Content::Missing => Lookup::Missing,
        Content::Invalid(why) => Lookup::Invalid(why),
        Content::Valid(hash, modified) if is_fresh(modified, video_modified) => Lookup::Fresh(hash),
        Content::Valid(hash, _) => Lookup::Stale(hash),
    }
}

/// What [`write`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// A fresh sidecar already held this hash; nothing was written.
    Unchanged,
    /// There was no sidecar; one was written.
    Created,
    /// The sidecar was invalid or stale; it was rewritten.
    Rewritten,
    /// A fresh sidecar held another hash (logged as a warning); it was replaced.
    Replaced(Md5Hex),
}

/// Writes `hash` as `video`'s sidecar (rule 5), after the video's last write: `hashed` is the video's [`Stamp`] taken
/// before its bytes were read, and nothing is written when the video changed since ([`Error::VideoChanged`]).
///
/// A fresh sidecar with the same hash stays as it is; a missing, invalid or stale one is written (info); a fresh one
/// with another hash is replaced (warning), because `hash` comes from the bytes. The write is atomic: a temporary file
/// in the same folder, fsync, rename, fsync of the folder. The content is the 32 lowercase hex digits, nothing else.
/// The sidecar never ends up older than its video, even when the video's time lies in the future.
pub fn write(video: &Path, hash: Md5Hex, hashed: Stamp) -> Result<Written, Error> {
    let sidecar = sidecar_of(video)?;
    if (hashed.len == 0) != hash.is_zero_byte() {
        return Err(Error::ImpossibleHash {
            video: video.to_path_buf(),
            len: hashed.len,
            hash,
        });
    }
    let now = Stamp::of(video)?;
    if now != hashed {
        return Err(Error::VideoChanged(video.to_path_buf()));
    }
    let before = judge(read(&sidecar), now.modified);
    let written = match before {
        Lookup::Fresh(old) if old == hash => {
            debug!("[Sidecar] {} is up to date", sidecar.display());
            return Ok(Written::Unchanged);
        }
        Lookup::Fresh(old) => Written::Replaced(old),
        Lookup::Missing => Written::Created,
        Lookup::Invalid(_) | Lookup::Stale(_) => Written::Rewritten,
    };
    commit(&sidecar, hash, Time::NotBefore(now.modified))?;
    match before {
        Lookup::Fresh(old) => warn!(
            "[Sidecar] Replaced {}: it said {old}, but the video's bytes give {hash}",
            sidecar.display()
        ),
        Lookup::Invalid(why) => info!("[Sidecar] Rewrote {} ({why}): {hash}", sidecar.display()),
        Lookup::Stale(_) => info!("[Sidecar] Rewrote {} (older than its video): {hash}", sidecar.display()),
        Lookup::Missing => info!("[Sidecar] Wrote {}: {hash}", sidecar.display()),
    }
    Ok(written)
}

/// The modification time a committed sidecar gets.
pub(crate) enum Time {
    /// The time of writing, but not before this one (the video's time).
    NotBefore(SystemTime),
    /// Exactly this one (a sidecar carried to another place keeps its time).
    Exactly(SystemTime),
}

/// Writes `hash` to `sidecar` atomically: temporary file in the same folder, fsync, rename, fsync of the folder.
pub(crate) fn commit(sidecar: &Path, hash: Md5Hex, time: Time) -> Result<(), Error> {
    let mut file = AtomicWriteFile::open(sidecar).map_err(Error::io(sidecar))?;
    file.write_all(hash.as_bytes()).map_err(Error::io(sidecar))?;
    let set = match time {
        Time::NotBefore(earliest) => (SystemTime::now() < earliest).then_some(earliest),
        Time::Exactly(time) => Some(time),
    };
    if let Some(time) = set {
        file.as_file().set_modified(time).map_err(Error::io(sidecar))?;
    }
    file.commit().map_err(Error::io(sidecar))
}
