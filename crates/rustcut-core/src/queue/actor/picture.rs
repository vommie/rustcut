//! The jobs' own pictures (P12 block 10, user decision 2026-11-02): one is made when a job is saved (and again when its
//! spec is replaced), one at a time and at a low priority, and stored with the job. A job without one (saved by an older
//! RustCut, or its source was missing then) gets it when the job card first asks; a picture that could not be made is
//! not tried again while RustCut runs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Semaphore;

use super::Actor;
use super::work::Report;
use crate::actor::Reply;
use crate::job::{JobId, JobSpec};
use crate::media::make_job_picture;
use crate::store::{JobPicture, SpecSlot, StoreCommand};

type Waiter = Reply<Option<JobPicture>>;

/// The pictures being made and who waits for them.
pub(super) struct Pictures {
    pending: HashMap<JobId, Vec<Waiter>>,
    failed: HashSet<JobId>,
    /// One ffmpeg at a time: saving many jobs quickly never starts many.
    turns: Arc<Semaphore>,
}

impl Default for Pictures {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            failed: HashSet::new(),
            turns: Arc::new(Semaphore::new(1)),
        }
    }
}

impl Actor {
    /// Makes the picture of job `id` from `spec` in the background (it reports with [`Report::Picture`]).
    pub(super) fn make_picture(&mut self, id: JobId, spec: &JobSpec) {
        self.pictures.failed.remove(&id);
        if self.pictures.pending.contains_key(&id) {
            return;
        }
        self.pictures.pending.insert(id, Vec::new());
        let (spec, ffmpeg, turns, reports) = (
            spec.clone(),
            self.env.ffmpeg.clone(),
            Arc::clone(&self.pictures.turns),
            self.reports.clone(),
        );
        tokio::spawn(async move {
            // The semaphore is never closed; without a turn the picture is made anyway.
            let _turn = turns.acquire_owned().await.ok();
            let picture = make_job_picture(&ffmpeg, &spec).await;
            let _ = reports.send(Report::Picture { id, picture });
        });
    }

    /// `QueueCommand::Picture`: the stored picture, the one being made, or a new one.
    pub(super) async fn picture(&mut self, id: JobId, reply: Waiter) {
        if let Some(waiters) = self.pictures.pending.get_mut(&id) {
            waiters.push(reply);
            return;
        }
        match self.store_request(|reply| StoreCommand::Picture { id, reply }).await {
            Ok(Some(picture)) => {
                let _ = reply.send(Some(picture));
                return;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(job = id.0, error = %e, "[Queue] The job picture could not be read");
                let _ = reply.send(None);
                return;
            }
        }
        let spec = match self.model.get(id).map(|r| &r.spec) {
            Some(SpecSlot::Decoded(spec)) if !self.pictures.failed.contains(&id) => spec.clone(),
            _ => {
                let _ = reply.send(None);
                return;
            }
        };
        self.make_picture(id, &spec);
        if let Some(waiters) = self.pictures.pending.get_mut(&id) {
            waiters.push(reply);
        }
    }

    /// A picture was made (or could not be): it is stored and handed to everyone who waits.
    pub(super) async fn picture_made(&mut self, id: JobId, picture: Option<JobPicture>) {
        let waiters = self.pictures.pending.remove(&id).unwrap_or_default();
        match &picture {
            Some(made) if self.model.get(id).is_some() => {
                let stored = made.clone();
                match self
                    .store_request(|reply| StoreCommand::SetPicture {
                        id,
                        picture: stored,
                        reply,
                    })
                    .await
                {
                    Ok(()) => tracing::debug!(job = id.0, bytes = made.webp.len(), "[Queue] Job picture stored"),
                    Err(e) => tracing::warn!(job = id.0, error = %e, "[Queue] The job picture could not be stored"),
                }
            }
            // The job went away meanwhile.
            Some(_) => {}
            None => {
                self.pictures.failed.insert(id);
            }
        }
        for waiter in waiters {
            let _ = waiter.send(picture.clone());
        }
    }
}
