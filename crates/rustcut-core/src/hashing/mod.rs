//! The hash service (M2.1): MD5 of a source for RustView's known-source check, taken from a sidecar `.<name>.md5` or
//! computed in the background (cancelable, progress shown non-modally; PyCutRv hashed on the UI thread, bug 4). Taking
//! the hash from RustView itself (`get-media`, no disk read) joins with the RustView client (M3.1).

mod service;

use std::path::{Path, PathBuf};

pub use service::{HashHandle, HashService, PROGRESS_INTERVAL, spawn_hash_service};

use crate::actor::Reply;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HashError {
    #[error("{path}: {error}")]
    Io { path: PathBuf, error: String },
    #[error("hashing was canceled")]
    Canceled,
}

#[derive(Debug)]
pub enum HashCommand {
    /// Hashes `path` (sidecar first); the answer also goes out as [`HashEvent::Finished`].
    Hash {
        path: PathBuf,
        reply: Option<Reply<Result<String, HashError>>>,
    },
    Cancel {
        path: PathBuf,
    },
    /// Whether a computed hash is written into a new sidecar next to the video (settings `hashing.write_sidecar`; off
    /// until the app says otherwise).
    SetWriteSidecar(bool),
    Shutdown {
        reply: Reply<()>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashEvent {
    Progress {
        path: PathBuf,
        done: u64,
        total: u64,
    },
    Finished {
        path: PathBuf,
        md5: String,
        from_sidecar: bool,
    },
    Failed {
        path: PathBuf,
        error: String,
    },
    Canceled {
        path: PathBuf,
    },
}

/// The sidecar of `video`: `.<file name>.md5` in the same folder (md5-sidecar rule 1, shared with RustKnown and
/// RustTransferFrom). RustCut never deletes a source's sidecar (PyCutRv bug 11).
pub fn sidecar_path(video: &Path) -> Option<PathBuf> {
    md5_sidecar::path_for(video)
}

/// The MD5 in a sidecar (md5-sidecar rule 3): the bare hash or `md5sum` output (`<hash>  <name>`); lower-cased, `None`
/// when it does not start with 32 hex digits.
pub fn parse_sidecar(text: &str) -> Option<String> {
    md5_sidecar::parse(text.as_bytes()).ok().map(|hash| hash.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_naming() {
        assert_eq!(
            sidecar_path(Path::new("/v/clip one.mp4")),
            Some(PathBuf::from("/v/.clip one.mp4.md5"))
        );
        assert_eq!(sidecar_path(Path::new("/")), None);
    }

    #[test]
    fn sidecar_contents() {
        let hash = "D41D8CD98F00B204E9800998ECF8427E";
        assert_eq!(
            parse_sidecar(&format!("{hash}  clip.mp4\n")).as_deref(),
            Some("d41d8cd98f00b204e9800998ecf8427e")
        );
        assert_eq!(
            parse_sidecar("d41d8cd98f00b204e9800998ecf8427e").as_deref(),
            Some("d41d8cd98f00b204e9800998ecf8427e")
        );
        assert_eq!(parse_sidecar("xyz  clip.mp4"), None);
        assert_eq!(parse_sidecar(""), None);
    }
}
