//! The RustView client interface. The CLI transport ([`super::CliClient`]: `rustview api <cmd> --json`, exit codes 0 and
//! 2, timeout) implements it; tests use fakes. Calls are made only by the sync actor (serialized; RustView's LMDB has one writer anyway).

use std::future::Future;
use std::path::Path;

use super::models::{BatchHashCheck, DeleteMedia, GetMedia, HashCheck, HashInsert, SetMedia, TagTree};

/// The `--source` value RustCut writes into records it registers.
pub const SOURCE_APP: &str = "RustCut";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RustViewError {
    /// RustView is not installed or its binary cannot be started (the panel shows how to fix it).
    #[error("RustView cannot be started: {0}")]
    Unavailable(String),
    #[error("RustView did not answer within {0} s")]
    Timeout(u8),
    /// RustView answered with `success: false`.
    #[error("RustView reported: {0}")]
    Api(String),
    /// The answer could not be decoded (a RustView version with another API).
    #[error("RustView's answer could not be read: {0}")]
    Decode(String),
}

impl RustViewError {
    /// Whether a retry later can help (the sync actor backs off and tries again).
    pub const fn is_transient(&self) -> bool {
        matches!(self, Self::Unavailable(_) | Self::Timeout(_))
    }
}

pub type Result<T> = std::result::Result<T, RustViewError>;

/// RustView's API as RustCut uses it. Futures are `Send` so the sync actor can run on the multi-thread runtime.
pub trait RustViewClient: Send + Sync + 'static {
    fn tag_tree(&self) -> impl Future<Output = Result<TagTree>> + Send;

    /// `found: false` (exit code 2) is a normal answer, not an error.
    fn get_media(&self, path: &Path) -> impl Future<Output = Result<GetMedia>> + Send;

    /// Creates or updates a record; `tags`/`rating`/`hash` `None` leave the stored value alone, an empty tag list clears
    /// the tags. The record keeps its source app (a new one gets RustView's default).
    fn set_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> impl Future<Output = Result<SetMedia>> + Send;

    /// Like [`set_media`](Self::set_media), and marks the record as RustCut's ([`SOURCE_APP`]): for records RustCut
    /// creates (pending stubs that RustView completes once the file exists). A record that existed before RustCut wrote
    /// to it is written with `set_media`, so its source stays.
    fn set_own_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> impl Future<Output = Result<SetMedia>> + Send;

    fn delete_media(&self, path: &Path) -> impl Future<Output = Result<DeleteMedia>> + Send;

    fn check_hash(&self, md5: &str) -> impl Future<Output = Result<HashCheck>> + Send;

    fn check_hashes(&self, md5s: &[String]) -> impl Future<Output = Result<BatchHashCheck>> + Send;

    fn insert_hash(&self, md5: &str) -> impl Future<Output = Result<HashInsert>> + Send;
}
