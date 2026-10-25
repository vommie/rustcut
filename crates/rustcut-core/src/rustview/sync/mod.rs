//! Interface of the RustView sync actor (ADR-6). Each job owns the metadata it wants in RustView; the actor makes
//! RustView match it, remembers what it did in a [`JobSyncRecord`] (stored with the job), and cleans up only what
//! RustCut created. [`plan`] holds the pure decisions, [`steps`] the calls of one job, [`actor`] the loop that serializes
//! them, retries while RustView is unreachable and answers the app's lookups.

mod actor;
#[cfg(test)]
mod actor_tests;
#[cfg(test)]
pub(crate) mod fake;
mod plan;
mod steps;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::client::RustViewError;
use super::models::{GetMedia, HashCheck, TagTree};
use crate::actor::{EventBus, Handle, Reply};
use crate::job::{JobId, JobMetadata};

pub use actor::spawn_sync;

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
    /// The metadata was written once more after the render was verified (the file existed then).
    pub finished: bool,
    /// The job's source hash is in RustView's hash list (`insert-hash`, done once when the job is created).
    pub hash_registered: bool,
}

impl JobSyncRecord {
    /// The record in a job's `sync` column; a missing or unreadable column is a record of a job never synced.
    pub fn from_column(column: Option<&str>) -> Self {
        column
            .and_then(|json| match serde_json::from_str(json) {
                Ok(record) => Some(record),
                Err(e) => {
                    tracing::warn!(error = %e, "[RustView] A job's sync record cannot be read; treated as not synced");
                    None
                }
            })
            .unwrap_or_default()
    }

    /// The JSON for the `sync` column.
    pub fn to_column(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

pub type SyncHandle = Handle<SyncCommand>;

/// Every changed job record, in order, for the queue engine that stores it with the job (an unbounded channel, so no
/// record is lost to a lagging subscriber; the same records are also published as [`SyncEvent::JobSynced`]).
pub type SyncRecords = mpsc::UnboundedReceiver<(JobId, JobSyncRecord)>;

/// The running sync actor.
#[derive(Debug, Clone)]
pub struct SyncService {
    pub handle: SyncHandle,
    pub events: EventBus<SyncEvent>,
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
                without_rustview: false,
            }),
            preexisting: Some(true),
            previous_tags: Some(vec![9]),
            previous_rating: Some(5),
            attempts: 2,
            finished: true,
            hash_registered: true,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<JobSyncRecord>(&json).unwrap(), record);
        assert_eq!(
            serde_json::from_str::<JobSyncRecord>("{}").unwrap(),
            JobSyncRecord::default()
        );
        assert_eq!(JobSyncRecord::from_column(record.to_column().as_deref()), record);
        assert_eq!(JobSyncRecord::from_column(Some("[1")), JobSyncRecord::default());
    }

    #[test]
    fn records_written_by_0_0_10_still_load() {
        // M0.3's shape, before `finished` and `hash_registered` existed.
        let old = r#"{"state":{"state":"synced"},"synced_path":"/out/a.mkv","synced":{"tags":[1],"rating":3},"preexisting":false,"previous_tags":null,"previous_rating":null,"attempts":0}"#;
        let record = JobSyncRecord::from_column(Some(old));
        assert_eq!(record.state, SyncState::Synced);
        assert_eq!(record.preexisting, Some(false));
        assert!(!record.finished && !record.hash_registered);
    }

    #[test]
    fn transient_errors_are_retried() {
        assert!(RustViewError::Timeout(10).is_transient());
        assert!(RustViewError::Unavailable("not found".into()).is_transient());
        assert!(!RustViewError::Api("bad tag".into()).is_transient());
    }
}
