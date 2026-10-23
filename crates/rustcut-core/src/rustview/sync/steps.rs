//! The calls of one job's sync, in the order ADR-6 gives them: register the source hash, ask `get-media` whether the
//! target already has a record (remembered in the [`JobSyncRecord`]), write; and the cleanup that deletes only a stub
//! RustCut created. Each step returns the record with everything it learned, also when a later call fails.

use std::path::{Path, PathBuf};

use super::plan::{self, Cleanup};
use super::{JobSyncRecord, SyncPhase, SyncState};
use crate::job::{JobId, JobMetadata};
use crate::rustview::{GetMedia, RustViewClient, RustViewError, SOURCE_APP};
use crate::settings::WriteTiming;

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    Done,
    /// RustView could not be reached: the work stays and is tried again after the back-off.
    Retry(RustViewError),
    /// RustView refused or answered something unreadable: the job's record says why; no automatic retry.
    Failed(RustViewError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepResult {
    pub record: JobSyncRecord,
    pub outcome: Outcome,
    /// RustView answered at least one call (it is reachable).
    pub answered: bool,
}

/// What a write needs to know about the job.
#[derive(Debug, Clone)]
pub(super) struct WriteWork {
    pub id: JobId,
    pub target: PathBuf,
    pub metadata: JobMetadata,
    pub phase: SyncPhase,
    pub record: JobSyncRecord,
    pub timing: WriteTiming,
}

/// Records the calls' answers and turns the first failure into the step's result.
struct Step {
    record: JobSyncRecord,
    answered: bool,
}

impl Step {
    fn new(record: JobSyncRecord) -> Self {
        Self {
            record,
            answered: false,
        }
    }

    /// The answer of one call; `Err` ends the step with the record as far as it got.
    fn take<T>(&mut self, answer: Result<T, RustViewError>) -> Result<T, Box<StepResult>> {
        match answer {
            Ok(value) => {
                self.answered = true;
                Ok(value)
            }
            Err(error) => Err(Box::new(self.fail(error))),
        }
    }

    /// The step's result after a failed call.
    fn fail(&mut self, error: RustViewError) -> StepResult {
        let mut record = self.record.clone();
        record.attempts = record.attempts.saturating_add(1);
        let outcome = if error.is_transient() {
            record.state = SyncState::Pending;
            Outcome::Retry(error)
        } else {
            self.answered = true;
            record.state = SyncState::Failed {
                error: error.to_string(),
            };
            Outcome::Failed(error)
        };
        StepResult {
            record,
            outcome,
            answered: self.answered,
        }
    }

    fn done(self) -> StepResult {
        StepResult {
            record: self.record,
            outcome: Outcome::Done,
            answered: self.answered,
        }
    }
}

/// Whether a file is at `path`; when that cannot be told, it counts as there (a record is never deleted on a guess).
async fn file_exists(path: &Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(true)
}

/// Makes RustView hold what the job wants at its target (the record was cleaned up first if it was written elsewhere).
pub(super) async fn write<C: RustViewClient>(client: &C, work: WriteWork) -> StepResult {
    match write_inner(client, &work).await {
        Ok(step) => step,
        Err(step) => *step,
    }
}

async fn write_inner<C: RustViewClient>(client: &C, work: &WriteWork) -> Result<StepResult, Box<StepResult>> {
    let mut step = Step::new(work.record.clone());
    let target = work.target.as_path();
    if !step.record.hash_registered
        && let Some(hash) = plan::valid_hash(&work.metadata)
    {
        match client.insert_hash(hash).await {
            Ok(answer) => {
                step.answered = true;
                step.record.hash_registered = true;
                tracing::info!(
                    job = work.id.0,
                    new = answer.is_new,
                    "[RustView] Source hash registered"
                );
            }
            Err(e) if !e.is_transient() => {
                // A refused hash must not block the job's record; it is not asked again.
                step.answered = true;
                step.record.hash_registered = true;
                tracing::warn!(job = work.id.0, error = %e, "[RustView] The source hash was refused");
            }
            Err(e) => return Err(Box::new(step.fail(e))),
        }
    }
    if !plan::writes(work.timing, work.phase, &step.record, target) {
        return Ok(step.done());
    }
    if work.phase == SyncPhase::Finished && !file_exists(target).await {
        tracing::warn!(job = work.id.0, target = %target.display(), "[RustView] The rendered file is gone; nothing written");
        return Ok(step.done());
    }
    if plan::in_sync(&step.record, target, &work.metadata, work.phase) {
        return Ok(step.done());
    }
    if step.record.preexisting.is_none() || !plan::synced_at(&step.record, target) {
        let found = step.take(client.get_media(target).await)?;
        learn(&mut step.record, &found);
        tracing::debug!(
            job = work.id.0,
            preexisting = step.record.preexisting,
            "[RustView] Asked whether the target has a record"
        );
    }
    let mut hash = step.record.synced.as_ref().and_then(|s| s.source_hash.clone());
    if let Some(args) = plan::write_args(&step.record, target, &work.metadata, work.phase) {
        let answer = if args.own {
            client
                .set_own_media(target, args.rating, args.tags.as_deref(), args.hash.as_deref())
                .await
        } else {
            client
                .set_media(target, args.rating, args.tags.as_deref(), args.hash.as_deref())
                .await
        };
        let answer = step.take(answer)?;
        if let Some(wanted) = &args.tags {
            let dropped: Vec<u32> = wanted.iter().copied().filter(|t| !answer.tags.contains(t)).collect();
            if !dropped.is_empty() {
                tracing::warn!(
                    job = work.id.0,
                    ?dropped,
                    "[RustView] Tags unknown to RustView were left out"
                );
            }
        }
        if args.hash.is_some() {
            hash = args.hash;
        }
        tracing::info!(
            job = work.id.0,
            action = ?answer.action,
            media = answer.media_id,
            phase = ?work.phase,
            "[RustView] Record written"
        );
    }
    let wanted = plan::wanted(&step.record, &work.metadata);
    step.record.synced = Some(wanted.as_metadata(hash.as_deref()));
    step.record.synced_path = Some(work.target.clone());
    step.record.state = SyncState::Synced;
    step.record.finished |= work.phase == SyncPhase::Finished;
    step.record.attempts = 0;
    Ok(step.done())
}

/// What `get-media` says about the target: a record there existed before RustCut wrote to it.
fn learn(record: &mut JobSyncRecord, found: &GetMedia) {
    if let Some(media) = found.media.as_ref().filter(|_| found.found) {
        record.preexisting = Some(true);
        record.previous_tags = Some(media.tags.clone());
        record.previous_rating = Some(media.rating);
    } else {
        record.preexisting = Some(false);
        record.previous_tags = None;
        record.previous_rating = None;
    }
}

/// Undoes what the job wrote ([`plan::cleanup`]). A record RustCut created is deleted only while its file does not exist
/// and the record still says RustCut made it; a preexisting record gets its tags and rating back.
pub(super) async fn clean<C: RustViewClient>(client: &C, id: JobId, record: JobSyncRecord) -> StepResult {
    match clean_inner(client, id, record).await {
        Ok(step) => step,
        Err(step) => *step,
    }
}

async fn clean_inner<C: RustViewClient>(
    client: &C,
    id: JobId,
    record: JobSyncRecord,
) -> Result<StepResult, Box<StepResult>> {
    let mut step = Step::new(record);
    let Some(path) = step.record.synced_path.clone() else {
        // Nothing was written; a write that was still due is off.
        if matches!(step.record.state, SyncState::Pending | SyncState::Failed { .. }) {
            step.record.state = SyncState::NotSynced;
            step.record.attempts = 0;
        }
        return Ok(step.done());
    };
    match plan::cleanup(&step.record) {
        Cleanup::Nothing => {}
        Cleanup::Delete => {
            if file_exists(&path).await {
                tracing::info!(job = id.0, path = %path.display(), "[RustView] The file exists; its record stays");
            } else {
                let found = step.take(client.get_media(&path).await)?;
                match found.media.filter(|_| found.found) {
                    None => tracing::debug!(job = id.0, "[RustView] The stub is gone already"),
                    Some(media) if media.source.as_deref() != Some(SOURCE_APP) => tracing::info!(
                        job = id.0,
                        source = ?media.source,
                        "[RustView] The record was taken over by another app; it stays"
                    ),
                    Some(_) => {
                        let deleted = step.take(client.delete_media(&path).await)?;
                        tracing::info!(job = id.0, deleted = deleted.deleted, "[RustView] Stub removed");
                    }
                }
            }
        }
        Cleanup::Restore { tags, rating } => {
            step.take(client.set_media(&path, rating, tags.as_deref(), None).await)?;
            tracing::info!(job = id.0, "[RustView] The record's tags and rating were given back");
        }
    }
    step.record = plan::cleaned(&step.record);
    Ok(step.done())
}
