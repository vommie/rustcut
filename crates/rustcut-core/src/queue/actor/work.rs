//! The renders of the queue: starting a job's render on the runtime, its reports, and what its end does to the job.

use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::{Actor, STOPPED_NOTE, control_channel};
use crate::job::{JobId, JobSpec, JobState, JobStateKind};
use crate::naming::same_path;
use crate::queue::{JobProgress, QueueError, QueueEvent};
use crate::render::{Control, JobLog, RenderEnv, RenderError, RenderUpdate, RenderedFile, Severity, render_job};
use crate::store::{LogStream, SpecSlot, StoreCommand};

/// Why the actor stopped a render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// The user canceled the job.
    Cancel,
    /// RustCut quits: the job waits again at the next start.
    Quit,
}

/// A running render.
pub(super) struct Worker {
    pub control: watch::Sender<Control>,
    pub task: JoinHandle<()>,
    pub stop: Option<Stop>,
}

/// What a render tells the actor.
#[derive(Debug)]
pub(super) enum Report {
    Pass {
        id: JobId,
        pass: u8,
        passes: u8,
    },
    Progress(JobProgress),
    Ended {
        id: JobId,
        result: Result<RenderedFile, RenderError>,
        log: Box<JobLog>,
    },
}

/// Refuses a job that can never render: no section, the source as the target, or a profile with an error.
pub fn validate(spec: &JobSpec) -> Result<(), QueueError> {
    if spec.sections.is_empty() {
        return Err(QueueError::NoSections);
    }
    if same_path(&spec.source, &spec.target_path()) {
        return Err(QueueError::SameSourceAndTarget);
    }
    if let Some(issue) = spec
        .profile
        .validate()
        .into_iter()
        .find(|i| i.severity == Severity::Error)
    {
        return Err(QueueError::InvalidProfile(issue.message));
    }
    Ok(())
}

impl Actor {
    /// Starts the render of a waiting job. A job that cannot render fails on its own and never holds up the queue.
    pub(super) async fn start_job(&mut self, id: JobId) {
        let Some(record) = self.model.get(id) else { return };
        let SpecSlot::Decoded(spec) = record.spec.clone() else {
            return;
        };
        let attempt = record.attempt;
        if !self
            .set_state(id, JobState::Rendering { pass: 0, passes: 0 }, None)
            .await
        {
            return;
        }
        let (control, receiver) = control_channel(self.model.is_paused());
        let task = tokio::spawn(work(
            id,
            attempt,
            spec,
            Arc::clone(&self.env),
            receiver,
            self.reports.clone(),
        ));
        self.workers.insert(
            id,
            Worker {
                control,
                task,
                stop: None,
            },
        );
        tracing::info!(job = id.0, attempt, "[Queue] Render started");
        self.publish_processing();
    }

    pub(super) async fn report(&mut self, report: Report) {
        match report {
            Report::Pass { id, pass, passes } => {
                let next = match self.model.get(id).map(|r| r.state.clone()) {
                    Some(JobState::Rendering { pass: now, .. }) if pass > now => JobState::Rendering { pass, passes },
                    Some(JobState::Paused { pass: now, .. }) if pass > now => JobState::Paused { pass, passes },
                    _ => return,
                };
                self.set_state(id, next, None).await;
            }
            Report::Progress(progress) => {
                if self.workers.contains_key(&progress.id) {
                    self.events.publish(QueueEvent::Progress(progress));
                }
            }
            Report::Ended { id, result, log } => self.ended(id, result, &log).await,
        }
    }

    /// A render ended: the job becomes finished, failed, canceled, or (stopped at quit) waiting again.
    async fn ended(&mut self, id: JobId, result: Result<RenderedFile, RenderError>, log: &JobLog) {
        let stop = self.workers.remove(&id).and_then(|w| w.stop);
        self.store_log(id, log);
        let (next, note) = match &result {
            Ok(_) => (JobState::Finished, None),
            Err(e) if e.is_canceled() && stop == Some(Stop::Quit) => (JobState::Waiting, Some(STOPPED_NOTE.to_owned())),
            Err(e) if e.is_canceled() => (JobState::Canceled, None),
            Err(e) => (JobState::Failed { reason: e.to_string() }, None),
        };
        // A render paused after its last pass (while verifying) still ends: it counts as rendering again first.
        if let Some(JobState::Paused { pass, passes }) = self.model.get(id).map(|r| r.state.clone())
            && matches!(next.kind(), JobStateKind::Finished | JobStateKind::Failed)
        {
            self.set_state(id, JobState::Rendering { pass, passes }, None).await;
        }
        self.model.count_end(&next);
        self.set_state(id, next, note).await;
        self.publish_processing();
        self.kick().await;
    }

    /// Writes the render's log to the store, one batch per run of lines of the same stream (fire and forget).
    fn store_log(&self, id: JobId, log: &JobLog) {
        let mut batch: Vec<String> = Vec::new();
        let mut stream = LogStream::App;
        let send = |stream: LogStream, lines: Vec<String>| {
            if !lines.is_empty() && self.store.send(StoreCommand::AppendLog { id, stream, lines }).is_err() {
                tracing::warn!(job = id.0, "[Queue] The log was not saved: the database has stopped");
            }
        };
        if log.dropped() > 0 {
            tracing::debug!(job = id.0, dropped = log.dropped(), "[Queue] Log lines left out");
        }
        for line in log.lines() {
            if line.stream != stream {
                send(stream, std::mem::take(&mut batch));
                stream = line.stream;
            }
            batch.push(line.line.clone());
        }
        send(stream, batch);
    }

    /// Asks the renders to pause or to run, and shows it in their rows.
    pub(super) async fn apply_pause(&mut self, paused: bool) {
        let ids: Vec<JobId> = self.workers.keys().copied().collect();
        for id in ids {
            if let Some(worker) = self.workers.get(&id)
                && worker.stop.is_none()
            {
                worker.control.send_replace(super::control_for(paused));
            }
            let next = match self.model.get(id).map(|r| r.state.clone()) {
                Some(JobState::Rendering { pass, passes }) if paused => JobState::Paused { pass, passes },
                Some(JobState::Paused { pass, passes }) if !paused => JobState::Rendering { pass, passes },
                _ => continue,
            };
            self.set_state(id, next, None).await;
        }
    }
}

/// One render on the runtime; its updates and its end go to the actor.
async fn work(
    id: JobId,
    attempt: u32,
    spec: Box<JobSpec>,
    env: Arc<RenderEnv>,
    control: watch::Receiver<Control>,
    reports: mpsc::UnboundedSender<Report>,
) {
    let mut log = JobLog::new();
    log.note(&format!("[Queue] Attempt {attempt}"));
    let updates = {
        let reports = reports.clone();
        move |update| {
            let report = match update {
                RenderUpdate::Pass { pass, passes, .. } => Report::Pass { id, pass, passes },
                RenderUpdate::Progress(progress) => Report::Progress(progress),
                RenderUpdate::Paused(_) | RenderUpdate::Verifying => return,
            };
            let _ = reports.send(report);
        }
    };
    let result = render_job(id, &spec, &env, control, &mut log, updates).await;
    let _ = reports.send(Report::Ended {
        id,
        result,
        log: Box::new(log),
    });
}
