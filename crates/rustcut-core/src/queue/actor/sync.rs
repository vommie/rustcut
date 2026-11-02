//! The queue's side of the RustView sync (ADR-6): the queue tells the sync actor what each job wants whenever a job is
//! added, edited, finished, failed, canceled or removed, and stores every record the sync actor reports in the job's
//! `sync` column (`StoreCommand::SetSync`), so the queue stays the only writer of its rows.

use super::Actor;
use crate::job::{JobId, JobState, JobStateKind};
use crate::rustview::{JobSyncRecord, SyncCommand, SyncHandle, SyncPhase, SyncRecords, SyncState};
use crate::store::{JobRecord, SpecSlot, StoreCommand};

/// The sync actor a queue works with: its commands, and the records it reports.
#[derive(Debug)]
pub struct SyncLink {
    pub handle: SyncHandle,
    pub records: SyncRecords,
}

impl Actor {
    fn sync_send(&self, command: SyncCommand) {
        if let Some(sync) = &self.sync
            && sync.send(command).is_err()
        {
            tracing::debug!("[Queue] The RustView sync has stopped");
        }
    }

    /// Asks the sync actor to make RustView match what the job wants now. A job saved without RustView (the Tag & Rate
    /// panel's "Active" switch was off, user decision Q-P8b) is never sent.
    pub(super) fn sync_job(&self, id: JobId, phase: SyncPhase) {
        let Some(record) = self.model.get(id) else { return };
        let SpecSlot::Decoded(spec) = &record.spec else { return };
        if spec.metadata.without_rustview {
            tracing::debug!(job = id.0, "[Queue] Saved without RustView; nothing is sent");
            return;
        }
        self.sync_send(SyncCommand::Job {
            id,
            target: record.target_path.clone(),
            metadata: spec.metadata.clone(),
            phase,
            record: JobSyncRecord::from_column(record.sync.as_deref()),
        });
    }

    /// Asks the sync actor to undo what it wrote for the job (a job that ended without a file, or was removed). Nothing
    /// was written for a job saved without RustView.
    pub(super) fn sync_cleanup(&self, record: &JobRecord) {
        if record.sync.is_none() && without_rustview(record) {
            return;
        }
        self.sync_send(SyncCommand::Cleanup {
            id: record.id,
            record: JobSyncRecord::from_column(record.sync.as_deref()),
        });
    }

    /// What a state change means for RustView: a verified file is asserted once more, a job that ended without a file
    /// loses what RustCut created, a restarted job wants its record again.
    pub(super) fn sync_state_changed(&self, id: JobId, from: JobStateKind, to: &JobState) {
        match to.kind() {
            // A file that still moves is asserted at its new place once it got there.
            JobStateKind::Finished if self.journal(id).is_some() => {}
            JobStateKind::Finished => self.sync_job(id, SyncPhase::Finished),
            JobStateKind::Failed | JobStateKind::Canceled => {
                if let Some(record) = self.model.get(id) {
                    self.sync_cleanup(record);
                }
            }
            JobStateKind::Waiting if from.is_done() => self.sync_job(id, SyncPhase::Enqueued),
            _ => {}
        }
    }

    /// At start: everything the sync actor has to finish from the last run. Jobs still to render want their records
    /// (the sync actor calls nothing for a record that is up to date); a finished job whose last write did not happen
    /// is asserted again; a failed or canceled job whose record was not cleaned up is cleaned up.
    pub(super) fn sync_resume(&self) {
        if self.sync.is_none() {
            return;
        }
        for job in self.model.jobs() {
            let record = JobSyncRecord::from_column(job.sync.as_deref());
            let unsettled = matches!(record.state, SyncState::Pending | SyncState::Failed { .. });
            match job.state.kind() {
                JobStateKind::Waiting | JobStateKind::Held | JobStateKind::Rendering | JobStateKind::Paused => {
                    self.sync_job(job.id, SyncPhase::Enqueued);
                }
                JobStateKind::Finished if unsettled || (record.state == SyncState::Synced && !record.finished) => {
                    self.sync_job(job.id, SyncPhase::Finished);
                }
                JobStateKind::Failed | JobStateKind::Canceled
                    if record.synced_path.is_some() && record.state != SyncState::CleanedUp =>
                {
                    self.sync_cleanup(job);
                }
                _ => {}
            }
        }
    }

    /// The sync actor changed a job's record: store it with the job and show it in the row.
    pub(super) async fn sync_reported(&mut self, id: JobId, record: JobSyncRecord) {
        let Some(mut job) = self.model.get(id).cloned() else {
            tracing::debug!(job = id.0, "[Queue] Sync record of a removed job dropped");
            return;
        };
        let sync = record.to_column();
        if job.sync == sync {
            return;
        }
        job.sync.clone_from(&sync);
        self.model.replace(job);
        if let Err(e) = self
            .store_request(|reply| StoreCommand::SetSync { id, sync, reply })
            .await
        {
            tracing::warn!(job = id.0, error = %e, "[Queue] Sync record not saved");
        }
        self.publish_row(id);
    }
}

/// Whether the job was saved without RustView (an unreadable spec counts as with: its record, if any, is cleaned up).
fn without_rustview(record: &JobRecord) -> bool {
    matches!(&record.spec, SpecSlot::Decoded(spec) if spec.metadata.without_rustview)
}

/// The next record the sync actor reports; never for a queue without a sync actor.
pub(super) async fn next_record(records: &mut Option<SyncRecords>) -> Option<(JobId, JobSyncRecord)> {
    match records {
        Some(records) => records.recv().await,
        None => std::future::pending().await,
    }
}
