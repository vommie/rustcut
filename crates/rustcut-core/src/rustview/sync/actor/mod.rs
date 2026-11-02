//! The sync actor (ADR-4, ADR-6): one call to RustView at a time, never on the UI thread. It keeps what every job wants
//! (an [`Entry`] per job), works through the jobs in id order after the app's lookups, reports every changed
//! [`JobSyncRecord`] to the queue engine, and while RustView cannot be reached it waits with a growing pause
//! (2 s doubling to 2 min) and shows that in its [`ConnectionState`]. A lookup during that pause fails at once instead
//! of waiting. Commands are taken while a call runs; `Shutdown` drops the call, which kills its `rustview` child.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use backon::{BackoffBuilder as _, ExponentialBackoff, ExponentialBuilder};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::steps::StepResult;
use super::{ConnectionState, JobSyncRecord, SyncCommand, SyncEvent, SyncPhase, SyncRecords, SyncService, SyncState};
use crate::actor::{EventBus, Mailbox, Reply, mailbox};
use crate::job::{JobId, JobMetadata};
use crate::rustview::{GetMedia, HashCheck, MoveMedia, RustViewClient, RustViewError, TagTree, is_md5};
use crate::settings::{RustViewSettings, WriteTiming};

mod work;

const FIRST_RETRY: Duration = Duration::from_secs(2);
const LONGEST_RETRY: Duration = Duration::from_secs(120);
const SWITCHED_OFF: &str = "RustView is switched off in the settings";

/// Starts the sync actor on `runtime`. `make_client` builds the client from the settings (again after `Reconfigure`).
/// The returned [`SyncRecords`] go to the queue engine, which stores each record with its job.
pub fn spawn_sync<C, F>(
    runtime: &tokio::runtime::Handle,
    settings: RustViewSettings,
    make_client: F,
) -> (SyncService, SyncRecords, JoinHandle<()>)
where
    C: RustViewClient,
    F: Fn(&RustViewSettings) -> C + Send + 'static,
{
    let (handle, commands) = mailbox();
    let events = EventBus::new();
    let (records_tx, records) = mpsc::unbounded_channel();
    let actor = Actor::new(settings, make_client, events.clone(), records_tx);
    let task = runtime.spawn(run(commands, actor));
    (SyncService { handle, events }, records, task)
}

/// What the actor wants for one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Goal {
    Idle,
    Write(SyncPhase),
    Clean,
}

#[derive(Debug)]
struct Entry {
    target: Option<PathBuf>,
    metadata: JobMetadata,
    goal: Goal,
    record: JobSyncRecord,
    /// Raised by every command for the job, so a step that ran meanwhile does not end newer work.
    generation: u64,
}

/// A lookup of the app, answered in order before the jobs' work.
#[derive(Debug)]
enum Request {
    GetMedia {
        path: PathBuf,
        reply: Reply<Result<GetMedia, RustViewError>>,
    },
    CheckHash {
        md5: String,
        reply: Reply<Result<HashCheck, RustViewError>>,
    },
    MoveRecord {
        id: JobId,
        from: PathBuf,
        to: PathBuf,
        replace: bool,
        reply: Reply<Result<MoveMedia, RustViewError>>,
    },
}

impl Request {
    fn refuse(self, error: RustViewError) {
        match self {
            Self::GetMedia { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::CheckHash { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::MoveRecord { reply, .. } => {
                // The caller (`lookup`) marks the record as not known to be RustCut's.
                let _ = reply.send(Err(error));
            }
        }
    }
}

/// The result of one piece of work (the future returns it to the loop).
enum Answer {
    Tree(Result<TagTree, RustViewError>),
    /// A lookup was answered (the reply went out inside the future); the error tells about the connection.
    Lookup(Option<RustViewError>),
    /// A record that followed its file was answered; on an error the record at the new path is not known to be RustCut's.
    Moved {
        id: JobId,
        error: Option<RustViewError>,
    },
    Hash {
        md5: String,
        result: Result<(), RustViewError>,
    },
    Job {
        id: JobId,
        generation: u64,
        moved: bool,
        result: StepResult,
    },
}

type Work = Pin<Box<dyn Future<Output = Answer> + Send>>;

/// The pause while RustView cannot be reached.
struct Backoff {
    delays: ExponentialBackoff,
    retry_at: Instant,
}

fn delays() -> ExponentialBackoff {
    ExponentialBuilder::new()
        .with_min_delay(FIRST_RETRY)
        .with_max_delay(LONGEST_RETRY)
        .with_factor(2.0)
        .without_max_times()
        .build()
}

struct Actor<C, F> {
    settings: RustViewSettings,
    make_client: F,
    client: Arc<C>,
    entries: BTreeMap<JobId, Entry>,
    requests: VecDeque<Request>,
    hashes: VecDeque<String>,
    registered: HashSet<String>,
    tree: Option<Arc<TagTree>>,
    tree_waiters: Vec<Reply<Result<Arc<TagTree>, RustViewError>>>,
    want_tree: bool,
    connection: ConnectionState,
    backoff: Option<Backoff>,
    events: EventBus<SyncEvent>,
    records: mpsc::UnboundedSender<(JobId, JobSyncRecord)>,
}

async fn run<C, F>(mut commands: Mailbox<SyncCommand>, mut actor: Actor<C, F>)
where
    C: RustViewClient,
    F: Fn(&RustViewSettings) -> C + Send + 'static,
{
    tracing::info!(enabled = actor.settings.enabled, "[RustView] Sync started");
    loop {
        if let Some(mut work) = actor.next_work() {
            let answer = loop {
                tokio::select! {
                    biased;
                    answer = &mut work => break answer,
                    command = commands.recv() => match command {
                        Some(SyncCommand::Shutdown { reply }) => {
                            drop(work);
                            actor.stop();
                            let _ = reply.send(());
                            return;
                        }
                        Some(command) => actor.command(command),
                        None => return actor.stop(),
                    },
                }
            };
            actor.finish(answer);
            continue;
        }
        let wake = actor.backoff.as_ref().map(|b| b.retry_at);
        tokio::select! {
            command = commands.recv() => match command {
                Some(SyncCommand::Shutdown { reply }) => {
                    actor.stop();
                    let _ = reply.send(());
                    return;
                }
                Some(command) => actor.command(command),
                None => return actor.stop(),
            },
            () = sleep_until(wake), if wake.is_some() => {}
        }
    }
}

async fn sleep_until(at: Option<Instant>) {
    if let Some(at) = at {
        tokio::time::sleep_until(at).await;
    }
}

impl<C, F> Actor<C, F>
where
    C: RustViewClient,
    F: Fn(&RustViewSettings) -> C + Send + 'static,
{
    fn new(
        settings: RustViewSettings,
        make_client: F,
        events: EventBus<SyncEvent>,
        records: mpsc::UnboundedSender<(JobId, JobSyncRecord)>,
    ) -> Self {
        let client = Arc::new(make_client(&settings));
        let connection = if settings.enabled {
            ConnectionState::Unknown
        } else {
            ConnectionState::Disabled
        };
        Self {
            want_tree: settings.enabled,
            settings,
            make_client,
            client,
            entries: BTreeMap::new(),
            requests: VecDeque::new(),
            hashes: VecDeque::new(),
            registered: HashSet::new(),
            tree: None,
            tree_waiters: Vec::new(),
            connection,
            backoff: None,
            events,
            records,
        }
    }

    fn stop(&mut self) {
        let error = RustViewError::Unavailable("RustCut is quitting".into());
        self.refuse_requests(&error);
        for waiter in self.tree_waiters.drain(..) {
            let _ = waiter.send(Err(error.clone()));
        }
        let open = self.entries.values().filter(|e| e.goal != Goal::Idle).count();
        tracing::info!(open, "[RustView] Sync stopped");
    }

    /// Why a lookup cannot be made right now (switched off, or pausing after a failure).
    fn refusal(&self) -> Option<RustViewError> {
        if !self.settings.enabled {
            return Some(RustViewError::Unavailable(SWITCHED_OFF.into()));
        }
        match (&self.backoff, &self.connection) {
            (Some(backoff), ConnectionState::Unavailable { reason, .. }) if backoff.retry_at > Instant::now() => {
                Some(RustViewError::Unavailable(reason.clone()))
            }
            _ => None,
        }
    }

    fn command(&mut self, command: SyncCommand) {
        match command {
            SyncCommand::Job {
                id,
                target,
                metadata,
                phase,
                record,
            } => self.job(id, target, metadata, phase, record),
            SyncCommand::Cleanup { id, record } => {
                let entry = self.entry(id, record);
                entry.goal = Goal::Clean;
                entry.generation = entry.generation.wrapping_add(1);
            }
            SyncCommand::TagTree { reply } => match (&self.tree, self.refusal()) {
                (Some(tree), _) => {
                    let _ = reply.send(Ok(Arc::clone(tree)));
                }
                (None, Some(error)) => {
                    let _ = reply.send(Err(error));
                }
                (None, None) => {
                    self.tree_waiters.push(reply);
                    self.want_tree = true;
                }
            },
            SyncCommand::MoveRecord {
                id,
                from,
                to,
                replace,
                record,
                reply,
            } => {
                self.follow_move(id, &to, record);
                tracing::info!(job = id.0, replace, "[RustView] The record follows its moved file");
                self.lookup(Request::MoveRecord {
                    id,
                    from,
                    to,
                    replace,
                    reply,
                });
            }
            SyncCommand::GetMedia { path, reply } => self.lookup(Request::GetMedia { path, reply }),
            SyncCommand::CheckHash { md5, reply } => self.lookup(Request::CheckHash { md5, reply }),
            SyncCommand::InsertHash { md5 } => {
                let md5 = md5.to_ascii_lowercase();
                if !is_md5(&md5) {
                    tracing::warn!(md5, "[RustView] Not an MD5; not registered");
                } else if !self.registered.contains(&md5) && !self.hashes.contains(&md5) {
                    self.hashes.push_back(md5);
                }
            }
            SyncCommand::Probe => {
                if let Some(backoff) = &mut self.backoff {
                    backoff.retry_at = Instant::now();
                }
                self.want_tree = self.settings.enabled;
            }
            SyncCommand::Test { reply } | SyncCommand::RefreshTree { reply } => {
                if !self.settings.enabled {
                    let _ = reply.send(Err(RustViewError::Unavailable(SWITCHED_OFF.into())));
                    return;
                }
                if let Some(backoff) = &mut self.backoff {
                    backoff.retry_at = Instant::now();
                }
                self.tree_waiters.push(reply);
                self.want_tree = true;
            }
            SyncCommand::Reconfigure { settings } => self.reconfigure(settings),
            // The loop handles it.
            SyncCommand::Shutdown { reply } => {
                let _ = reply.send(());
            }
        }
    }

    fn lookup(&mut self, request: Request) {
        match self.refusal() {
            Some(error) => {
                if let Request::MoveRecord { id, .. } = &request {
                    self.move_refused(*id);
                }
                request.refuse(error);
            }
            None => self.requests.push_back(request),
        }
    }

    /// Refuses every waiting lookup; a record move among them counts as refused.
    pub(super) fn refuse_requests(&mut self, error: &RustViewError) {
        let requests: Vec<Request> = self.requests.drain(..).collect();
        for request in requests {
            if let Request::MoveRecord { id, .. } = &request {
                self.move_refused(*id);
            }
            request.refuse(error.clone());
        }
    }

    /// RustView did not move the record: whatever is at the new path is not known to be RustCut's, so nothing there is
    /// ever moved or deleted as RustCut's own; a later write asks RustView first (user decision Q-P12f: the user moves
    /// the old entry, RustCut never deletes and sets it).
    pub(super) fn move_refused(&mut self, id: JobId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.record.preexisting = None;
            entry.record.previous_tags = None;
            entry.record.previous_rating = None;
            let record = entry.record.clone();
            self.report(id, record);
        }
    }

    /// RustCut counts the job's record as moved to `to` (whatever RustView answers): later writes go there, nothing is
    /// cleaned at the old path.
    fn follow_move(&mut self, id: JobId, to: &std::path::Path, record: JobSyncRecord) {
        let entry = self.entry(id, record);
        if entry.record.synced_path.is_none() {
            return;
        }
        entry.record.synced_path = Some(to.to_path_buf());
        if entry.target.is_some() {
            entry.target = Some(to.to_path_buf());
        }
        entry.generation = entry.generation.wrapping_add(1);
        let record = entry.record.clone();
        self.report(id, record);
    }

    fn entry(&mut self, id: JobId, record: JobSyncRecord) -> &mut Entry {
        // A known job keeps the actor's record: it is newer than the one the command carries.
        self.entries.entry(id).or_insert_with(|| Entry {
            target: None,
            metadata: JobMetadata::default(),
            goal: Goal::Idle,
            record,
            generation: 0,
        })
    }

    fn job(&mut self, id: JobId, target: PathBuf, metadata: JobMetadata, phase: SyncPhase, record: JobSyncRecord) {
        let enabled = self.settings.enabled;
        let timing = self.settings.write_timing;
        let entry = self.entry(id, record);
        // Editing a finished job keeps the "file exists" phase; a restart (Enqueued) does not.
        let phase = match (entry.goal, phase) {
            (Goal::Write(SyncPhase::Finished), SyncPhase::Edited) => SyncPhase::Finished,
            _ => phase,
        };
        let pending = enabled
            && matches!(entry.record.state, SyncState::NotSynced | SyncState::CleanedUp)
            && (timing == WriteTiming::AtEnqueue || phase == SyncPhase::Finished);
        entry.target = Some(target);
        entry.metadata = metadata;
        entry.goal = Goal::Write(phase);
        entry.generation = entry.generation.wrapping_add(1);
        if pending {
            entry.record.state = SyncState::Pending;
            let record = entry.record.clone();
            self.report(id, record);
        }
    }

    fn reconfigure(&mut self, settings: RustViewSettings) {
        let was_enabled = self.settings.enabled;
        self.client = Arc::new((self.make_client)(&settings));
        self.settings = settings;
        self.backoff = None;
        self.tree = None;
        if self.settings.enabled {
            self.want_tree = true;
            self.set_connection(ConnectionState::Unknown);
        } else {
            self.want_tree = false;
            let error = RustViewError::Unavailable(SWITCHED_OFF.into());
            self.refuse_requests(&error);
            for waiter in self.tree_waiters.drain(..) {
                let _ = waiter.send(Err(error.clone()));
            }
            self.set_connection(ConnectionState::Disabled);
        }
        tracing::info!(
            enabled = self.settings.enabled,
            was_enabled,
            timing = ?self.settings.write_timing,
            "[RustView] Settings changed"
        );
    }
}
