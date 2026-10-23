//! The queue's commands: each one changes the model, stores the change and publishes it.

use std::sync::Arc;

use super::work::{Stop, validate};
use super::{Actor, StaleFiles, summarize, summary_rows};
use crate::job::{JobId, JobMetadata, JobSpec, JobState, JobStateKind};
use crate::queue::{QueueCommand, QueueError, QueueEvent, QueueSnapshot};
use crate::render::{Control, RenderProfile};
use crate::rustview::SyncPhase;
use crate::store::{SpecSlot, StoreCommand};

impl Actor {
    pub(super) async fn handle(&mut self, command: QueueCommand) {
        match command {
            QueueCommand::Enqueue { spec, hold, reply } => {
                let _ = reply.send(self.enqueue(spec, hold).await);
            }
            QueueCommand::UpdateSpec { id, spec, reply } => {
                let _ = reply.send(self.update_spec(id, spec).await);
            }
            QueueCommand::Move { ids, to } => {
                if let Some(order) = self.model.reorder(&ids, to) {
                    let ids = order.clone();
                    if let Err(e) = self.store_request(|reply| StoreCommand::SetOrder { ids, reply }).await {
                        tracing::warn!(error = %e, "[Queue] Order not saved");
                    }
                    self.events.publish(QueueEvent::Reordered(order));
                    self.kick().await;
                }
            }
            QueueCommand::Hold { ids } => self.switch(&ids, JobStateKind::Waiting, JobState::Held).await,
            QueueCommand::Release { ids } => self.switch(&ids, JobStateKind::Held, JobState::Waiting).await,
            QueueCommand::Restart { ids, profile } => self.restart(&ids, profile).await,
            QueueCommand::Duplicate { ids, reply } => {
                let _ = reply.send(self.duplicate(&ids).await);
            }
            QueueCommand::Remove { ids, reply } => {
                let _ = reply.send(self.remove(&ids).await);
            }
            QueueCommand::RemoveFinished { reply } => {
                let ids: Vec<JobId> = self
                    .model
                    .jobs()
                    .iter()
                    .filter(|j| j.state == JobState::Finished)
                    .map(|j| j.id)
                    .collect();
                let _ = reply.send(self.remove(&ids).await);
            }
            QueueCommand::Cancel { ids } => self.cancel(&ids).await,
            QueueCommand::SetPaused(paused) => {
                self.model.set_paused(paused);
                tracing::info!(paused, "[Queue] Processing switched");
                self.apply_pause(paused).await;
                self.publish_processing();
                self.kick().await;
            }
            QueueCommand::SetPauseAfterCurrent(on) => {
                self.model.set_pause_after_current(on);
                self.publish_processing();
                self.kick().await;
            }
            QueueCommand::SetWorkers(workers) => {
                self.model.set_workers(workers);
                self.publish_processing();
                self.kick().await;
            }
            QueueCommand::SetWhenDone(when_done) => {
                self.model.set_when_done(when_done);
                self.publish_processing();
            }
            QueueCommand::Snapshot { reply } => {
                let _ = reply.send(QueueSnapshot {
                    jobs: summary_rows(self.model.jobs()),
                    processing: self.model.processing(),
                });
            }
            QueueCommand::Log { id, limit, reply } => {
                let answer = if self.model.get(id).is_some() {
                    self.store_request(|reply| StoreCommand::Log { id, limit, reply }).await
                } else {
                    Err(QueueError::NoSuchJob(id))
                };
                let _ = reply.send(answer);
            }
            QueueCommand::SetRenderEnv(env) => {
                tracing::debug!("[Queue] Render environment updated");
                self.env = Arc::new(*env);
            }
            QueueCommand::Metadata { id, reply } => {
                let answer = match self.model.get(id).map(|r| &r.spec) {
                    Some(SpecSlot::Decoded(spec)) => Ok(spec.metadata.clone()),
                    Some(SpecSlot::Unreadable { .. }) => Err(QueueError::Unreadable(id)),
                    None => Err(QueueError::NoSuchJob(id)),
                };
                let _ = reply.send(answer);
            }
            QueueCommand::UpdateMetadata { id, metadata, reply } => {
                let _ = reply.send(self.update_metadata(id, metadata).await);
            }
            QueueCommand::Spec { id, reply } => {
                let answer = match self.model.get(id).map(|r| &r.spec) {
                    Some(SpecSlot::Decoded(spec)) => Ok(spec.clone()),
                    Some(SpecSlot::Unreadable { .. }) => Err(QueueError::Unreadable(id)),
                    None => Err(QueueError::NoSuchJob(id)),
                };
                let _ = reply.send(answer);
            }
            // Handled by the loop before it gets here.
            QueueCommand::Shutdown { reply } => {
                let _ = reply.send(());
            }
        }
    }

    async fn enqueue(&mut self, spec: Box<JobSpec>, hold: bool) -> Result<JobId, QueueError> {
        validate(&spec)?;
        let state = if hold { JobState::Held } else { JobState::Waiting };
        let record = self
            .store_request(|reply| StoreCommand::InsertJob { spec, state, reply })
            .await?;
        let id = record.id;
        tracing::info!(job = id.0, target = %record.target_path.display(), hold, "[Queue] Job added");
        self.events.publish(QueueEvent::Added(Box::new(summarize(&record))));
        self.model.push(record);
        self.sync_job(id, SyncPhase::Enqueued);
        self.kick().await;
        Ok(id)
    }

    async fn update_spec(&mut self, id: JobId, spec: Box<JobSpec>) -> Result<(), QueueError> {
        let record = self.model.get(id).ok_or(QueueError::NoSuchJob(id))?;
        let kind = record.state.kind();
        if !matches!(kind, JobStateKind::Waiting | JobStateKind::Held) {
            return Err(QueueError::NotEditable { id, state: kind });
        }
        validate(&spec)?;
        self.store_spec(id, spec).await?;
        self.sync_job(id, SyncPhase::Edited);
        self.kick().await;
        Ok(())
    }

    /// New tags, rating or source hash for a job in any state; see [`QueueCommand::UpdateMetadata`].
    async fn update_metadata(&mut self, id: JobId, metadata: JobMetadata) -> Result<(), QueueError> {
        let record = self.model.get(id).ok_or(QueueError::NoSuchJob(id))?;
        let SpecSlot::Decoded(spec) = &record.spec else {
            return Err(QueueError::Unreadable(id));
        };
        if spec.metadata == metadata {
            return Ok(());
        }
        let kind = record.state.kind();
        let mut spec = spec.clone();
        spec.metadata = metadata;
        self.store_spec(id, spec).await?;
        tracing::info!(job = id.0, state = %kind, "[Queue] RustView metadata of a job changed");
        match kind {
            JobStateKind::Finished => self.sync_job(id, SyncPhase::Finished),
            JobStateKind::Failed | JobStateKind::Canceled => {}
            _ => self.sync_job(id, SyncPhase::Edited),
        }
        Ok(())
    }

    /// Stores a new spec and takes the row back from the store.
    async fn store_spec(&mut self, id: JobId, spec: Box<JobSpec>) -> Result<(), QueueError> {
        let stored = spec.clone();
        self.store_request(|reply| StoreCommand::UpdateSpec { id, spec, reply })
            .await?;
        if let Some(mut record) = self.model.get(id).cloned() {
            record.target_path = stored.target_path();
            record.source_path.clone_from(&stored.source);
            record.spec = SpecSlot::Decoded(stored);
            self.model.replace(record);
        }
        self.refresh(id).await;
        Ok(())
    }

    /// Hold (Waiting -> Held) or release (Held -> Waiting) the jobs that are in `from`.
    async fn switch(&mut self, ids: &[JobId], from: JobStateKind, to: JobState) {
        for &id in ids {
            if self.model.get(id).is_some_and(|r| r.state.kind() == from) {
                self.set_state(id, to.clone(), None).await;
            }
        }
        self.kick().await;
    }

    /// Done jobs wait again as a new attempt, optionally with another profile.
    async fn restart(&mut self, ids: &[JobId], profile: Option<Box<RenderProfile>>) {
        for &id in ids {
            let Some(record) = self.model.get(id) else { continue };
            if !record.state.kind().is_done() {
                continue;
            }
            if let (Some(profile), SpecSlot::Decoded(spec)) = (&profile, &record.spec) {
                let mut spec = spec.clone();
                spec.profile = (**profile).clone();
                if let Err(e) = self.store_spec(id, spec).await {
                    tracing::warn!(job = id.0, error = %e, "[Queue] New profile not saved; restarting with the old one");
                }
            }
            self.set_state(id, JobState::Waiting, None).await;
        }
        self.kick().await;
    }

    /// Copies of the jobs, each right after its original.
    async fn duplicate(&mut self, ids: &[JobId]) -> Result<Vec<JobId>, QueueError> {
        let originals: Vec<(JobId, Box<JobSpec>)> = self
            .model
            .jobs()
            .iter()
            .filter(|j| ids.contains(&j.id))
            .filter_map(|j| match &j.spec {
                SpecSlot::Decoded(spec) => Some((j.id, spec.clone())),
                SpecSlot::Unreadable { .. } => None,
            })
            .collect();
        let mut copies = Vec::new();
        for (original, spec) in originals {
            let record = self
                .store_request(|reply| StoreCommand::InsertJob {
                    spec,
                    state: JobState::Waiting,
                    reply,
                })
                .await?;
            let id = record.id;
            copies.push(id);
            self.events.publish(QueueEvent::Added(Box::new(summarize(&record))));
            self.model.insert_after(original, record);
            self.sync_job(id, SyncPhase::Enqueued);
        }
        if !copies.is_empty() {
            let order = self.model.order();
            let ids = order.clone();
            self.store_request(|reply| StoreCommand::SetOrder { ids, reply })
                .await?;
            self.events.publish(QueueEvent::Reordered(order));
            self.kick().await;
        }
        Ok(copies)
    }

    /// Deletes jobs that do not render, and what their renders may have left behind (never the target, never a
    /// file next to the source).
    async fn remove(&mut self, ids: &[JobId]) -> Result<usize, QueueError> {
        let stale: Vec<StaleFiles> = self
            .model
            .jobs()
            .iter()
            .filter(|j| ids.contains(&j.id) && !j.state.kind().is_active())
            .map(|j| StaleFiles::of(j, &self.env.dirs))
            .collect();
        // A finished job's record describes its file and stays; any other job's record is cleaned up.
        let unfinished: Vec<_> = self
            .model
            .jobs()
            .iter()
            .filter(|j| ids.contains(&j.id) && !j.state.kind().is_active() && j.state != JobState::Finished)
            .cloned()
            .collect();
        let removed = self.model.remove(ids);
        if removed.is_empty() {
            return Ok(0);
        }
        let delete = removed.clone();
        let deleted = self
            .store_request(|reply| StoreCommand::DeleteJobs { ids: delete, reply })
            .await;
        for mut files in stale {
            if files.temp.as_deref().is_some_and(|t| self.temp_busy(t)) {
                files.temp = None;
            }
            files.remove().await;
        }
        for job in unfinished.iter().filter(|j| removed.contains(&j.id)) {
            self.sync_cleanup(job);
        }
        tracing::info!(jobs = removed.len(), "[Queue] Jobs removed");
        self.events.publish(QueueEvent::Removed(removed.clone()));
        self.kick().await;
        deleted.map(|_| removed.len())
    }

    /// Whether `temp` is the temp file of a running render (a job with the same target writes it right now).
    fn temp_busy(&self, temp: &std::path::Path) -> bool {
        self.workers
            .keys()
            .filter_map(|id| self.model.get(*id))
            .filter_map(|r| StaleFiles::of(r, &self.env.dirs).temp)
            .any(|t| crate::naming::same_path(&t, temp))
    }

    /// Stops renders (the temp file goes, an existing target stays) and cancels waiting or held jobs.
    async fn cancel(&mut self, ids: &[JobId]) {
        for &id in ids {
            if let Some(worker) = self.workers.get_mut(&id) {
                worker.stop = Some(Stop::Cancel);
                worker.control.send_replace(Control::Cancel);
                tracing::info!(job = id.0, "[Queue] Render canceled");
                continue;
            }
            if self
                .model
                .get(id)
                .is_some_and(|r| matches!(r.state.kind(), JobStateKind::Waiting | JobStateKind::Held))
            {
                self.set_state(id, JobState::Canceled, None).await;
            }
        }
        self.kick().await;
    }
}
