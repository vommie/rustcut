//! Moving the target files of jobs (P12 block 7, user decision Q-P12f): the dialog started from the queue's menu, its
//! checks on the runtime (folder scans, permissions, free space, the files and their drives), the status bar's segments
//! while files move or wait for their renders, the results, and the warning about RustView records that could not
//! follow. `plan.rs` decides everything without Slint; the queue engine does the moves (`QueueCommand::MoveTargets`).

pub mod plan;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use parking_lot::Mutex;
use plan::{Facts, Folder, Job, Level, Plan, Resolution};
use rustcut_core::folders::{FolderId, FolderTree};
use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::queue::{
    JobSummary, QueueCommand, QueueEvent, QueueHandle, QueueService, Relocated, RelocationOutcome, RelocationProgress,
};
use rustcut_core::rustview::{ConnectionState, SyncEvent};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::ui::export::ExportBridge;
use crate::ui::folders::select::{FolderSelect, Session};
use crate::ui::notices::{self, Kind, Level as NoticeLevel, Notice};
use crate::ui::queue::QueueBridge;
use crate::ui::status::activity;
use crate::util::clipboard;
use crate::{MainWindow, Relocate, RelocateCheckView, RelocateConflictView, RelocateRecordView};

/// The folder field's owner name.
const OWNER: &str = "relocate";
/// The status bar's segments.
const MOVING: &str = "relocate";
const WAITING: &str = "relocate-wait";
const PROGRESS: CoalesceKey = CoalesceKey::new("relocate", 0);

thread_local! {
    static BRIDGE: RefCell<Weak<RelocateBridge>> = const { RefCell::new(Weak::new()) };
}

/// The open dialog.
struct Dialog {
    jobs: Vec<Job>,
    folder: Folder,
    /// The folder came from the list (its entry), for the field's picture.
    entry: Option<FolderId>,
    /// The typed name of a single job.
    name: Option<String>,
    resolution: Resolution,
    facts: Facts,
    plan: Plan,
    /// Raised by every change; a check that finishes for an older one is dropped.
    generation: u64,
    /// Move was pressed: the checks run once more, and the moves go when nothing changed.
    confirming: bool,
}

/// The moves sent to the queue whose outcomes are awaited, for one result message.
#[derive(Default)]
struct Batch {
    waiting: BTreeSet<JobId>,
    outcomes: Vec<RelocationOutcome>,
}

pub struct RelocateBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    queue: QueueHandle,
    queue_bridge: Weak<QueueBridge>,
    export: Weak<ExportBridge>,
    select: Rc<FolderSelect>,
    dialog: RefCell<Option<Dialog>>,
    batch: RefCell<Batch>,
    /// Records that could not follow their files, shown in the warning dialog.
    records: RefCell<Vec<RelocationOutcome>>,
    connection: Arc<Mutex<ConnectionState>>,
    progress: Arc<Mutex<RelocationProgress>>,
    /// The segments show.
    moving_shown: Cell<bool>,
    waiting_shown: Cell<bool>,
}

impl RelocateBridge {
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        service: &QueueService,
        sync_events: Option<tokio::sync::broadcast::Receiver<SyncEvent>>,
        queue_bridge: &Rc<QueueBridge>,
        export: &Rc<ExportBridge>,
        runtime: tokio::runtime::Handle,
    ) -> Rc<Self> {
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime: runtime.clone(),
            queue: service.handle.clone(),
            queue_bridge: Rc::downgrade(queue_bridge),
            export: Rc::downgrade(export),
            select: export.select(),
            dialog: RefCell::new(None),
            batch: RefCell::default(),
            records: RefCell::default(),
            connection: Arc::default(),
            progress: Arc::default(),
            moving_shown: Cell::new(false),
            waiting_shown: Cell::new(false),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        connect(&bridge, window);
        runtime.spawn(follow(service.clone(), ui.clone(), Arc::clone(&bridge.progress)));
        if let Some(events) = sync_events {
            runtime.spawn(follow_sync(events, Arc::clone(&bridge.connection)));
        }
        bridge
    }

    /// The queue's "Move Target File": the dialog for the selected jobs.
    pub fn open(&self, rows: &[JobSummary]) {
        if rows.is_empty() {
            return;
        }
        let jobs: Vec<Job> = rows.iter().map(job_of).collect();
        let single = jobs.len() == 1;
        let folder = match rows {
            [only] => only
                .target
                .parent()
                .map_or(Folder::None, |p| Folder::Path(p.to_path_buf())),
            _ => Folder::None,
        };
        let name = single.then(|| jobs.first().map(|j| j.stem.clone()).unwrap_or_default());
        tracing::info!(jobs = jobs.len(), "[Move] Dialog opened");
        self.dialog.replace(Some(Dialog {
            jobs,
            folder,
            entry: None,
            name,
            resolution: Resolution::AddNumber,
            facts: Facts::default(),
            plan: Plan::default(),
            generation: 0,
            confirming: false,
        }));
        if let Some(window) = self.window.upgrade() {
            window.global::<Relocate<'_>>().set_resolution(0);
            window.global::<Relocate<'_>>().set_open(true);
        }
        self.changed();
    }

    fn close(&self) {
        self.dialog.replace(None);
        if self.select.is_open_for(OWNER) {
            self.select.close();
        }
        if let Some(window) = self.window.upgrade() {
            window.global::<Relocate<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
    }

    /// The choice changed: plan with what is known, then check what is not known yet on the runtime.
    fn changed(&self) {
        let missing = {
            let mut d = self.dialog.borrow_mut();
            let Some(d) = d.as_mut() else { return };
            d.generation = d.generation.wrapping_add(1);
            d.confirming = false;
            self.replan(d);
            unknown(d)
        };
        self.publish();
        if let Some(request) = missing {
            self.check(request);
        }
    }

    fn replan(&self, d: &mut Dialog) {
        let queue = self.queue_bridge.upgrade().map(|q| q.targets()).unwrap_or_default();
        d.plan = plan::plan(&d.jobs, &d.folder, d.name.as_deref(), &queue, &d.facts, d.resolution);
    }

    /// Checks the folders and files of the current choice on the runtime.
    fn check(&self, request: Request) {
        let generation = request.generation;
        let task = self.runtime.spawn_blocking(move || gather(request));
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(found) = task.await else { return };
            let Some(bridge) = weak.upgrade() else { return };
            bridge.checked(generation, found);
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Move] The event loop is gone; the checks were dropped");
        }
    }

    fn checked(&self, generation: u64, found: Facts) {
        let send = {
            let mut d = self.dialog.borrow_mut();
            let Some(d) = d.as_mut() else { return };
            if d.generation != generation {
                return;
            }
            let before = d.plan.clone();
            d.facts.scans.extend(found.scans);
            d.facts.unusable.retain(|k, _| !found.free.contains_key(k));
            d.facts.unusable.extend(found.unusable);
            d.facts.free.extend(found.free);
            d.facts.files.extend(found.files);
            self.replan(d);
            let confirming = std::mem::take(&mut d.confirming);
            // Move was pressed: the moves go only when the fresh checks left everything as the user saw it.
            (confirming && d.plan == before && d.plan.ready()).then(|| d.plan.moves.clone())
        };
        match send {
            Some(moves) => self.send(moves),
            None => self.publish(),
        }
    }

    /// Move: the drives are checked once more right before (the user may have waited a while).
    fn confirm(&self) {
        let request = {
            let mut d = self.dialog.borrow_mut();
            let Some(d) = d.as_mut() else { return };
            if !d.plan.ready() {
                return;
            }
            d.generation = d.generation.wrapping_add(1);
            d.confirming = true;
            request_for(d, true)
        };
        self.check(request);
    }

    fn send(&self, moves: Vec<rustcut_core::queue::TargetMove>) {
        tracing::info!(moves = moves.len(), "[Move] Sent to the queue");
        {
            let mut batch = self.batch.borrow_mut();
            batch.waiting.extend(moves.iter().map(|m| m.id));
        }
        let queue = self.queue.clone();
        self.runtime.spawn(async move {
            match queue.request(|reply| QueueCommand::MoveTargets { moves, reply }).await {
                Ok(Ok(taken)) => tracing::debug!(taken, "[Move] The queue took the moves"),
                Ok(Err(e)) => tracing::warn!(error = %e, "[Move] The queue refused the moves"),
                Err(_) => tracing::warn!("[Move] The queue has stopped; nothing moved"),
            }
        });
        self.close();
    }

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<Relocate<'_>>();
        let d = self.dialog.borrow();
        let Some(d) = d.as_ref() else { return };
        let n = d.jobs.len();
        ui.set_single(n == 1);
        ui.set_title(if n == 1 {
            "Move Target File".into()
        } else {
            format!("Move {n} Target Files").into()
        });
        ui.set_text(states_text(&d.jobs).into());
        let (field, path) = self.field(d);
        ui.set_folder_field(field);
        ui.set_path(path.into());
        if let [job] = d.jobs.as_slice() {
            ui.set_name(d.name.clone().unwrap_or_default().into());
            ui.set_extension(format!(".{}", job.extension).into());
        }
        let mut checks: Vec<RelocateCheckView> = d
            .plan
            .checks
            .iter()
            .map(|c| RelocateCheckView {
                level: match c.level {
                    Level::Info => 0,
                    Level::Warning => 2,
                    Level::Error => 3,
                },
                text: c.text.as_str().into(),
            })
            .collect();
        if let Some(warning) = self.rustview_warning(d) {
            checks.insert(
                0,
                RelocateCheckView {
                    level: 2,
                    text: warning.into(),
                },
            );
        }
        ui.set_checks(ModelRc::new(VecModel::from(checks)));
        ui.set_checking(d.plan.pending && d.folder != Folder::None);
        let conflicts = &d.plan.conflicts;
        ui.set_conflict_title(conflict_title(conflicts.len(), &d.folder).into());
        ui.set_conflicts(ModelRc::new(VecModel::from(
            conflicts
                .iter()
                .map(|c| RelocateConflictView {
                    file: c.file.as_str().into(),
                    numbered: c.numbered.as_str().into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_can_overwrite(conflicts.iter().any(|c| c.file_exists));
        ui.set_ready(d.plan.ready());
        ui.set_move_label(if d.plan.moves.len() > 1 {
            format!("Move {}", d.plan.moves.len()).into()
        } else {
            "Move".into()
        });
    }

    /// The field's view and the path text.
    fn field(&self, d: &Dialog) -> (crate::FolderFieldView, String) {
        let export = self.export.upgrade();
        match (&d.folder, d.entry, export) {
            (Folder::Source, _, _) => (
                crate::FolderFieldView {
                    label: rustcut_core::folders::SOURCE_NAME.into(),
                    tooltip: "Each job's file goes next to its source video".into(),
                    ..Default::default()
                },
                String::new(),
            ),
            (Folder::Path(path), Some(id), Some(export)) => {
                let (tree, _) = export.folder_choices();
                let label = tree.entry(id).map_or_else(String::new, |e| e.name().to_owned());
                (
                    self.select.entry_field(&tree, id, label, path.display().to_string()),
                    path.display().to_string(),
                )
            }
            (Folder::Path(path), _, _) => (
                FolderSelect::loose_field(path, rustcut_core::folders::default_name(path)),
                path.display().to_string(),
            ),
            (Folder::None, _, _) => (
                crate::FolderFieldView {
                    label: "Choose a folder".into(),
                    tooltip: "Pick one of your target folders, type a path or browse".into(),
                    ..Default::default()
                },
                String::new(),
            ),
        }
    }

    /// RustView cannot take the records now (they are moved by the user afterwards, user decision Q-P12f).
    fn rustview_warning(&self, d: &Dialog) -> Option<String> {
        let finished = d
            .jobs
            .iter()
            .any(|j| j.state != JobStateKind::Failed && j.state != JobStateKind::Canceled);
        if !finished {
            return None;
        }
        match &*self.connection.lock() {
            ConnectionState::Unavailable { reason, .. } => Some(format!(
                "RustView cannot be reached ({reason}): the files move, but their records stay; you get a list to move them in RustView"
            )),
            ConnectionState::Disabled => Some(
                "RustView is switched off in the settings: the records of these files stay at their old paths".into(),
            ),
            ConnectionState::Unknown | ConnectionState::Connected => None,
        }
    }

    fn set_folder(&self, folder: Folder, entry: Option<FolderId>) {
        if let Some(d) = self.dialog.borrow_mut().as_mut() {
            d.folder = folder;
            d.entry = entry;
        }
        self.changed();
    }

    fn open_list(&self) {
        let Some(export) = self.export.upgrade() else { return };
        let (tree, recent) = export.folder_choices();
        let current = self
            .dialog
            .borrow()
            .as_ref()
            .map_or(FolderId::SOURCE, |d| match (&d.folder, d.entry) {
                (_, Some(id)) => id,
                _ => FolderId::SOURCE,
            });
        let weak = Weak::clone(&self.me);
        let picked_tree = tree.clone();
        self.select.open(Session {
            owner: OWNER,
            tree,
            recent,
            current,
            job_target: None,
            on_pick: Rc::new(move |id| {
                if let Some(bridge) = weak.upgrade() {
                    bridge.picked(&picked_tree, id);
                }
            }),
        });
    }

    fn picked(&self, tree: &FolderTree, id: FolderId) {
        if id == FolderId::SOURCE {
            self.set_folder(Folder::Source, None);
            return;
        }
        if let Some(path) = tree.find(id).and_then(|n| n.path.clone()) {
            self.set_folder(Folder::Path(path), Some(id));
        } else {
            tracing::debug!(id = id.0, "[Move] A group has no folder");
        }
    }

    fn browse(&self) {
        let start = self.dialog.borrow().as_ref().and_then(|d| match &d.folder {
            Folder::Path(p) => Some(p.clone()),
            _ => None,
        });
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Move the Target Files Into");
        if let Some(start) = start.filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = self.window.upgrade() {
            dialog = dialog.set_parent(&slint::ComponentHandle::window(&window).window_handle());
        }
        let picked = dialog.pick_folder();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(folder) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                bridge.set_folder(Folder::Path(folder.path().to_path_buf()), None);
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Move] The event loop is gone; no folder dialog");
        }
    }

    /// The queue reported how the moves stand.
    fn progress_changed(&self) {
        let progress = self.progress.lock().clone();
        let cancel = |running: bool, waiting: bool| -> Option<Rc<dyn Fn()>> {
            let queue = self.queue.clone();
            Some(Rc::new(move || {
                if queue
                    .send(QueueCommand::CancelRelocations { running, waiting })
                    .is_err()
                {
                    tracing::warn!("[Move] The queue has stopped");
                }
            }))
        };
        if progress.is_running() {
            let mut text = format!(
                "Moving {} of {}",
                (progress.finished + 1).min(progress.total),
                progress.total
            );
            if progress.bytes_total > 0 {
                let pct = progress.bytes_done.saturating_mul(100) / progress.bytes_total.max(1);
                let _ = write!(text, " · {pct} %");
            }
            if self.moving_shown.replace(true) {
                activity::update(MOVING, text);
            } else {
                activity::begin(MOVING, text, cancel(true, false));
            }
        } else if self.moving_shown.replace(false) {
            activity::finish(MOVING);
        }
        let waiting = progress.waiting.len();
        if waiting > 0 {
            let text = if waiting == 1 {
                "1 move waits for its render".to_owned()
            } else {
                format!("{waiting} moves wait for their renders")
            };
            if self.waiting_shown.replace(true) {
                activity::update(WAITING, text);
            } else {
                activity::begin(WAITING, text, cancel(false, true));
            }
        } else if self.waiting_shown.replace(false) {
            activity::finish(WAITING);
        }
    }

    /// One job's move ended.
    fn outcome(&self, outcome: RelocationOutcome) {
        crate::app::redaction::learn_path(&outcome.to);
        if outcome.record_error.is_some() {
            self.records.borrow_mut().push(outcome.clone());
            self.show_records();
        }
        let in_batch = {
            let mut batch = self.batch.borrow_mut();
            // A rendering job reports twice: its target changed now, its file moves later (a result of its own).
            if batch.waiting.remove(&outcome.id) {
                batch.outcomes.push(outcome.clone());
                true
            } else {
                false
            }
        };
        if !in_batch {
            report(std::slice::from_ref(&outcome));
            return;
        }
        let done = {
            let mut batch = self.batch.borrow_mut();
            if batch.waiting.is_empty() {
                Some(std::mem::take(&mut batch.outcomes))
            } else {
                None
            }
        };
        if let Some(outcomes) = done {
            report(&outcomes);
        }
    }

    fn events_lost(&self) {
        let outcomes = {
            let mut batch = self.batch.borrow_mut();
            batch.waiting.clear();
            std::mem::take(&mut batch.outcomes)
        };
        if !outcomes.is_empty() {
            report(&outcomes);
        }
    }

    fn show_records(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<Relocate<'_>>();
        let records = self.records.borrow();
        let n = records.len();
        ui.set_records_title(if n == 1 {
            "1 RustView Record Stayed Behind".into()
        } else {
            format!("{n} RustView Records Stayed Behind").into()
        });
        ui.set_records(ModelRc::new(VecModel::from(
            records
                .iter()
                .map(|r| RelocateRecordView {
                    from: r.from.display().to_string().into(),
                    to: r.to.display().to_string().into(),
                    reason: r.record_error.clone().unwrap_or_default().into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_records_open(true);
    }

    fn copy_records(&self) {
        let mut text = String::new();
        for r in self.records.borrow().iter() {
            let _ = writeln!(text, "{}\t{}", r.from.display(), r.to.display());
        }
        if !clipboard::copy(text) {
            notices::post(Notice::new(Kind::ClipboardFailed, "The clipboard cannot be used"));
        }
    }
}

/// The result message of a batch (or of one later move).
fn report(outcomes: &[RelocationOutcome]) {
    let count = |want: fn(&Relocated) -> bool| {
        outcomes
            .iter()
            .filter(|o| o.result.as_ref().is_ok_and(want))
            .count()
    };
    let files = count(|r| matches!(r, Relocated::Renamed | Relocated::Copied));
    let targets = count(|r| matches!(r, Relocated::TargetChanged));
    let later = count(|r| matches!(r, Relocated::AfterRender));
    let canceled = count(|r| matches!(r, Relocated::Canceled));
    let failed: Vec<&RelocationOutcome> = outcomes.iter().filter(|o| o.result.is_err()).collect();
    let mut parts = Vec::new();
    if files > 0 {
        parts.push(format!(
            "Moved {}",
            if files == 1 {
                "1 file".into()
            } else {
                format!("{files} files")
            }
        ));
    }
    if targets > 0 {
        parts.push(format!(
            "{targets} {} changed",
            if targets == 1 { "target" } else { "targets" }
        ));
    }
    if later > 0 {
        parts.push(format!(
            "{later} after {}",
            if later == 1 { "its render" } else { "their renders" }
        ));
    }
    if canceled > 0 {
        parts.push(format!("{canceled} canceled"));
    }
    let folders: BTreeSet<&Path> = outcomes.iter().filter_map(|o| o.to.parent()).collect();
    let place = match folders.iter().next() {
        Some(folder) if folders.len() == 1 => format!("to {}", folder.display()),
        _ => String::new(),
    };
    if let Some(first) = failed.first() {
        let name = first
            .from
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let title = if failed.len() == 1 {
            format!("{name} was not moved")
        } else {
            format!("{} files were not moved", failed.len())
        };
        let mut detail = first.result.clone().err().unwrap_or_default();
        if !parts.is_empty() {
            detail = format!("{detail}; {}", parts.join(", "));
        }
        notices::post(
            Notice::new(Kind::TargetMoveFailed, title)
                .detail(detail)
                .activity(MOVING),
        );
    } else if !parts.is_empty() {
        let title = parts.remove(0);
        let mut detail = place;
        if !parts.is_empty() {
            if !detail.is_empty() {
                detail.push_str("; ");
            }
            detail.push_str(&parts.join(", "));
        }
        let level = if canceled > 0 && files + targets + later == 0 {
            NoticeLevel::Info
        } else {
            NoticeLevel::Success
        };
        notices::post(
            Notice::new(Kind::TargetsMoved, title)
                .detail(detail)
                .level(level)
                .activity(MOVING),
        );
    }
    activity::finish(MOVING);
}

fn job_of(row: &JobSummary) -> Job {
    let extension = row
        .target
        .extension()
        .map_or_else(|| "mkv".to_owned(), |e| e.to_string_lossy().into_owned());
    let stem = row
        .target
        .file_stem()
        .map_or_else(|| row.name.clone(), |s| s.to_string_lossy().into_owned());
    let state = row.state.kind();
    Job {
        id: row.id,
        stem,
        extension,
        state,
        target: row.target.clone(),
        source: row.source.clone(),
        moving: row.relocation.is_some() && !state.is_active(),
        unreadable: row.profile.is_empty(),
    }
}

/// "2 finished · 1 rendering".
fn states_text(jobs: &[Job]) -> String {
    let mut counts: Vec<(JobStateKind, usize)> = Vec::new();
    for job in jobs {
        match counts.iter_mut().find(|(k, _)| *k == job.state) {
            Some((_, n)) => *n += 1,
            None => counts.push((job.state, 1)),
        }
    }
    if let [(kind, 1)] = counts.as_slice()
        && let [job] = jobs
    {
        return format!("{}.{} · {}", job.stem, job.extension, kind.as_str());
    }
    counts
        .iter()
        .map(|(k, n)| format!("{n} {}", k.as_str()))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn conflict_title(n: usize, folder: &Folder) -> String {
    let place = match folder {
        Folder::Path(p) => format!(" in {}", p.display()),
        Folder::Source | Folder::None => String::new(),
    };
    if n == 1 {
        format!("1 name is taken{place}")
    } else {
        format!("{n} names are taken{place}")
    }
}

/// What the runtime checks.
struct Request {
    generation: u64,
    folders: Vec<PathBuf>,
    /// Finished jobs: (id, file, destination folder).
    files: Vec<(JobId, PathBuf, PathBuf)>,
}

/// The checks the current choice still needs (`None` = everything is known).
fn unknown(d: &Dialog) -> Option<Request> {
    let request = request_for(d, false);
    (!request.folders.is_empty() || !request.files.is_empty()).then_some(request)
}

/// The checks of the current choice; `all` = again, also what is known.
fn request_for(d: &Dialog, all: bool) -> Request {
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut files = Vec::new();
    for job in &d.jobs {
        let Some(dest) = plan::destination(job, &d.folder) else {
            continue;
        };
        if !dest.is_absolute() {
            continue;
        }
        if (all || !d.facts.scans.contains_key(&dest)) && !folders.contains(&dest) {
            folders.push(dest.clone());
        }
        if job.state == JobStateKind::Finished && (all || !d.facts.files.contains_key(&job.id)) {
            files.push((job.id, job.target.clone(), dest));
        }
    }
    Request {
        generation: d.generation,
        folders,
        files,
    }
}

/// Runs the checks (blocking, on the runtime's pool).
fn gather(request: Request) -> Facts {
    let mut facts = Facts::default();
    for folder in request.folders {
        let place = rustcut_core::files::place(&folder);
        if let Some(why) = place.unusable {
            facts.unusable.insert(folder.clone(), why);
        }
        if let Some(free) = place.free {
            facts.free.insert(folder.clone(), free);
        }
        facts
            .scans
            .insert(folder.clone(), rustcut_core::naming::scan_folder_now(&folder));
    }
    let mut sizes: HashMap<JobId, Option<(u64, bool)>> = HashMap::new();
    for (id, file, dest) in request.files {
        let found = std::fs::metadata(&file)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map(|meta| {
                let across = rustcut_core::files::needs_copy(&file, &dest).unwrap_or(false);
                (meta.len(), across)
            });
        sizes.insert(id, found);
    }
    facts.files = sizes;
    facts
}

fn connect(bridge: &Rc<RelocateBridge>, window: &MainWindow) {
    let ui = window.global::<Relocate<'_>>();
    let weak = Rc::downgrade(bridge);
    bridge.select.on_field(OWNER, move || {
        if let Some(b) = weak.upgrade() {
            b.open_list();
        }
    });
    let weak = Rc::downgrade(bridge);
    ui.on_path_committed(move |text| {
        let Some(b) = weak.upgrade() else { return false };
        let text = text.trim();
        let folder = if text.is_empty() {
            Folder::None
        } else {
            Folder::Path(PathBuf::from(shellexpand_home(text)))
        };
        b.set_folder(folder, None);
        true
    });
    ui.on_path_typed(|_| {});
    let weak = Rc::downgrade(bridge);
    ui.on_name_committed(move |text| {
        let Some(b) = weak.upgrade() else { return false };
        if let Some(d) = b.dialog.borrow_mut().as_mut() {
            d.name = Some(text.to_string());
        }
        b.changed();
        true
    });
    let weak = Rc::downgrade(bridge);
    ui.on_name_typed(move |text| {
        let Some(b) = weak.upgrade() else { return };
        if let Some(d) = b.dialog.borrow_mut().as_mut() {
            d.name = Some(text.to_string());
        }
        b.changed();
    });
    let weak = Rc::downgrade(bridge);
    ui.on_browse(move || {
        if let Some(b) = weak.upgrade() {
            b.browse();
        }
    });
    let weak = Rc::downgrade(bridge);
    ui.on_set_resolution(move |choice| {
        let Some(b) = weak.upgrade() else { return };
        if let Some(window) = b.window.upgrade() {
            window.global::<Relocate<'_>>().set_resolution(choice);
        }
        if let Some(d) = b.dialog.borrow_mut().as_mut() {
            d.resolution = match choice {
                1 => Resolution::Skip,
                2 => Resolution::Overwrite,
                _ => Resolution::AddNumber,
            };
        }
        b.changed();
    });
    let weak = Rc::downgrade(bridge);
    ui.on_confirm(move || {
        if let Some(b) = weak.upgrade() {
            b.confirm();
        }
    });
    let weak = Rc::downgrade(bridge);
    ui.on_cancel(move || {
        if let Some(b) = weak.upgrade() {
            tracing::info!("[Move] Dialog canceled; nothing moved");
            b.close();
        }
    });
    let weak = Rc::downgrade(bridge);
    ui.on_records_copy(move || {
        if let Some(b) = weak.upgrade() {
            b.copy_records();
        }
    });
    let weak = Rc::downgrade(bridge);
    ui.on_records_close(move || {
        if let Some(b) = weak.upgrade() {
            b.records.borrow_mut().clear();
            if let Some(window) = b.window.upgrade() {
                window.global::<Relocate<'_>>().set_records_open(false);
                window.invoke_focus_keys();
            }
        }
    });
}

/// `~/x` as the home folder's `x`.
fn shellexpand_home(text: &str) -> String {
    match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest).display().to_string(),
        _ => text.to_owned(),
    }
}

/// Follows the queue's move events: the progress coalesced, the outcomes in order.
async fn follow(service: QueueService, ui: UiDispatcher, progress: Arc<Mutex<RelocationProgress>>) {
    let mut events = service.events.subscribe();
    loop {
        match events.recv().await {
            Ok(QueueEvent::Relocation(now)) => {
                *progress.lock() = now;
                ui.post_latest(PROGRESS, rate::PROGRESS, || {
                    BRIDGE.with(|b| {
                        if let Some(bridge) = b.borrow().upgrade() {
                            bridge.progress_changed();
                        }
                    });
                });
            }
            Ok(QueueEvent::Relocated(outcome)) => {
                ui.post(move || {
                    BRIDGE.with(|b| {
                        if let Some(bridge) = b.borrow().upgrade() {
                            bridge.outcome(*outcome);
                        }
                    });
                });
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                // Outcomes may be lost: the batch reports what it has instead of waiting forever.
                tracing::warn!(missed = n, "[Move] Queue events were missed");
                ui.post(|| {
                    BRIDGE.with(|b| {
                        if let Some(bridge) = b.borrow().upgrade() {
                            bridge.events_lost();
                        }
                    });
                });
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Keeps RustView's connection state for the dialog's warning.
async fn follow_sync(mut events: tokio::sync::broadcast::Receiver<SyncEvent>, connection: Arc<Mutex<ConnectionState>>) {
    loop {
        match events.recv().await {
            Ok(SyncEvent::Connection(state)) => *connection.lock() = state,
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}
