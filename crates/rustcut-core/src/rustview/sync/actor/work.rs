//! The actor's work: which call comes next, and what its answer changes (records, connection state, back-off).

use std::sync::Arc;

use tokio::time::Instant;

use super::{Actor, Answer, Backoff, Goal, LONGEST_RETRY, Request, Work, delays};
use crate::job::JobId;
use crate::naming::same_path;
use crate::rustview::sync::plan;
use crate::rustview::sync::steps::{self, Outcome, StepResult, WriteWork};
use crate::rustview::sync::{ConnectionState, JobSyncRecord, SyncEvent, SyncState};
use crate::rustview::{RustViewClient, RustViewError};
use crate::settings::RustViewSettings;

impl<C, F> Actor<C, F>
where
    C: RustViewClient,
    F: Fn(&RustViewSettings) -> C + Send + 'static,
{
    /// The next call to make: lookups, the tag tree, hashes, then the jobs in id order. `None` while switched off or
    /// pausing after a failure.
    pub(super) fn next_work(&mut self) -> Option<Work> {
        if !self.settings.enabled || self.backoff.as_ref().is_some_and(|b| b.retry_at > Instant::now()) {
            return None;
        }
        let client = Arc::clone(&self.client);
        if let Some(request) = self.requests.pop_front() {
            return Some(match request {
                Request::GetMedia { path, reply } => Box::pin(async move {
                    let answer = client.get_media(&path).await;
                    let error = answer.as_ref().err().cloned();
                    let _ = reply.send(answer);
                    Answer::Lookup(error)
                }),
                Request::CheckHash { md5, reply } => Box::pin(async move {
                    let answer = client.check_hash(&md5).await;
                    let error = answer.as_ref().err().cloned();
                    let _ = reply.send(answer);
                    Answer::Lookup(error)
                }),
            });
        }
        if self.want_tree {
            return Some(Box::pin(async move { Answer::Tree(client.tag_tree().await) }));
        }
        if let Some(md5) = self.hashes.pop_front() {
            return Some(Box::pin(async move {
                let result = client.insert_hash(&md5).await.map(|_| ());
                Answer::Hash { md5, result }
            }));
        }
        self.next_job(&client)
    }

    fn next_job(&mut self, client: &Arc<C>) -> Option<Work> {
        loop {
            let (id, entry) = self.entries.iter().find(|(_, e)| e.goal != Goal::Idle)?;
            let id = *id;
            let generation = entry.generation;
            let timing = self.settings.write_timing;
            let (record, moved) = match (entry.goal, &entry.target) {
                (Goal::Clean, _) => (entry.record.clone(), false),
                (Goal::Write(phase), Some(target)) => match &entry.record.synced_path {
                    // Written for another target before: that record is cleaned up first.
                    Some(old) if !same_path(old, target) => (entry.record.clone(), true),
                    _ => {
                        let work = WriteWork {
                            id,
                            target: target.clone(),
                            metadata: entry.metadata.clone(),
                            phase,
                            record: entry.record.clone(),
                            timing,
                        };
                        let client = Arc::clone(client);
                        return Some(Box::pin(async move {
                            let result = steps::write(&*client, work).await;
                            Answer::Job {
                                id,
                                generation,
                                moved: false,
                                result,
                            }
                        }));
                    }
                },
                (Goal::Write(_), None) | (Goal::Idle, _) => {
                    self.settle(id, generation);
                    continue;
                }
            };
            if self.hand_over(id, &record) {
                if !moved {
                    self.settle(id, generation);
                }
                continue;
            }
            let client = Arc::clone(client);
            return Some(Box::pin(async move {
                let result = steps::clean(&*client, id, record).await;
                Answer::Job {
                    id,
                    generation,
                    moved,
                    result,
                }
            }));
        }
    }

    /// Another job still uses the record `record` was written for (two jobs with one target): instead of deleting it,
    /// that job takes it over (as RustCut's own record when this job created it). `true` = handed over, this job's
    /// record is cleaned without a call.
    fn hand_over(&mut self, id: JobId, record: &JobSyncRecord) -> bool {
        let Some(path) = record.synced_path.clone() else {
            return false;
        };
        let heir = self
            .entries
            .iter()
            .find(|(other, e)| {
                **other != id
                    && e.goal != Goal::Clean
                    && e.record.state != SyncState::CleanedUp
                    && e.record.synced_path.as_deref().is_some_and(|p| same_path(p, &path))
            })
            .map(|(other, _)| *other);
        let Some(heir) = heir else {
            return false;
        };
        tracing::info!(
            job = id.0,
            heir = heir.0,
            "[RustView] Another job uses the record; it stays"
        );
        if record.preexisting == Some(false)
            && let Some(entry) = self.entries.get_mut(&heir)
        {
            entry.record.preexisting = Some(false);
            entry.record.previous_tags = None;
            entry.record.previous_rating = None;
            let heir_record = entry.record.clone();
            self.report(heir, heir_record);
        }
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.record = plan::cleaned(record);
            let cleaned = entry.record.clone();
            self.report(id, cleaned);
        }
        true
    }

    /// Nothing more to do for the job (unless a newer command arrived meanwhile).
    fn settle(&mut self, id: JobId, generation: u64) {
        if let Some(entry) = self.entries.get_mut(&id)
            && entry.generation == generation
        {
            entry.goal = Goal::Idle;
        }
    }

    pub(super) fn finish(&mut self, answer: Answer) {
        match answer {
            Answer::Tree(Ok(tree)) => {
                self.reached();
                self.want_tree = false;
                let tree = Arc::new(tree);
                tracing::info!(tags = tree.tags.len(), "[RustView] Tag tree loaded");
                self.tree = Some(Arc::clone(&tree));
                for waiter in self.tree_waiters.drain(..) {
                    let _ = waiter.send(Ok(Arc::clone(&tree)));
                }
                self.events.publish(SyncEvent::TagTreeChanged(tree));
            }
            Answer::Tree(Err(error)) => {
                for waiter in self.tree_waiters.drain(..) {
                    let _ = waiter.send(Err(error.clone()));
                }
                if error.is_transient() {
                    self.unreachable(&error);
                } else {
                    tracing::warn!(error = %error, "[RustView] The tag tree could not be read");
                    self.want_tree = false;
                    self.reached();
                }
            }
            Answer::Lookup(None) => self.reached(),
            Answer::Lookup(Some(error)) => {
                if error.is_transient() {
                    self.unreachable(&error);
                } else {
                    self.reached();
                }
            }
            Answer::Hash { md5, result } => match result {
                Ok(()) => {
                    tracing::info!("[RustView] Source hash registered");
                    self.registered.insert(md5);
                    self.reached();
                }
                Err(error) if error.is_transient() => {
                    self.hashes.push_front(md5);
                    self.unreachable(&error);
                }
                Err(error) => {
                    tracing::warn!(error = %error, "[RustView] The source hash was refused");
                    self.reached();
                }
            },
            Answer::Job {
                id,
                generation,
                moved,
                result,
            } => self.job_done(id, generation, moved, result),
        }
    }

    fn job_done(&mut self, id: JobId, generation: u64, moved: bool, result: StepResult) {
        if let Some(entry) = self.entries.get_mut(&id) {
            let changed = entry.record != result.record;
            entry.record = result.record.clone();
            if changed {
                self.report(id, result.record.clone());
            }
        }
        match &result.outcome {
            Outcome::Done => {
                // A moved job writes at its new target next; everything else is done.
                if !moved {
                    self.settle(id, generation);
                }
            }
            Outcome::Retry(error) => self.unreachable(error),
            Outcome::Failed(error) => {
                tracing::warn!(job = id.0, error = %error, "[RustView] The job's record could not be written");
                self.settle(id, generation);
            }
        }
        if result.answered && !matches!(result.outcome, Outcome::Retry(_)) {
            self.reached();
        }
    }

    pub(super) fn report(&self, id: JobId, record: JobSyncRecord) {
        let _ = self.records.send((id, record.clone()));
        self.events.publish(SyncEvent::JobSynced { id, record });
    }

    pub(super) fn set_connection(&mut self, state: ConnectionState) {
        if self.connection != state {
            self.connection = state.clone();
            self.events.publish(SyncEvent::Connection(state));
        }
    }

    /// RustView answered.
    fn reached(&mut self) {
        if matches!(self.connection, ConnectionState::Unavailable { .. }) {
            tracing::info!("[RustView] Reachable again");
            // Tags may have changed meanwhile.
            self.want_tree = true;
        }
        self.backoff = None;
        self.set_connection(ConnectionState::Connected);
    }

    /// RustView could not be reached: pause before the next call, refuse the waiting lookups.
    fn unreachable(&mut self, error: &RustViewError) {
        let backoff = self.backoff.get_or_insert_with(|| Backoff {
            delays: delays(),
            retry_at: Instant::now(),
        });
        let delay = backoff.delays.next().unwrap_or(LONGEST_RETRY);
        backoff.retry_at = Instant::now() + delay;
        self.want_tree = true;
        for request in self.requests.drain(..) {
            request.refuse(error.clone());
        }
        for waiter in self.tree_waiters.drain(..) {
            let _ = waiter.send(Err(error.clone()));
        }
        tracing::warn!(error = %error, retry_in = delay.as_secs(), "[RustView] Not reachable");
        self.set_connection(ConnectionState::Unavailable {
            reason: error.to_string(),
            retry_in_secs: u32::try_from(delay.as_secs()).unwrap_or(u32::MAX),
        });
    }
}
