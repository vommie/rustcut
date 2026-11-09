//! Running events through the plugins: one event after the other, each through every running plugin in the user's
//! order. A later plugin's context carries what the earlier ones changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustcut_plugin_host::runtime::{CallError, Worker};
use rustcut_plugin_host::types::{Action, Context, Event, LogLine, Target};
use tokio::sync::{mpsc, oneshot};

use super::{Outcome, PluginCommand};
use crate::actor::Handle;

/// How long past its own time limit a call may stay unanswered before the dispatcher stops waiting (the sandbox
/// stops it at the limit; this only guards against a lost answer).
const GRACE: Duration = Duration::from_secs(2);

/// A running plugin as the dispatcher sees it.
#[derive(Clone)]
pub(super) struct Runner {
    pub id: String,
    pub name: String,
    pub worker: Worker,
    pub limit: Duration,
}

/// The running plugins in order; the actor replaces the list, the dispatcher clones it per event.
pub(super) type Runners = Arc<parking_lot::Mutex<Arc<Vec<Runner>>>>;

/// What kind of call a report is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Configure,
    Handle(&'static str),
    Check,
}

/// The result of one call, for the statistics, the failure count and the log.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub id: String,
    pub kind: CallKind,
    pub took: Duration,
    pub error: Option<CallError>,
    pub log: Vec<LogLine>,
}

pub(super) struct Job {
    pub event: Event,
    pub context: Context,
    pub folders: Arc<BTreeMap<u32, PathBuf>>,
    pub outcomes: mpsc::UnboundedSender<Outcome>,
}

/// The dispatcher's loop; it ends when the actor drops the sender.
pub(super) async fn run(mut jobs: mpsc::UnboundedReceiver<Job>, runners: Runners, actor: Handle<PluginCommand>) {
    while let Some(job) = jobs.recv().await {
        let list = Arc::clone(&runners.lock());
        let mut context = job.context;
        for runner in list.iter() {
            let (tx, rx) = oneshot::channel();
            runner.worker.handle(
                job.event.clone(),
                context.clone(),
                Box::new(move |report| {
                    let _ = tx.send(report);
                }),
            );
            let (took, result, log) = match tokio::time::timeout(runner.limit + GRACE, rx).await {
                Ok(Ok(report)) => (report.took, report.result, report.log),
                Ok(Err(_)) => (
                    Duration::ZERO,
                    Err(CallError::Start("the plugin's thread ended".to_owned())),
                    Vec::new(),
                ),
                Err(_) => (runner.limit + GRACE, Err(CallError::Timeout(runner.limit)), Vec::new()),
            };
            let error = result.as_ref().err().cloned();
            let report = Report {
                id: runner.id.clone(),
                kind: CallKind::Handle(job.event.name()),
                took,
                error,
                log,
            };
            let _ = actor.send(PluginCommand::Reported(Box::new(report)));
            if let Ok(actions) = result {
                for action in &actions {
                    apply(&mut context, action, &job.folders);
                }
                if !actions.is_empty() {
                    let _ = job.outcomes.send(Outcome {
                        id: runner.id.clone(),
                        name: runner.name.clone(),
                        actions,
                    });
                }
            }
        }
    }
}

/// What an action changes in the context the next plugin sees.
pub(super) fn apply(context: &mut Context, action: &Action, folders: &BTreeMap<u32, PathBuf>) {
    match action {
        Action::SetTarget {
            target: Target::Path(path),
            ..
        } => {
            context.target = Some(path.clone());
            context.target_folder = None;
        }
        Action::SetTarget {
            target: Target::Folder(id),
            ..
        } => {
            // 0 is "Next to the source": the video's own folder.
            let path = if *id == 0 {
                context.video.as_deref().and_then(Path::parent).map(Path::to_path_buf)
            } else {
                folders.get(id).cloned()
            };
            if let Some(path) = path {
                context.target = Some(path);
                context.target_folder = Some(*id);
            }
        }
        Action::SetRating { stars, .. } => context.rating = Some((*stars).min(5)),
        Action::Notify(_) => {}
    }
}
