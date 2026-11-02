//! Moving the targets of jobs (P12 block 7, user decision Q-P12f). A job still to render only changes its target; a
//! rendering job changes its target now and its file moves after the verified render; a finished job's file moves. File
//! moves run one after the other on the blocking pool (`files::relocate`: a rename, or a verified copy across file
//! systems). Every move is written to the job's journal (`relocation` column) before anything happens and cleared after,
//! so a crash is settled at the next start. The RustView record follows with `move-media`; when RustView cannot do it the
//! outcome says so (the app's warning dialog), and the record is never deleted and set instead.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::Actor;
use super::work::{Report, validate};
use crate::files::{self, Interrupted, Method, RelocateError};
use crate::job::{JobId, JobSpec, JobState, JobStateKind};
use crate::naming::same_path;
use crate::queue::summary::stored_temp;
use crate::queue::{QueueError, QueueEvent, Relocated, Relocation, RelocationOutcome, RelocationProgress, TargetMove};
use crate::rustview::{JobSyncRecord, SyncCommand, SyncPhase};
use crate::store::{LogStream, SpecSlot, StoreCommand};

/// How often a running copy reports its bytes.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The file moves of the queue.
#[derive(Default)]
pub(super) struct Relocations {
    /// Finished jobs whose file waits to move (their journal is stored).
    queue: VecDeque<JobId>,
    running: Option<Running>,
    /// This run: moves ended and planned, bytes of the copies done and planned. Reset when nothing is left.
    finished: usize,
    total: usize,
    bytes_done: u64,
    bytes_total: u64,
    /// Sizes of the queued copies (renames are not counted).
    sizes: HashMap<JobId, u64>,
    /// Outcomes waiting for RustView's answer about the record.
    awaiting_record: HashMap<JobId, RelocationOutcome>,
}

struct Running {
    id: JobId,
    name: String,
    cancel: Arc<AtomicBool>,
    copied: u64,
}

/// What happens to a job's RustView record when its target changes.
enum RecordStep {
    Nothing,
    /// The record follows: `move-media` from where it was written.
    Move {
        from: PathBuf,
    },
    /// The sync's own way (a foreign record gets its values back and the new target its own record).
    Resync,
}

/// The new spec for `to` (folder and name from the path; the container keeps the extension).
fn spec_for(spec: &JobSpec, to: &Path, overwrite: bool) -> Option<JobSpec> {
    let folder = to.parent()?.to_path_buf();
    let stem = to.file_stem()?.to_string_lossy().into_owned();
    let mut next = spec.clone();
    next.target.folder = folder;
    if next.target.stem != stem {
        next.target.stem = stem;
        next.target.naming = None;
    }
    next.target.overwrite = overwrite;
    Some(next)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

impl Actor {
    /// `QueueCommand::MoveTargets`.
    pub(super) async fn move_targets(&mut self, moves: Vec<TargetMove>) -> Result<usize, QueueError> {
        let mut taken = 0;
        let mut copies = Vec::new();
        for mv in moves {
            match self.take_move(&mv).await {
                Ok(Some(copy)) => {
                    taken += 1;
                    copies.push(copy);
                }
                Ok(None) => taken += 1,
                Err(reason) => {
                    let from = self.model.get(mv.id).map(|r| r.target_path.clone()).unwrap_or_default();
                    let to = mv.folder.clone();
                    tracing::warn!(job = mv.id.0, reason, "[Move] A target was not changed");
                    self.publish_outcome(RelocationOutcome {
                        id: mv.id,
                        from,
                        to,
                        result: Err(reason),
                        record_error: None,
                    });
                }
            }
        }
        self.count_bytes(copies).await;
        self.kick().await;
        self.next_relocation();
        self.publish_relocation();
        Ok(taken)
    }

    /// Adds the sizes of the moves that copy (another file system) to this run's bytes.
    async fn count_bytes(&mut self, moves: Vec<(JobId, PathBuf, PathBuf)>) {
        if moves.is_empty() {
            return;
        }
        let sizes = tokio::task::spawn_blocking(move || {
            moves
                .into_iter()
                .map(|(id, from, folder)| {
                    let size = match files::needs_copy(&from, &folder) {
                        Some(true) => std::fs::metadata(&from).map_or(0, |m| m.len()),
                        _ => 0,
                    };
                    (id, size)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (id, size) in sizes {
            self.relocations.bytes_total = self.relocations.bytes_total.saturating_add(size);
            self.relocations.sizes.insert(id, size);
        }
    }

    /// Takes one job's new target. `Ok(Some((id, from, folder)))` = a file move was queued.
    async fn take_move(&mut self, mv: &TargetMove) -> Result<Option<(JobId, PathBuf, PathBuf)>, String> {
        let record = self.model.get(mv.id).ok_or("the job does not exist any more")?.clone();
        let SpecSlot::Decoded(spec) = &record.spec else {
            return Err("this job was written by another RustCut version and cannot be changed".into());
        };
        let journal = Relocation::from_column(record.relocation.as_deref());
        let kind = record.state.kind();
        if journal
            .as_ref()
            .is_some_and(|j| !j.after_render || kind == JobStateKind::Finished)
        {
            return Err("its file is being moved already".into());
        }
        let Some(next) = mv.apply(spec) else {
            return Ok(None);
        };
        validate(&next).map_err(|e| e.to_string())?;
        let from = record.target_path.clone();
        let to = next.target_path();
        if kind == JobStateKind::Finished {
            let journal = Relocation {
                from: from.clone(),
                to,
                overwrite: mv.overwrite,
                after_render: false,
                restore_overwrite: spec.target.overwrite,
            };
            self.set_journal(mv.id, Some(&journal)).await;
            self.relocations.queue.push_back(mv.id);
            self.relocations.total += 1;
            self.log(
                mv.id,
                format!(
                    "[Move] Moving the file from {} to {}",
                    from.display(),
                    journal.to.display()
                ),
            );
            return Ok(Some((mv.id, from, mv.folder.clone())));
        }
        let step = self.record_step(&record, false);
        self.store_spec(mv.id, Box::new(next))
            .await
            .map_err(|e| e.to_string())?;
        let rendering = kind.is_active();
        if rendering {
            // The render puts its file where the job pointed when it started: that is where the file moves from.
            let render_at = journal.as_ref().map_or_else(|| from.clone(), |j| j.from.clone());
            if same_path(&render_at, &to) {
                self.set_journal(mv.id, None).await;
            } else {
                let restore = journal.as_ref().map_or(spec.target.overwrite, |j| j.restore_overwrite);
                let journal = Relocation {
                    from: render_at,
                    to: to.clone(),
                    overwrite: mv.overwrite,
                    after_render: true,
                    restore_overwrite: restore,
                };
                self.set_journal(mv.id, Some(&journal)).await;
            }
        }
        self.log(
            mv.id,
            format!("[Move] Target changed from {} to {}", from.display(), to.display()),
        );
        let result = if rendering
            && Relocation::from_column(self.model.get(mv.id).and_then(|r| r.relocation.as_deref())).is_some()
        {
            Relocated::AfterRender
        } else {
            Relocated::TargetChanged
        };
        let outcome = RelocationOutcome {
            id: mv.id,
            from: from.clone(),
            to: to.clone(),
            result: Ok(result),
            record_error: None,
        };
        let syncs = matches!(
            kind,
            JobStateKind::Waiting | JobStateKind::Held | JobStateKind::Rendering | JobStateKind::Paused
        );
        self.follow_record(
            mv.id,
            step,
            &to,
            mv.overwrite,
            outcome,
            syncs.then_some(SyncPhase::Edited),
        );
        Ok(None)
    }

    /// What the job's record does when its file or target moves. `finished`: the file at the old place is the job's.
    fn record_step(&self, record: &crate::store::JobRecord, finished: bool) -> RecordStep {
        if self.sync.is_none() {
            return RecordStep::Nothing;
        }
        let SpecSlot::Decoded(spec) = &record.spec else {
            return RecordStep::Nothing;
        };
        if spec.metadata.without_rustview {
            return RecordStep::Nothing;
        }
        let sync = JobSyncRecord::from_column(record.sync.as_deref());
        let Some(at) = sync.synced_path.clone() else {
            return if finished {
                RecordStep::Nothing
            } else {
                RecordStep::Resync
            };
        };
        if finished {
            return RecordStep::Move { from: at };
        }
        let shared = self.model.jobs().iter().any(|other| {
            other.id != record.id
                && JobSyncRecord::from_column(other.sync.as_deref())
                    .synced_path
                    .is_some_and(|p| same_path(&p, &at))
        });
        if shared || sync.preexisting != Some(false) {
            RecordStep::Resync
        } else {
            RecordStep::Move { from: at }
        }
    }

    /// Lets the record follow and publishes the outcome once RustView answered (at once without a record to move).
    /// `phase`: the sync is told the job's new target afterwards.
    fn follow_record(
        &mut self,
        id: JobId,
        step: RecordStep,
        to: &Path,
        replace: bool,
        outcome: RelocationOutcome,
        phase: Option<SyncPhase>,
    ) {
        match step {
            RecordStep::Move { from } if !same_path(&from, to) => {
                let Some(sync) = self.sync.clone() else { return };
                let record = self
                    .model
                    .get(id)
                    .map(|r| JobSyncRecord::from_column(r.sync.as_deref()))
                    .unwrap_or_default();
                let (reply, answer) = tokio::sync::oneshot::channel();
                let command = SyncCommand::MoveRecord {
                    id,
                    from,
                    to: to.to_path_buf(),
                    replace,
                    record,
                    reply,
                };
                if sync.send(command).is_err() {
                    let mut outcome = outcome;
                    outcome.record_error = Some("the RustView sync has stopped".into());
                    self.publish_outcome(outcome);
                } else {
                    self.relocations.awaiting_record.insert(id, outcome);
                    let reports = self.reports.clone();
                    tokio::spawn(async move {
                        let result = match answer.await {
                            Ok(Ok(_)) => Ok(()),
                            Ok(Err(e)) => Err(e.to_string()),
                            Err(_) => Err("the RustView sync has stopped".into()),
                        };
                        let _ = reports.send(Report::RecordMoved { id, result });
                    });
                }
            }
            RecordStep::Move { .. } | RecordStep::Nothing | RecordStep::Resync => self.publish_outcome(outcome),
        }
        if let Some(phase) = phase {
            self.sync_job(id, phase);
        }
    }

    /// RustView answered about a record that followed its file.
    pub(super) fn record_moved(&mut self, id: JobId, result: Result<(), String>) {
        let Some(mut outcome) = self.relocations.awaiting_record.remove(&id) else {
            return;
        };
        if let Err(reason) = result {
            self.log(
                id,
                format!(
                    "[Move] The RustView record stayed at {}: {reason}",
                    outcome.from.display()
                ),
            );
            outcome.record_error = Some(reason);
        }
        self.publish_outcome(outcome);
    }

    fn publish_outcome(&self, outcome: RelocationOutcome) {
        self.events.publish(QueueEvent::Relocated(Box::new(outcome)));
    }

    /// Stores (or clears) a job's journal and shows the row.
    async fn set_journal(&mut self, id: JobId, journal: Option<&Relocation>) {
        let relocation = journal.and_then(Relocation::to_column);
        if let Some(mut record) = self.model.get(id).cloned() {
            record.relocation.clone_from(&relocation);
            self.model.replace(record);
        }
        if let Err(e) = self
            .store_request(|reply| StoreCommand::SetRelocation { id, relocation, reply })
            .await
        {
            tracing::warn!(job = id.0, error = %e, "[Move] The journal of the move was not saved");
        }
        self.publish_row(id);
    }

    fn log(&self, id: JobId, line: String) {
        tracing::info!(job = id.0, "{line}");
        let _ = self.store.send(StoreCommand::AppendLog {
            id,
            stream: LogStream::App,
            lines: vec![line],
        });
    }

    /// The journal of a job, if a move is unfinished.
    pub(super) fn journal(&self, id: JobId) -> Option<Relocation> {
        Relocation::from_column(self.model.get(id)?.relocation.as_deref())
    }

    /// Starts the next file move when none runs.
    fn next_relocation(&mut self) {
        if self.relocations.running.is_some() || self.stopping {
            return;
        }
        while let Some(id) = self.relocations.queue.pop_front() {
            let Some(journal) = self.journal(id) else {
                self.relocations.finished += 1;
                continue;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            self.relocations.running = Some(Running {
                id,
                name: name_of(&journal.from),
                cancel: Arc::clone(&cancel),
                copied: 0,
            });
            let reports = self.reports.clone();
            tokio::task::spawn_blocking(move || {
                let mut last: Option<Instant> = None;
                let mut progress = |done: u64, total: u64| {
                    if done >= total || last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                        last = Some(Instant::now());
                        let _ = reports.send(Report::MoveProgress { id, done });
                    }
                };
                let result = files::relocate(&journal.from, &journal.to, journal.overwrite, &mut progress, &cancel);
                let _ = reports.send(Report::MoveEnded { id, result });
            });
            return;
        }
        if self.relocations.finished >= self.relocations.total {
            self.relocations.finished = 0;
            self.relocations.total = 0;
            self.relocations.bytes_done = 0;
            self.relocations.bytes_total = 0;
            self.relocations.sizes.clear();
        }
    }

    pub(super) fn move_progress(&mut self, id: JobId, done: u64) {
        if let Some(running) = self.relocations.running.as_mut().filter(|r| r.id == id) {
            running.copied = done;
            self.publish_relocation();
        }
    }

    /// A file move ended.
    pub(super) async fn move_ended(&mut self, id: JobId, result: Result<Method, RelocateError>) {
        let running = self.relocations.running.take().filter(|r| r.id == id);
        let size = self.relocations.sizes.remove(&id).unwrap_or(0);
        self.relocations.finished += 1;
        self.relocations.bytes_done = self.relocations.bytes_done.saturating_add(size);
        let Some(journal) = self.journal(id) else {
            self.after_move().await;
            return;
        };
        if self.stopping && result == Err(RelocateError::Canceled) {
            // RustCut quits: the journal stays, the move starts again at the next start.
            self.log(
                id,
                "[Move] Stopped because RustCut quit; it goes on at the next start".into(),
            );
            return;
        }
        drop(running);
        match result {
            Ok(method) => self.file_moved(id, &journal, method).await,
            Err(error) => {
                let canceled = error == RelocateError::Canceled;
                self.give_up(
                    id,
                    &journal,
                    if canceled {
                        Ok(Relocated::Canceled)
                    } else {
                        Err(error.to_string())
                    },
                )
                .await;
            }
        }
        self.after_move().await;
    }

    async fn after_move(&mut self) {
        self.next_relocation();
        self.publish_relocation();
        self.kick().await;
    }

    /// The file is at its new place: the spec follows (unless it did already), the journal goes, the record follows.
    async fn file_moved(&mut self, id: JobId, journal: &Relocation, method: Method) {
        let how = match method {
            Method::Copied { bytes } => format!("copied {bytes} bytes, verified"),
            Method::Renamed => "renamed".into(),
            Method::Unchanged => "nothing to do".into(),
        };
        self.log(
            id,
            format!(
                "[Move] Moved from {} to {} ({how})",
                journal.from.display(),
                journal.to.display()
            ),
        );
        let Some(record) = self.model.get(id).cloned() else {
            return;
        };
        if !journal.after_render
            && let SpecSlot::Decoded(spec) = &record.spec
            && let Some(next) = spec_for(spec, &journal.to, journal.overwrite)
            && let Err(e) = self.store_spec(id, Box::new(next)).await
        {
            tracing::warn!(job = id.0, error = %e, "[Move] The job's new target was not saved");
        }
        self.set_journal(id, None).await;
        // A rendering job's record followed when its target changed.
        let step = if journal.after_render {
            RecordStep::Nothing
        } else {
            self.record_step(&record, true)
        };
        let outcome = RelocationOutcome {
            id,
            from: journal.from.clone(),
            to: journal.to.clone(),
            result: Ok(match method {
                Method::Copied { .. } => Relocated::Copied,
                Method::Renamed | Method::Unchanged => Relocated::Renamed,
            }),
            record_error: None,
        };
        self.follow_record(
            id,
            step,
            &journal.to,
            journal.overwrite,
            outcome,
            Some(SyncPhase::Finished),
        );
    }

    /// A move did not happen (canceled or failed): the journal goes; a job that waited for its render gets its old
    /// target back, and its record goes back with it.
    async fn give_up(&mut self, id: JobId, journal: &Relocation, result: Result<Relocated, String>) {
        if let Err(reason) = &result {
            self.log(
                id,
                format!("[Move] The file stays at {}: {reason}", journal.from.display()),
            );
        }
        self.set_journal(id, None).await;
        // A waiting move taken back goes the other way: from the new target back to the old one.
        let (from, to) = if journal.after_render {
            (journal.to.clone(), journal.from.clone())
        } else {
            (journal.from.clone(), journal.to.clone())
        };
        let outcome = RelocationOutcome {
            id,
            from,
            to,
            result,
            record_error: None,
        };
        if !journal.after_render {
            self.publish_outcome(outcome);
            return;
        }
        let Some(record) = self.model.get(id).cloned() else {
            return;
        };
        if let SpecSlot::Decoded(spec) = &record.spec
            && let Some(back) = spec_for(spec, &journal.from, journal.restore_overwrite)
            && let Err(e) = self.store_spec(id, Box::new(back)).await
        {
            tracing::warn!(job = id.0, error = %e, "[Move] The old target was not saved");
        }
        self.log(id, format!("[Move] The target is {} again", journal.from.display()));
        let finished = record.state == JobState::Finished;
        let step = self.record_step(&record, finished);
        let phase = if finished {
            SyncPhase::Finished
        } else {
            SyncPhase::Edited
        };
        self.follow_record(id, step, &journal.from, false, outcome, Some(phase));
    }

    /// A render ended for a job whose file waits to move: a verified render queues the move, any other end keeps the new
    /// target with nothing to move.
    pub(super) async fn render_ended_moving(&mut self, id: JobId, next: &JobState) {
        let Some(journal) = self.journal(id).filter(|j| j.after_render) else {
            return;
        };
        match next.kind() {
            JobStateKind::Finished => {
                let waiting = Relocation {
                    after_render: true,
                    ..journal
                };
                self.set_journal(id, Some(&waiting)).await;
                let folder = waiting.to.parent().map(Path::to_path_buf).unwrap_or_default();
                self.count_bytes(vec![(id, waiting.from.clone(), folder)]).await;
                self.relocations.queue.push_back(id);
                self.relocations.total += 1;
                self.next_relocation();
            }
            // Canceled, failed, or stopped at quit: the new target stays and nothing has to move.
            _ => self.set_journal(id, None).await,
        }
        self.publish_relocation();
    }

    /// `QueueCommand::CancelRelocations`.
    pub(super) async fn cancel_relocations(&mut self, running: bool, waiting: bool) {
        if running {
            if let Some(run) = &self.relocations.running {
                run.cancel.store(true, Ordering::Relaxed);
                tracing::info!(job = run.id.0, "[Move] Canceled");
            }
            let queued: Vec<JobId> = self.relocations.queue.drain(..).collect();
            for id in queued {
                self.relocations.finished += 1;
                self.relocations.sizes.remove(&id);
                if let Some(journal) = self.journal(id) {
                    self.give_up(id, &journal, Ok(Relocated::Canceled)).await;
                }
            }
        }
        if waiting {
            let ids: Vec<JobId> = self
                .model
                .jobs()
                .iter()
                .filter(|j| j.state.kind().is_active())
                .map(|j| j.id)
                .collect();
            for id in ids {
                if let Some(journal) = self.journal(id).filter(|j| j.after_render) {
                    self.give_up(id, &journal, Ok(Relocated::Canceled)).await;
                }
            }
        }
        self.next_relocation();
        self.publish_relocation();
        self.kick().await;
    }

    /// At start: settles the moves a crash or a quit cut short.
    pub(super) async fn resume_relocations(&mut self) {
        let open: Vec<(JobId, Relocation, JobState)> = self
            .model
            .jobs()
            .iter()
            .filter_map(|j| Some((j.id, Relocation::from_column(j.relocation.as_deref())?, j.state.clone())))
            .collect();
        for (id, journal, state) in open {
            if state != JobState::Finished {
                // The render starts again and writes to the new target; what the old one left goes.
                if let Some(temp) = stored_temp(&journal.from)
                    && tokio::fs::remove_file(&temp).await.is_ok()
                {
                    tracing::info!(job = id.0, "[Move] The old target's temp file was removed");
                }
                self.set_journal(id, None).await;
                continue;
            }
            let (from, to) = (journal.from.clone(), journal.to.clone());
            let settled = tokio::task::spawn_blocking(move || files::settle_interrupted(&from, &to))
                .await
                .unwrap_or(Interrupted::Again);
            tracing::info!(job = id.0, ?settled, "[Move] An interrupted move was found");
            match settled {
                Interrupted::Again => {
                    self.relocations.queue.push_back(id);
                    self.relocations.total += 1;
                }
                Interrupted::Done => self.file_moved(id, &journal, Method::Renamed).await,
                Interrupted::Missing => {
                    self.give_up(
                        id,
                        &journal,
                        Err("the file is neither at its old nor at its new place".into()),
                    )
                    .await;
                }
            }
        }
        self.next_relocation();
        self.publish_relocation();
    }

    /// Stops a running copy at quit (its journal stays); `true` while one runs.
    pub(super) fn stop_relocation(&self) -> bool {
        match &self.relocations.running {
            Some(run) => {
                run.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub(super) fn relocation_running(&self) -> bool {
        self.relocations.running.is_some()
    }

    pub(super) fn publish_relocation(&self) {
        let r = &self.relocations;
        let waiting = self
            .model
            .jobs()
            .iter()
            .filter(|j| j.state.kind().is_active())
            .filter(|j| Relocation::from_column(j.relocation.as_deref()).is_some_and(|x| x.after_render))
            .map(|j| j.id)
            .collect();
        let copied = r.running.as_ref().map_or(0, |run| run.copied);
        self.events.publish(QueueEvent::Relocation(RelocationProgress {
            current: r.running.as_ref().map(|run| (run.id, run.name.clone())),
            finished: r.finished,
            total: r.total,
            bytes_done: r.bytes_done.saturating_add(copied).min(r.bytes_total),
            bytes_total: r.bytes_total,
            waiting,
        }));
    }
}
