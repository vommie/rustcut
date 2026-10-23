//! The RustView link (ADR-6): typed models of `rustview api <cmd> --json`, the [`RustViewClient`] trait (the CLI transport
//! comes in M3.1) and the interface of the sync actor that makes RustView match each job.

mod client;
mod models;
mod sync;

pub use client::{RustViewClient, RustViewError, SOURCE_APP};
pub use models::{
    ApiErrorResponse, BatchHashCheck, DeleteMedia, GetMedia, HashCheck, HashCheckItem, HashInsert, MediaDetails,
    SetMedia, SetMediaAction, TagItem, TagTree,
};
pub use sync::{ConnectionState, JobSyncRecord, SyncCommand, SyncEvent, SyncPhase, SyncState};
