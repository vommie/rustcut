//! The queue engine actor (ADR-4, ADR-15): it owns the jobs ([`Model`]), persists every change through the store actor,
//! runs one [`render_job`](crate::render::render_job) per started job with its own `watch` control channel, and
//! publishes [`QueueEvent`]s. Commands come through a [`QueueHandle`]; the renders report back on an internal channel,
//! so the actor never waits for a render.
//!
//! At start it puts jobs that a crash or a kill left rendering back to Waiting with a note (ADR-9, PyCutRv bug 10) and
//! deletes what their renders left behind. At shutdown it stops the running renders; they wait again at the next start.

mod commands;
mod sync;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;
mod work;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use self::work::{Report, Stop, Worker};
use super::model::Model;
use super::summary::{StaleFiles, summarize};
use super::{QueueCommand, QueueError, QueueEvent};
use crate::actor::{EventBus, Handle, Mailbox, Reply, mailbox};
use crate::job::{JobId, JobState};
use crate::render::RenderEnv;
use crate::rustview::{SyncHandle, SyncRecords};
use crate::settings::WhenDone;
use crate::store::{StoreCommand, StoreError, StoreHandle};

pub use self::sync::SyncLink;
pub use self::work::validate;

/// The note of a job that a crash or a kill left rendering.
pub const RECOVERED_NOTE: &str = "Interrupted when RustCut stopped; waiting again.";
/// The note of a job whose render was stopped because RustCut quit.
pub const STOPPED_NOTE: &str = "Stopped when RustCut quit; waiting again.";
/// How long the shutdown waits for the renders to stop (ffmpeg's cancel ladder takes at most 8 s).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(12);

pub type QueueHandle = Handle<QueueCommand>;

/// The running queue engine: its command handle and its event bus.
#[derive(Debug, Clone)]
pub struct QueueService {
    pub handle: QueueHandle,
    pub events: EventBus<QueueEvent>,
}

/// How the queue starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueOptions {
    pub workers: u8,
    /// Processing was paused when RustCut stopped (`ui-state.toml`).
    pub paused: bool,
    /// Start paused when jobs are waiting (setting `queue.pause_on_start`).
    pub pause_on_start: bool,
    pub when_done: WhenDone,
}

/// Starts the queue engine on `runtime`. It loads the jobs from `store` (after the crash recovery) before it answers
/// the first command.
pub fn spawn_queue(
    runtime: &tokio::runtime::Handle,
    store: StoreHandle,
    env: RenderEnv,
    options: QueueOptions,
) -> (QueueService, JoinHandle<()>) {
    spawn_queue_linked(runtime, store, env, options, None)
}

/// [`spawn_queue`] with the RustView sync actor (ADR-6): the queue tells it what each job wants and stores the records
/// it reports with the jobs.
pub fn spawn_queue_linked(
    runtime: &tokio::runtime::Handle,
    store: StoreHandle,
    env: RenderEnv,
    options: QueueOptions,
    link: Option<SyncLink>,
) -> (QueueService, JoinHandle<()>) {
    let (handle, commands) = mailbox();
    let events = EventBus::new();
    let task = runtime.spawn(run(commands, store, env, options, events.clone(), link));
    (QueueService { handle, events }, task)
}

async fn run(
    mut commands: Mailbox<QueueCommand>,
    store: StoreHandle,
    env: RenderEnv,
    options: QueueOptions,
    events: EventBus<QueueEvent>,
    link: Option<SyncLink>,
) {
    let (reports_tx, mut reports) = mpsc::unbounded_channel();
    let (sync, mut records) = link.map_or((None, None), |l| (Some(l.handle), Some(l.records)));
    let mut actor = Actor::start(store, env, options, events, reports_tx, sync).await;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(QueueCommand::Shutdown { reply }) => {
                    actor.shutdown(&mut reports, &mut records).await;
                    let _ = reply.send(());
                    return;
                }
                Some(command) => actor.handle(command).await,
                None => {
                    actor.shutdown(&mut reports, &mut records).await;
                    return;
                }
            },
            Some(report) = reports.recv() => actor.report(report).await,
            record = sync::next_record(&mut records) => match record {
                Some((id, record)) => actor.sync_reported(id, record).await,
                None => records = None,
            },
        }
    }
}

struct Actor {
    model: Model,
    store: StoreHandle,
    events: EventBus<QueueEvent>,
    env: Arc<RenderEnv>,
    workers: HashMap<JobId, Worker>,
    reports: mpsc::UnboundedSender<Report>,
    /// Shutting down: nothing starts any more.
    stopping: bool,
    /// The RustView sync actor, when linked.
    sync: Option<SyncHandle>,
}

impl Actor {
    async fn start(
        store: StoreHandle,
        env: RenderEnv,
        options: QueueOptions,
        events: EventBus<QueueEvent>,
        reports: mpsc::UnboundedSender<Report>,
        sync: Option<SyncHandle>,
    ) -> Self {
        let mut actor = Self {
            model: Model::new(Vec::new(), options.workers, options.paused, options.when_done),
            store,
            events,
            env: Arc::new(env),
            workers: HashMap::new(),
            reports,
            stopping: false,
            sync,
        };
        let (jobs, recovered) = actor.read_jobs().await;
        let waiting = jobs.iter().any(|j| j.state == JobState::Waiting);
        let paused = options.paused || (options.pause_on_start && waiting);
        actor.model = Model::new(jobs, options.workers, paused, options.when_done);
        actor.clean_recovered(&recovered).await;
        tracing::info!(
            jobs = actor.model.jobs().len(),
            recovered = recovered.len(),
            paused,
            workers = options.workers,
            "[Queue] Started"
        );
        actor.sync_resume();
        actor.kick().await;
        actor
    }

    /// The jobs of the store after the crash recovery, and the recovered ones.
    async fn read_jobs(&self) -> (Vec<crate::store::JobRecord>, Vec<JobId>) {
        let recovered = self
            .store_request(|reply| StoreCommand::RecoverInterrupted {
                note: RECOVERED_NOTE.into(),
                reply,
            })
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "[Queue] Interrupted jobs could not be recovered");
                Vec::new()
            });
        let jobs = self
            .store_request(|reply| StoreCommand::Jobs { reply })
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "[Queue] The jobs could not be read; the queue starts empty");
                Vec::new()
            });
        (jobs, recovered)
    }

    async fn clean_recovered(&self, recovered: &[JobId]) {
        for id in recovered {
            if let Some(record) = self.model.get(*id) {
                StaleFiles::of(record, &self.env.dirs).remove().await;
            }
        }
    }

    /// `QueueCommand::Reload`: the store was replaced; its jobs take the place of the ones shown (nothing renders).
    pub(super) async fn reload(&mut self) -> Result<usize, QueueError> {
        if !self.workers.is_empty() {
            return Err(QueueError::Store("a job is rendering".into()));
        }
        let (jobs, recovered) = self.read_jobs().await;
        let state = self.model.processing();
        let old: Vec<JobId> = self.model.jobs().iter().map(|j| j.id).collect();
        self.model = Model::new(jobs, state.workers, state.paused, state.when_done);
        self.clean_recovered(&recovered).await;
        if !old.is_empty() {
            self.events.publish(QueueEvent::Removed(old));
        }
        for record in self.model.jobs() {
            self.events.publish(QueueEvent::Added(Box::new(summarize(record))));
        }
        self.events
            .publish(QueueEvent::Reordered(self.model.jobs().iter().map(|j| j.id).collect()));
        self.publish_processing();
        tracing::info!(jobs = self.model.jobs().len(), "[Queue] Reloaded from the store");
        self.sync_resume();
        self.kick().await;
        Ok(self.model.jobs().len())
    }

    /// A store request whose failure becomes [`QueueError::Store`].
    async fn store_request<T>(
        &self,
        make: impl FnOnce(Reply<Result<T, StoreError>>) -> StoreCommand,
    ) -> Result<T, QueueError> {
        match self.store.request(make).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(QueueError::Store(e.to_string())),
            Err(_gone) => Err(QueueError::Store("the database has stopped".into())),
        }
    }

    /// Moves a job to `next` (checked by the state machine), stores it and publishes the row. `false` when refused.
    async fn set_state(&mut self, id: JobId, next: JobState, note: Option<String>) -> bool {
        let Some(from) = self.model.get(id).map(|r| r.state.kind()) else {
            return false;
        };
        if let Err(e) = self.model.transition(id, next.clone(), note.clone()) {
            tracing::warn!(error = %e, "[Queue] State change refused");
            return false;
        }
        let stored = self
            .store_request(|reply| StoreCommand::SetState {
                id,
                state: next.clone(),
                note,
                reply,
            })
            .await;
        if let Err(e) = stored {
            tracing::warn!(job = id.0, error = %e, "[Queue] State not saved");
        }
        self.refresh(id).await;
        self.sync_state_changed(id, from, &next);
        true
    }

    /// Reads the row again (the store stamps times and counts attempts) and publishes it. The state and the note stay
    /// the model's, also when the store could not save them.
    async fn refresh(&mut self, id: JobId) {
        if let Ok(Some(mut record)) = self.store_request(|reply| StoreCommand::Job { id, reply }).await
            && let Some(current) = self.model.get(id)
        {
            record.state = current.state.clone();
            record.note.clone_from(&current.note);
            self.model.replace(record);
        }
        self.publish_row(id);
    }

    fn publish_row(&self, id: JobId) {
        if let Some(record) = self.model.get(id) {
            self.events.publish(QueueEvent::Updated(Box::new(summarize(record))));
        }
    }

    fn publish_processing(&self) {
        self.events.publish(QueueEvent::Processing(self.model.processing()));
    }

    /// After every change: a pending "pause after current" takes effect, free workers get the next jobs, and an idle
    /// queue reports what it did.
    async fn kick(&mut self) {
        if self.stopping {
            return;
        }
        if self.model.settle_pause() {
            tracing::info!("[Queue] The running jobs are done; processing paused");
            self.publish_processing();
        }
        loop {
            let picks = self.model.schedule();
            if picks.is_empty() {
                break;
            }
            for id in picks {
                self.start_job(id).await;
            }
        }
        if let Some(tally) = self.model.take_done() {
            tracing::info!(
                finished = tally.finished,
                failed = tally.failed,
                "[Queue] All waiting jobs are done"
            );
            self.events.publish(QueueEvent::Done {
                finished: tally.finished,
                failed: tally.failed,
            });
        }
    }

    /// Stops every render; the jobs wait again at the next start. Records the sync actor reported before are stored.
    async fn shutdown(&mut self, reports: &mut mpsc::UnboundedReceiver<Report>, records: &mut Option<SyncRecords>) {
        self.stopping = true;
        if let Some(records) = records {
            while let Ok((id, record)) = records.try_recv() {
                self.sync_reported(id, record).await;
            }
        }
        if self.workers.is_empty() {
            tracing::info!("[Queue] Stopped");
            return;
        }
        tracing::info!(running = self.workers.len(), "[Queue] Stopping the running renders");
        for worker in self.workers.values_mut() {
            worker.stop = Some(Stop::Quit);
            worker.control.send_replace(crate::render::Control::Cancel);
        }
        let deadline = tokio::time::Instant::now() + SHUTDOWN_WAIT;
        while !self.workers.is_empty() {
            match tokio::time::timeout_at(deadline, reports.recv()).await {
                Ok(Some(report)) => self.report(report).await,
                Ok(None) | Err(_) => break,
            }
        }
        let left: Vec<JobId> = self.workers.keys().copied().collect();
        for id in left {
            if let Some(worker) = self.workers.remove(&id) {
                tracing::warn!(job = id.0, "[Queue] The render did not stop in time; it was killed");
                // Dropping the render kills ffmpeg (`kill_on_drop`) and deletes the temp file (`Leftovers`).
                worker.task.abort();
                self.set_state(id, JobState::Waiting, Some(STOPPED_NOTE.into())).await;
            }
        }
        tracing::info!("[Queue] Stopped");
    }
}

fn summary_rows(jobs: &[crate::store::JobRecord]) -> Vec<super::JobSummary> {
    jobs.iter().map(summarize).collect()
}

/// The control value a new or resumed render gets.
const fn control_for(paused: bool) -> crate::render::Control {
    if paused {
        crate::render::Control::Pause
    } else {
        crate::render::Control::Run
    }
}

/// A fresh control channel for a render.
fn control_channel(
    paused: bool,
) -> (
    watch::Sender<crate::render::Control>,
    watch::Receiver<crate::render::Control>,
) {
    watch::channel(control_for(paused))
}
