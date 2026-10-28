//! The picture store's thread (ADR-4): the only owner of `pictures.sqlite`. Importing reads the picked file here too,
//! so neither the UI thread nor the runtime waits for the decoders.

use std::collections::HashSet;
use std::io::Read as _;
use std::path::PathBuf;
use std::thread::JoinHandle;

use super::import::{MAX_INPUT, PictureError};
use super::store::{PictureStore, StoredPicture};
use crate::actor::{Handle, Reply, mailbox};

type Answer<T> = Reply<Result<T, PictureError>>;

#[derive(Debug)]
pub enum PictureCommand {
    /// Reads `path` (at most 32 MiB), prepares and stores it.
    Import {
        path: PathBuf,
        reply: Answer<StoredPicture>,
    },
    Get {
        hash: String,
        reply: Answer<Option<StoredPicture>>,
    },
    /// Removes the pictures not in `keep` (fire and forget; the count is logged).
    Retain {
        keep: HashSet<String>,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

pub type PictureHandle = Handle<PictureCommand>;

/// Reads a file the user picked, refusing what is larger than the store takes.
fn read_limited(path: &std::path::Path) -> Result<Vec<u8>, PictureError> {
    let file = std::fs::File::open(path).map_err(|e| PictureError::Unreadable(e.to_string()))?;
    let mut data = Vec::new();
    file.take(MAX_INPUT.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|e| PictureError::Unreadable(e.to_string()))?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_INPUT {
        return Err(PictureError::TooLarge);
    }
    Ok(data)
}

/// Starts the thread with an open store.
pub fn spawn_picture_store(store: PictureStore) -> std::io::Result<(PictureHandle, JoinHandle<()>)> {
    let (handle, mut rx) = mailbox::<PictureCommand>();
    let thread = std::thread::Builder::new()
        .name("rustcut-pictures".into())
        .spawn(move || {
            while let Some(command) = rx.blocking_recv() {
                match command {
                    PictureCommand::Import { path, reply } => {
                        let result = read_limited(&path).and_then(|data| store.import(&data));
                        match &result {
                            Ok(p) => tracing::info!(hash = %p.hash, kind = p.kind.id(), "[Pictures] Picture stored"),
                            Err(e) => tracing::warn!(error = %e, "[Pictures] Picture refused"),
                        }
                        let _ = reply.send(result);
                    }
                    PictureCommand::Get { hash, reply } => {
                        let _ = reply.send(store.get(&hash));
                    }
                    PictureCommand::Retain { keep } => match store.retain(&keep) {
                        Ok(0) => {}
                        Ok(n) => tracing::info!(removed = n, "[Pictures] Unused pictures removed"),
                        Err(e) => tracing::warn!(error = %e, "[Pictures] Unused pictures not removed"),
                    },
                    PictureCommand::Shutdown { reply } => {
                        drop(store);
                        let _ = reply.send(());
                        return;
                    }
                }
            }
        })?;
    Ok((handle, thread))
}
