//! Interface of the RustView sync actor (implemented in M3.1). Each job owns the metadata it wants in RustView; the actor
//! makes RustView match it, remembers what it did in a [`JobSyncRecord`] (stored with the job), and cleans up only what
//! RustCut created (ADR-6).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::client::RustViewError;
use super::models::{GetMedia, HashCheck, TagTree};
use crate::actor::Reply;
use crate::job::{JobId, JobMetadata};

/// Whether RustView can be reached. Shown in one place: the Tag & Rate panel header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Unknown,
    Connected,
    /// The last call failed for a transient reason; the actor retries after `retry_in_secs`.
    Unavailable {
        reason: String,
        retry_in_secs: u32,
    },
    /// Switched off in the settings.
    Disabled,
}

/// Why the job is being synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    /// The job was queued (write timing "at enqueue": a pending stub).
    Enqueued,
    /// Tags, rating or the target of a waiting job changed.
    Edited,
    /// The render was verified: the file exists now; the metadata is asserted once more.
    Finished,
}

/// Where the job's RustView record stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SyncState {
    /// Nothing written yet (write timing "after render", or RustView disabled).
    #[default]
    NotSynced,
    /// Waiting to be written (RustView unreachable, or queued behind other calls).
    Pending,
    Synced,
    Failed {
        error: String,
    },
    /// The record RustCut created was removed (job failed, canceled, deleted or renamed), or a preexisting record got its
    /// tags and rating back.
    CleanedUp,
}

/// What the sync actor did for one job (JSON in the `sync` column of the job).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobSyncRecord {
    pub state: SyncState,
    /// The path the record was written for; a changed target moves the record.
    pub synced_path: Option<PathBuf>,
    /// The metadata last written.
    pub synced: Option<JobMetadata>,
    /// Whether RustView had a record for the target before RustCut wrote to it (`None` = not asked yet). A preexisting record
    /// is never deleted by a cleanup.
    pub preexisting: Option<bool>,
    /// Tags and rating of the preexisting record, restored on cleanup.
    pub previous_tags: Option<Vec<u32>>,
    pub previous_rating: Option<u8>,
    /// Failed attempts in a row (for the back-off).
    pub attempts: u32,
}

/// Commands of the sync actor.
#[derive(Debug)]
pub enum SyncCommand {
    /// Make RustView match the job.
    Job {
        id: JobId,
        target: PathBuf,
        metadata: JobMetadata,
        phase: SyncPhase,
        record: JobSyncRecord,
    },
    /// The job failed, was canceled, deleted or renamed: undo what RustCut created for `record`.
    Cleanup {
        id: JobId,
        record: JobSyncRecord,
    },
    TagTree {
        reply: Reply<Result<Arc<TagTree>, RustViewError>>,
    },
    GetMedia {
        path: PathBuf,
        reply: Reply<Result<GetMedia, RustViewError>>,
    },
    CheckHash {
        md5: String,
        reply: Reply<Result<HashCheck, RustViewError>>,
    },
    /// Register a source hash (only when a job of that source is created, not on open: PyCutRv bug 3).
    InsertHash {
        md5: String,
    },
    /// Try the connection now (the Retry button) instead of waiting for the back-off.
    Probe,
    /// The settings changed (enabled, binary, timeout, write timing).
    Reconfigure {
        settings: crate::settings::RustViewSettings,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

/// Events of the sync actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    Connection(ConnectionState),
    /// A job's record changed (the queue engine stores it with the job).
    JobSynced {
        id: JobId,
        record: JobSyncRecord,
    },
    /// The tag tree was loaded again (after connecting, or when RustView changed it).
    TagTreeChanged(Arc<TagTree>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_records_round_trip_and_default() {
        let record = JobSyncRecord {
            state: SyncState::Failed {
                error: "timeout".into(),
            },
            synced_path: Some("/out/a.mkv".into()),
            synced: Some(JobMetadata {
                tags: vec![1],
                rating: Some(3),
                source_hash: None,
            }),
            preexisting: Some(true),
            previous_tags: Some(vec![9]),
            previous_rating: Some(5),
            attempts: 2,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<JobSyncRecord>(&json).unwrap(), record);
        assert_eq!(
            serde_json::from_str::<JobSyncRecord>("{}").unwrap(),
            JobSyncRecord::default()
        );
    }

    #[test]
    fn transient_errors_are_retried() {
        assert!(RustViewError::Timeout(10).is_transient());
        assert!(RustViewError::Unavailable("not found".into()).is_transient());
        assert!(!RustViewError::Api("bad tag".into()).is_transient());
    }
}
