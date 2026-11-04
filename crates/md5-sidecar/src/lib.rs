//! md5 sidecars, the same in every app (`SPEC.md`, rules v1).
//!
//! A sidecar is the file `.<video name>.md5` next to a video. It holds the video's MD5 as 32 lowercase hex digits and
//! nothing else, so it survives renames. RustCut, RustTransferFrom and RustKnownClean read, write, move and clean up
//! sidecars only through this crate; each vendors it byte-identical from the canonical copy in RustKnownClean
//! (`crates/md5-sidecar`, kept in sync by `scripts/sync-md5-sidecar.sh`).
//!
//! - Reading: [`lookup`] says whether a video's sidecar is missing, invalid, stale or fresh; only a fresh one replaces
//!   hashing the video. [`parse`] reads the bare hash and md5sum's `<hash>  <name>` line alike.
//! - Writing: take a [`Stamp`] of the video before hashing it, then [`write`].
//! - Moving, copying, trashing, deleting a video: [`move_along`], [`copy_along`], [`trash_along`], [`remove_along`]
//!   right after the video's own operation. [`orphans_in`] finds sidecars whose video is gone.

mod carry;
mod file;
mod hash;
mod name;

pub use carry::{Carried, LEFTOVER_AGE, copy_along, is_orphan, move_along, orphans_in, remove_along, trash_along};
pub use file::{Error, Lookup, Stamp, Written, is_fresh, lookup, lookup_at, write};
pub use hash::{Invalid, MAX_LEN, Md5Hex, ZERO_BYTE_MD5, parse};
pub use name::{EXTENSION, is_sidecar_name, is_write_leftover, path_for, video_for, video_name};

/// The version of the rules in `SPEC.md` this crate implements.
pub const SPEC_VERSION: u32 = 1;
