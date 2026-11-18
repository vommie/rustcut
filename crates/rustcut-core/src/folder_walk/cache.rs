//! One cached listing, of one folder (the folder of the last "next file"), kept only while a watcher reports every
//! change of it (user request 2026-11-18: never miss a new file; one cache and one watcher at a time).
//!
//! - The folder must lie on a local file system (ext4, btrfs, xfs, tmpfs, ...): there inotify sees every change. On a
//!   network or FUSE mount (NFS, SMB, sshfs, rclone) another computer's changes reach no watcher, so such folders are
//!   always read anew.
//! - The watcher starts before the folder is read; a listing is kept only when no change came in between, and is used
//!   only while none came since: no event (any change, an overflow, a watcher error, the folder's own removal), and
//!   the folder's change and status times as they were. Those times answer at once, while inotify's events arrive a
//!   moment later: a file made just before the key press is never missed.
//! - Another folder drops the watcher and every listing of the old one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use super::natural::SortingChoice;
use super::order::SortSpec;

/// What makes two listings of the folder the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Variant {
    pub sort: SortSpec,
    pub choice: SortingChoice,
    pub hidden: bool,
    pub videos_only: bool,
}

/// A read started at this state: the events counted so far and the folder's times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ticket {
    folder: PathBuf,
    events: u64,
    stamp: Stamp,
}

/// The folder's change and status times (nanoseconds); every new, removed or renamed entry moves them.
type Stamp = Option<(i64, u64, i64, u64)>;

fn stamp(folder: &Path) -> Stamp {
    let stat = rustix::fs::stat(folder).ok()?;
    Some((stat.st_mtime, stat.st_mtime_nsec, stat.st_ctime, stat.st_ctime_nsec))
}

struct Watched {
    folder: PathBuf,
    // Dropping it ends the watch.
    _watcher: notify::RecommendedWatcher,
    events: Arc<AtomicU64>,
    listings: HashMap<Variant, (Ticket, Arc<Vec<PathBuf>>)>,
}

/// The cache; one per plugin service.
#[derive(Default)]
pub struct FolderCache {
    watched: Mutex<Option<Watched>>,
}

impl std::fmt::Debug for FolderCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let folder = self.watched.lock().as_ref().map(|w| w.folder.clone());
        f.debug_struct("FolderCache").field("folder", &folder).finish()
    }
}

impl FolderCache {
    /// The kept listing of `folder` for `variant`, if nothing changed since it was read.
    pub(super) fn get(&self, folder: &Path, variant: &Variant) -> Option<Arc<Vec<PathBuf>>> {
        let watched = self.watched.lock();
        let watched = watched.as_ref().filter(|w| w.folder == folder)?;
        let (ticket, files) = watched.listings.get(variant)?;
        let now = Ticket {
            folder: folder.to_path_buf(),
            events: watched.events.load(Ordering::Acquire),
            stamp: stamp(folder),
        };
        (now == *ticket && ticket.stamp.is_some()).then(|| Arc::clone(files))
    }

    /// Before reading `folder`: watches it (dropping the watch of any other folder) and notes the counters; `None`
    /// when its changes cannot be seen reliably (a network mount, no watcher).
    pub(super) fn begin(&self, folder: &Path) -> Option<Ticket> {
        let mut watched = self.watched.lock();
        if watched.as_ref().is_some_and(|w| w.folder != folder) {
            tracing::debug!("[NextInFolder] Another folder: the old listings and their watcher go");
            *watched = None;
        }
        if watched.is_none() {
            if !reliable(folder) {
                return None;
            }
            *watched = watch(folder);
        }
        let watched = watched.as_ref()?;
        Some(Ticket {
            folder: folder.to_path_buf(),
            events: watched.events.load(Ordering::Acquire),
            stamp: stamp(folder),
        })
    }

    /// Keeps a listing read after `ticket`, if no change came in meanwhile.
    pub(super) fn put(&self, ticket: Ticket, variant: Variant, files: Arc<Vec<PathBuf>>) {
        let mut watched = self.watched.lock();
        let Some(watched) = watched.as_mut().filter(|w| w.folder == ticket.folder) else {
            return;
        };
        let unchanged =
            watched.events.load(Ordering::Acquire) == ticket.events && stamp(&ticket.folder) == ticket.stamp;
        if unchanged && ticket.stamp.is_some() {
            watched.listings.insert(variant, (ticket, files));
        }
    }
}

fn watch(folder: &Path) -> Option<Watched> {
    use notify::Watcher as _;
    let events = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&events);
    // Every event counts but a read (opening or closing a file or the folder unwritten, as reading the folder does);
    // an error counts too: a lost event must never keep a listing.
    let handler = move |event: notify::Result<notify::Event>| {
        let read_only = event
            .as_ref()
            .is_ok_and(|e| matches!(e.kind, notify::EventKind::Access(_)) && !e.need_rescan());
        if !read_only {
            seen.fetch_add(1, Ordering::AcqRel);
        }
    };
    let mut watcher = notify::recommended_watcher(handler)
        .inspect_err(|e| tracing::info!("[NextInFolder] No watcher, the folder is read every time: {e}"))
        .ok()?;
    watcher
        .watch(folder, notify::RecursiveMode::NonRecursive)
        .inspect_err(|e| {
            tracing::info!(
                "[NextInFolder] {} cannot be watched, it is read every time: {e}",
                folder.display()
            );
        })
        .ok()?;
    tracing::debug!("[NextInFolder] Watching {}", folder.display());
    Some(Watched {
        folder: folder.to_path_buf(),
        _watcher: watcher,
        events,
        listings: HashMap::new(),
    })
}

/// File systems whose every change inotify reports (local block devices and memory). FUSE, overlay and network file
/// systems are left out: changes made elsewhere (another computer, a lower layer) pass by the watcher.
const LOCAL: [i128; 13] = [
    0xEF53,      // ext2, ext3, ext4
    0x9123_683E, // btrfs
    0x5846_5342, // xfs
    0x0102_1994, // tmpfs
    0xF2F5_2010, // f2fs
    0x2FC1_2FC1, // zfs
    0xCA45_1A4E, // bcachefs
    0x3153_464A, // jfs
    0x5265_4973, // reiserfs
    0x3434,      // nilfs2
    0x7366_746E, // ntfs3 (the kernel's driver)
    0x2011_BAB0, // exfat
    0x4D44,      // vfat
];

/// Whether inotify sees every change of `folder`.
#[must_use]
pub fn reliable(folder: &Path) -> bool {
    rustix::fs::statfs(folder).is_ok_and(|stat| local_kind(i128::from(stat.f_type)))
}

fn local_kind(kind: i128) -> bool {
    LOCAL.contains(&kind)
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
