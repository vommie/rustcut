//! The message about a job's tags and rating that RustView refused (0.0.67, user review): a refusal is not retried,
//! so it is told once per job (a keyed toast, ended when the record gets there after all). An unreachable RustView is
//! retried and shown by the Tag & Rate panel instead.

use rustcut_core::job::JobId;
use rustcut_core::rustview::{JobSyncRecord, SyncState};

use crate::ui::jump::JumpTarget;
use crate::ui::notices::{self, Kind, Notice};

/// A job's record changed.
pub fn job_synced(id: JobId, record: &JobSyncRecord) {
    let key = format!("sync_failed:{id}");
    match &record.state {
        SyncState::Failed { error } => {
            tracing::warn!(%id, error, "[Tags] RustView refused the job's tags and rating");
            notices::post(
                Notice::new(Kind::SyncFailed, format!("Tags of job {id} not written to RustView"))
                    .detail(error.clone())
                    .key(key)
                    .jump_action("Show the job", JumpTarget::QueueJob(id)),
            );
        }
        SyncState::Synced => notices::clear(&key),
        _ => {}
    }
}
