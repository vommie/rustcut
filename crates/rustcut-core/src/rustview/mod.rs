//! The RustView link (ADR-6): typed models of `rustview api <cmd> --json`, the [`RustViewClient`] trait with its CLI
//! transport ([`CliClient`]), and the sync actor that makes RustView match each job.

mod cli;
mod client;
mod models;
mod source;
mod sync;

pub use cli::{CliClient, DEFAULT_BINARY, decode};
pub use client::{RustViewClient, RustViewError, SOURCE_APP};
pub use models::{
    ApiErrorResponse, BatchHashCheck, DeleteMedia, GetMedia, HashCheck, HashCheckItem, HashInsert, MediaDetails,
    MoveMedia, MoveMediaAction, SetMedia, SetMediaAction, TagItem, TagTree,
};
pub use source::{is_md5, trusted_hash};
#[cfg(test)]
pub(crate) use sync::fake;
pub use sync::{
    ConnectionState, JobSyncRecord, SyncCommand, SyncEvent, SyncHandle, SyncPhase, SyncRecords, SyncService, SyncState,
    spawn_sync,
};
