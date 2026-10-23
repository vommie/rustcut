//! Save (Ctrl+S / F9, `Action::Save`): the job is built from the editor, the filters, the working profile and the
//! source; the target folder is read once more on the runtime; a conflict opens the dialog (or, with its warning off,
//! takes a free name without asking); then the queue gets the job. Afterwards the counter goes up (auto-increment) and
//! the sections are cleared when the card says so.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use humansize::{DECIMAL, format_size};
use rustcut_core::edit::Size;
use rustcut_core::job::{JOB_SPEC_VERSION, JobId, JobMetadata, JobSpec, SourceSummary, TargetSpec};
use rustcut_core::media::MediaInfo;
use rustcut_core::naming::{CheckInput, ExistingFile, check, existing_file, file_name, same_path, scan_folder};
use rustcut_core::queue::{QueueCommand, QueueError};
use rustcut_core::render::Severity;
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::ExportBridge;
use super::dialog::{ConflictDialog, DialogInput, Outcome, silent_outcome};
use super::state::JobFacts;
use crate::ui::queue::title as state_title;
use crate::ui::status::Level;
use crate::{ConflictJobView, ConflictOptionView, Export, MainWindow};

/// Similar names the dialog lists in its line (the tooltip has them all).
const SIMILAR_SHOWN: usize = 4;

/// A job waiting for its target: everything but the target's stem and the overwrite flag.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    pub(super) spec: JobSpec,
    folder: PathBuf,
    pub(super) facts: JobFacts,
    /// The stem the card shows.
    stem: String,
    /// "Save as new" while a job is edited: never an update.
    force_new: bool,
}

/// The open conflict dialog with its job.
pub(super) struct Pending {
    dialog: ConflictDialog,
    draft: Draft,
}

impl ExportBridge {
    /// Builds the job and checks its target; see the module documentation. While a queued job is edited, a save that
    /// writes the job's file updates it (`editing.rs`).
    pub fn save(self: &Rc<Self>) {
        self.save_with(false);
    }

    pub(super) fn save_with(self: &Rc<Self>, force_new: bool) {
        if self.saving.get() || self.pending.borrow().is_some() || self.meta.borrow().is_some() {
            return;
        }
        let draft = match self.draft(force_new) {
            Ok(draft) => draft,
            Err(message) => {
                self.status(Level::Warning, &message);
                return;
            }
        };
        if let Some(draft) = self.ask_metadata(draft) {
            self.check_target(draft);
        }
    }

    /// Reads the target folder on the runtime, then queues the job or asks about the conflict.
    pub(super) fn check_target(self: &Rc<Self>, draft: Draft) {
        self.saving.set(true);
        self.publish();
        let folder = draft.folder.clone();
        let target = draft.folder.join(file_name(&draft.stem, draft.facts.container));
        let task = self
            .runtime
            .spawn(async move { (scan_folder(&folder).await, existing_file(&target).await) });
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Some(bridge) = weak.upgrade() else { return };
            match task.await {
                Ok((scan, exists)) => bridge.decide(draft, scan, exists),
                Err(e) => {
                    bridge.saving.set(false);
                    bridge.status(Level::Error, &format!("The target folder could not be checked: {e}"));
                    bridge.publish();
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; not saved");
        }
    }

    /// The job as the panels describe it now, or why it cannot be saved.
    pub(super) fn draft(&self, force_new: bool) -> Result<Draft, String> {
        let gone = || "RustCut is shutting down.".to_owned();
        let player = self.player.upgrade().ok_or_else(gone)?;
        let source = player.current_file().ok_or("Open a video first.")?;
        let editor = self.editor.upgrade().ok_or_else(gone)?;
        let sections = editor.sections_for_save(self.auto_section)?;
        let render = self.render.upgrade().ok_or_else(gone)?;
        let profile = render.working_profile();
        if let Some(issue) = profile.validate().into_iter().find(|i| i.severity == Severity::Error) {
            return Err(format!("The render settings cannot be used: {}", issue.message));
        }
        let filters = self.filters.upgrade().ok_or_else(gone)?.chain();
        let media = self.media.upgrade();
        let info = media.as_ref().and_then(|m| m.current());
        // The Tag & Rate panel's tags and rating; the source hash goes to RustView's hash list. An edited job keeps
        // what the panels have not learned yet (the probe may still run).
        let edited = self
            .editing
            .borrow()
            .upgrade()
            .and_then(|e| e.info())
            .filter(|e| e.spec.source == source);
        let source_hash = media
            .as_ref()
            .and_then(|m| m.source_hash(&source))
            .or_else(|| edited.as_ref().and_then(|e| e.spec.metadata.source_hash.clone()));
        let source_info = info
            .as_deref()
            .and_then(source_summary)
            .or_else(|| edited.as_ref().and_then(|e| e.spec.source_info));
        let (tags, rating) = self.tags.upgrade().map(|t| t.draft_metadata()).unwrap_or_default();
        let facts = JobFacts::new(&profile, sections.len(), sections.total_duration());
        let st = self.state.borrow();
        let folder = st.target_folder().ok_or("Choose a target folder.")?;
        let (stem, file) = st
            .file(&facts)
            .map_err(|e| format!("The file name cannot be used: {e}"))?;
        if same_path(&source, &folder.join(&file)) {
            return Err("The target is the source file: choose another name or folder.".to_owned());
        }
        let spec = JobSpec {
            version: JOB_SPEC_VERSION,
            source_info,
            source,
            sections,
            filters,
            profile,
            target: TargetSpec {
                folder: folder.clone(),
                stem: stem.clone(),
                overwrite: false,
                naming: st.naming(st.active_counter()),
            },
            metadata: JobMetadata {
                tags,
                rating,
                source_hash,
            },
        };
        Ok(Draft {
            spec,
            folder,
            facts,
            stem,
            force_new,
        })
    }

    /// The folder was read: queue the job, or ask how to get around the conflict.
    fn decide(self: &Rc<Self>, draft: Draft, scan: rustcut_core::naming::FolderScan, exists: Option<ExistingFile>) {
        let warnings = self.warnings.borrow().clone();
        let target = draft.folder.join(file_name(&draft.stem, draft.facts.container));
        // An update of the edited job: the job itself is no conflict, and its own choice about an existing file stays.
        let edit = self.editing.borrow().upgrade().and_then(|e| e.info());
        let update = self.updates(&target, draft.force_new);
        if update && (exists.is_none() || edit.as_ref().is_some_and(|e| e.overwrite)) {
            self.saving.set(false);
            self.update_job(draft);
            return;
        }
        let mut queue = self.queue.upgrade().map(|q| q.targets()).unwrap_or_default();
        if update && let Some(edit) = &edit {
            queue.retain(|q| q.id != edit.id);
        }
        let base = self.state.borrow().name.clone();
        let conflicts = check(&CheckInput {
            target: &target,
            base: &base,
            scan: &scan,
            exists,
            queue: &queue,
            warnings: &warnings,
        });
        let silent = exists.is_some() && !warnings.target_exists;
        if !silent && !conflicts.needs_decision() {
            let outcome = Outcome {
                stem: draft.stem.clone(),
                counter: None,
                overwrite: false,
                remove: Vec::new(),
            };
            self.enqueue(draft, outcome);
            return;
        }
        if update {
            tracing::info!("[Export] The edited job's file exists: asking");
        }
        // The dialog shows the existing file even when its warning is off (the silent way needs its choices).
        let mut shown = conflicts;
        shown.exists = exists;
        let dialog = {
            let st = self.state.borrow();
            ConflictDialog::new(DialogInput {
                folder: &draft.folder,
                stem: &draft.stem,
                conflicts: shown,
                scan,
                queue,
                export: &st,
                facts: &draft.facts,
            })
        };
        if silent {
            let st = self.state.borrow();
            let outcome = silent_outcome(&dialog, &st);
            drop(st);
            if let Some(outcome) = outcome {
                tracing::info!(stem = %outcome.stem, "[Export] The target exists; a free name was taken without asking");
                self.enqueue(draft, outcome);
            } else {
                self.saving.set(false);
                self.status(Level::Error, "The file exists and no free name was found.");
                self.publish();
            }
            return;
        }
        tracing::info!(target = %target.display(), "[Export] The target is taken; asking");
        self.saving.set(false);
        *self.pending.borrow_mut() = Some(Pending { dialog, draft });
        self.publish_dialog();
        self.publish();
    }

    /// Hands the job to the queue (after removing the jobs the dialog chose to remove); an edited job writing its own
    /// file is updated instead.
    fn enqueue(self: &Rc<Self>, draft: Draft, outcome: Outcome) {
        let target = draft.folder.join(file_name(&outcome.stem, draft.facts.container));
        if outcome.remove.is_empty() && self.updates(&target, draft.force_new) {
            let mut draft = draft;
            draft.spec.target.stem.clone_from(&outcome.stem);
            if outcome.overwrite {
                draft.spec.target.overwrite = true;
            }
            self.update_job(draft);
            return;
        }
        let Some(queue) = self.queue.upgrade() else { return };
        self.saving.set(true);
        self.publish();
        let mut spec = draft.spec;
        spec.target.stem.clone_from(&outcome.stem);
        spec.target.overwrite = outcome.overwrite;
        if let (Some(n), Some(naming)) = (outcome.counter, spec.target.naming.as_mut()) {
            naming.counter = Some(n);
        }
        let file = file_name(&outcome.stem, draft.facts.container);
        let counter = outcome.counter;
        let source = spec.source.clone();
        let with_hash = spec.metadata.source_hash.is_some();
        let (tags, rating) = (spec.metadata.tags.clone(), spec.metadata.rating);
        let handle = queue.handle().clone();
        let remove = outcome.remove;
        let task = self.runtime.spawn(async move {
            if !remove.is_empty() {
                let removed = handle
                    .request(|reply| QueueCommand::Remove { ids: remove, reply })
                    .await
                    .map_err(|_gone| QueueError::Store("the queue has stopped".into()))?;
                removed?;
            }
            handle
                .request(|reply| QueueCommand::Enqueue {
                    spec: Box::new(spec),
                    hold: false,
                    reply,
                })
                .await
                .map_err(|_gone| QueueError::Store("the queue has stopped".into()))?
        });
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Some(bridge) = weak.upgrade() else { return };
            let result = task.await.unwrap_or_else(|e| Err(QueueError::Store(e.to_string())));
            bridge.saving.set(false);
            match result {
                Ok(id) => {
                    if let Some(media) = bridge.media.upgrade() {
                        media.job_saved(&source, with_hash);
                    }
                    if let Some(panel) = bridge.tags.upgrade() {
                        panel.job_saved(&tags, rating);
                    }
                    bridge.saved(id, &file, counter);
                    // A new job made while another one was edited ends the editing (that job stays as it was).
                    let editing = bridge.editing.borrow().upgrade();
                    if let Some(editing) = editing.filter(|e| e.info().is_some()) {
                        editing.job_saved();
                    }
                }
                Err(e) => bridge.status(Level::Error, &format!("The job was not saved: {e}")),
            }
            bridge.rescan();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; the save is not confirmed");
        }
    }

    /// The queue took the job.
    fn saved(&self, id: JobId, file: &str, counter: Option<u32>) {
        let clear = {
            let mut st = self.state.borrow_mut();
            st.saved(counter);
            st.clear_sections
        };
        if clear && let Some(editor) = self.editor.upgrade() {
            editor.clear_sections();
        }
        self.status(Level::Success, &format!("Job {id} queued: {file}"));
    }

    /// Shows the dialog's state (or closes it).
    pub(super) fn publish_dialog(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let e = window.global::<Export<'_>>();
        let pending = self.pending.borrow();
        let Some(Pending { dialog, .. }) = pending.as_ref() else {
            e.set_dialog_open(false);
            return;
        };
        let c = &dialog.conflicts;
        let title = match (c.exists.is_some(), c.queued.is_empty()) {
            (true, false) => "The file exists, and queued jobs write it too",
            (true, true) => "The file exists already",
            _ => "Queued jobs write the same file",
        };
        e.set_dialog_title(title.into());
        e.set_dialog_file(dialog.file.as_str().into());
        e.set_dialog_folder(dialog.folder.display().to_string().into());
        e.set_dialog_exists(c.exists.map(existing_text).unwrap_or_default().into());
        e.set_dialog_jobs_title(
            match c.queued.len() {
                1 => "1 job in the queue writes this file".to_owned(),
                n => format!("{n} jobs in the queue write this file"),
            }
            .into(),
        );
        let jobs: Vec<ConflictJobView> = c
            .queued
            .iter()
            .map(|q| ConflictJobView {
                id: q.id.to_string().into(),
                name: q
                    .target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into(),
                state: state_title(q.state).into(),
                running: q.state.is_active(),
            })
            .collect();
        e.set_dialog_jobs(ModelRc::new(VecModel::from(jobs)));
        let more = c.similar.len().saturating_sub(SIMILAR_SHOWN);
        let mut similar = c
            .similar
            .iter()
            .take(SIMILAR_SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if more > 0 {
            let _ = write!(similar, " and {more} more");
        }
        e.set_dialog_similar(similar.into());
        e.set_dialog_similar_tooltip(c.similar.join("\n").into());
        let options: Vec<ConflictOptionView> = dialog
            .options
            .iter()
            .map(|o| ConflictOptionView {
                title: o.title.as_str().into(),
                file: o.file.as_str().into(),
                detail: o.detail.as_str().into(),
                enabled: o.enabled,
                destructive: o.destructive,
                typed: o.choice == super::Choice::Rename,
            })
            .collect();
        e.set_dialog_options(ModelRc::new(VecModel::from(options)));
        e.set_dialog_selected(i32::try_from(dialog.selected).unwrap_or_default());
        e.set_dialog_typed(dialog.typed.as_str().into());
        e.set_dialog_dont_ask_text(dialog.dont_ask_text().into());
        e.set_dialog_dont_ask(dialog.dont_ask);
        e.set_dialog_can_confirm(dialog.outcome().is_some());
        e.set_dialog_confirm_text(dialog.confirm_text().into());
        e.set_dialog_open(true);
    }

    fn with_dialog(&self, change: impl FnOnce(&mut ConflictDialog)) {
        if let Some(pending) = self.pending.borrow_mut().as_mut() {
            change(&mut pending.dialog);
        }
        self.publish_dialog();
    }

    fn confirm(self: &Rc<Self>) {
        let Some(pending) = self.pending.borrow_mut().take() else {
            return;
        };
        let Some(outcome) = pending.dialog.outcome() else {
            *self.pending.borrow_mut() = Some(pending);
            return;
        };
        if pending.dialog.dont_ask {
            let exists = pending.dialog.conflicts.exists.is_some();
            {
                let mut warnings = self.warnings.borrow_mut();
                if exists {
                    warnings.target_exists = false;
                } else {
                    warnings.target_queued = false;
                }
            }
            let warnings = self.warnings.borrow().clone();
            if !self.writer.update(|s| s.warnings = warnings) {
                self.status(
                    Level::Warning,
                    "config.toml is from a newer RustCut: the choice is kept for this run only.",
                );
            }
        }
        self.publish_dialog();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        self.enqueue(pending.draft, outcome);
    }

    fn cancel(&self) {
        if self.pending.borrow_mut().take().is_some() {
            self.status(Level::Info, "Not saved: the conflict dialog was canceled.");
        }
        self.publish_dialog();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        self.publish();
    }

    /// The existing target: played in the default player, or shown in the file manager (off the UI thread).
    fn open_existing(&self, reveal: bool) {
        let Some(path) = self
            .pending
            .borrow()
            .as_ref()
            .map(|p| p.dialog.folder.join(&p.dialog.file))
        else {
            return;
        };
        self.runtime.spawn_blocking(move || {
            let result = if reveal {
                opener::reveal(&path)
            } else {
                opener::open(&path)
            };
            if let Err(e) = result {
                tracing::warn!(file = %path.display(), error = %e, "[Export] The existing file could not be opened");
            }
        });
    }
}

/// "245 MB · modified 2026-10-22 14:05".
fn existing_text(file: ExistingFile) -> String {
    let size = format_size(file.bytes, DECIMAL);
    match file.modified {
        Some(time) => {
            let local: chrono::DateTime<chrono::Local> = time.into();
            format!("{size} · modified {}", local.format("%Y-%m-%d %H:%M"))
        }
        None => size,
    }
}

/// What the probe found about the source, for the job's record.
fn source_summary(info: &MediaInfo) -> Option<SourceSummary> {
    let (_, video) = info.primary_video()?;
    let (width, height) = video.oriented_size()?;
    Some(SourceSummary {
        size: Size::new(width, height),
        frame_rate: video.avg_frame_rate.or(video.frame_rate),
        duration: info.duration()?,
        audio_tracks: u32::try_from(info.audio_streams().count()).unwrap_or(u32::MAX),
    })
}

pub(super) fn connect_dialog(bridge: &Rc<ExportBridge>, window: &MainWindow) {
    let e = window.global::<Export<'_>>();
    let weak = Rc::downgrade(bridge);
    e.on_dialog_select(move |index| {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| d.select(usize::try_from(index).unwrap_or(usize::MAX)));
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_step(move |delta| {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| d.step(delta));
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_type(move |text| {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| d.type_name(&text));
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_toggle_dont_ask(move || {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| d.dont_ask = !d.dont_ask);
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_confirm(move || {
        if let Some(b) = weak.upgrade() {
            b.confirm();
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_cancel(move || {
        if let Some(b) = weak.upgrade() {
            b.cancel();
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_play_existing(move || {
        if let Some(b) = weak.upgrade() {
            b.open_existing(false);
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_reveal(move || {
        if let Some(b) = weak.upgrade() {
            b.open_existing(true);
        }
    });
}
