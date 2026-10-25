//! Save (Ctrl+S / F9, `Action::Save`): the job is built from the editor, the filters, the working profile and the
//! source; the target folder is read once more on the runtime; a conflict opens the dialog (or, with its warning off,
//! takes a free name without asking); then the queue gets the job. Afterwards the counter goes up (auto-increment) and
//! the sections are cleared when the card says so.

use std::path::PathBuf;
use std::rc::Rc;

use rustcut_core::edit::Size;
use rustcut_core::job::{JOB_SPEC_VERSION, JobId, JobMetadata, JobSpec, SourceSummary, TargetSpec};
use rustcut_core::media::MediaInfo;
use rustcut_core::naming::{CheckInput, ExistingFile, check, existing_file, file_name, same_path, scan_folder};
use rustcut_core::queue::{QueueCommand, QueueError};
use slint::ComponentHandle as _;

use super::ExportBridge;
use super::dialog::{ConflictDialog, DialogInput, Outcome, silent_outcome};
use super::state::JobFacts;
use crate::ui::jump::JumpTarget;
use crate::ui::notices::{self, Kind, Notice};
use crate::{Export, MainWindow};

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

/// The open conflict dialog with its job (none while it only shows the files, `browse.rs`).
pub(super) struct Pending {
    pub(super) dialog: ConflictDialog,
    pub(super) draft: Option<Draft>,
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
                let mut notice = Notice::new(Kind::SaveRefused, "The job was not saved").detail(message);
                // No section yet (Q-N8 #16): the banner leads to the Sections card.
                let has_video = self.player.upgrade().is_some_and(|p| p.current_file().is_some());
                if has_video && self.editor.upgrade().is_some_and(|e| e.section_count() == 0) {
                    let target = JumpTarget::Sections;
                    notice = notice
                        .jump(target)
                        .action("Show the sections", move || crate::ui::jump::go(target));
                }
                Self::tell(notice);
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
                    Self::tell(
                        Notice::new(Kind::SaveFailed, "The job was not saved")
                            .detail(format!("The target folder could not be checked: {e}")),
                    );
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
        let sections = editor.sections_for_save(self.auto_section.get())?;
        let render = self.render.upgrade().ok_or_else(gone)?;
        let profile = render.working_profile();
        let filters = self.filters.upgrade().ok_or_else(gone)?.chain();
        let media = self.media.upgrade();
        let info = media.as_ref().and_then(|m| m.current());
        if let Some(problem) = super::job_check::job_problem(&profile, sections.len(), info.as_deref()) {
            return Err(problem.tooltip);
        }
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
        // The Tag & Rate panel's "Active" switch is off: RustView learns nothing of this job (user decision Q-P8b).
        let panel = self.tags.upgrade();
        let without_rustview = panel.as_ref().is_some_and(|t| !t.active());
        let (tags, rating) = if without_rustview {
            (Vec::new(), None)
        } else {
            panel.map(|t| t.draft_metadata()).unwrap_or_default()
        };
        let source_hash = source_hash.filter(|_| !without_rustview);
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
                without_rustview,
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
        let (base, source) = {
            let st = self.state.borrow();
            (st.name.clone(), st.source.clone())
        };
        let conflicts = check(&CheckInput {
            target: &target,
            base: &base,
            source: source.as_deref(),
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
                settings: &self.dialog_settings.borrow(),
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
                Self::tell(
                    Notice::new(Kind::SaveFailed, "The job was not saved")
                        .detail("The file exists and no free name was found."),
                );
                self.publish();
            }
            return;
        }
        tracing::info!(target = %target.display(), "[Export] The target is taken; asking");
        self.saving.set(false);
        *self.pending.borrow_mut() = Some(Pending {
            dialog,
            draft: Some(draft),
        });
        self.publish_dialog();
        self.start_previews();
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
        let without_rustview = spec.metadata.without_rustview;
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
                    // Saved without RustView: no late hash registration, and the panel's choice stays for later.
                    if !without_rustview {
                        if let Some(media) = bridge.media.upgrade() {
                            media.job_saved(&source, with_hash);
                        }
                        if let Some(panel) = bridge.tags.upgrade() {
                            panel.job_saved(&tags, rating);
                        }
                    }
                    bridge.saved(id, &file, counter);
                    // A new job made while another one was edited ends the editing (that job stays as it was).
                    let editing = bridge.editing.borrow().upgrade();
                    if let Some(editing) = editing.filter(|e| e.info().is_some()) {
                        editing.job_saved();
                    }
                }
                Err(e) => Self::tell(Notice::new(Kind::SaveFailed, "The job was not saved").detail(e.to_string())),
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
        notices::clear_kind(Kind::SaveRefused);
        notices::clear_kind(Kind::SaveFailed);
        // "Not saved" of an earlier attempt must not stay in the status line (found in the P8b audit).
        notices::clear_kind(Kind::SaveCanceled);
        Self::tell(Notice::new(Kind::JobQueued, format!("Job {id} queued")).detail(file));
    }

    pub(super) fn with_dialog(&self, change: impl FnOnce(&mut ConflictDialog)) {
        if let Some(pending) = self.pending.borrow_mut().as_mut() {
            change(&mut pending.dialog);
        }
        self.publish_dialog();
    }

    fn confirm(self: &Rc<Self>) {
        let Some(pending) = self.pending.borrow_mut().take() else {
            return;
        };
        let Some(draft) = pending.draft else {
            // Only looking at the files: the button closes.
            self.close_dialog();
            return;
        };
        let Some(outcome) = pending.dialog.outcome() else {
            *self.pending.borrow_mut() = Some(Pending {
                dialog: pending.dialog,
                draft: Some(draft),
            });
            return;
        };
        self.publish_dialog();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        self.enqueue(draft, outcome);
    }

    fn cancel(&self) {
        if let Some(pending) = self.pending.borrow_mut().take()
            && pending.draft.is_some()
        {
            Self::tell(Notice::new(Kind::SaveCanceled, "Not saved").detail("The conflict dialog was canceled."));
        }
        self.close_dialog();
    }

    fn close_dialog(&self) {
        self.resume_video();
        self.publish_dialog();
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
        self.publish();
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
    e.on_dialog_open_other(move || {
        if let Some(b) = weak.upgrade() {
            b.pending.borrow_mut().take();
            b.close_dialog();
            if let Some(window) = b.window.upgrade() {
                window.invoke_open_file();
            }
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_open_file(move |group, index, reveal| {
        if let Some(b) = weak.upgrade() {
            b.open_listed(group, index, reveal);
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_set_tab(move |index| {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| {
                if let Some(tab) = d.tabs().get(usize::try_from(index).unwrap_or(usize::MAX)) {
                    d.set_tab(*tab);
                }
            });
        }
    });
    let weak = Rc::downgrade(bridge);
    e.on_dialog_cycle_tab(move |delta| {
        if let Some(b) = weak.upgrade() {
            b.with_dialog(|d| d.cycle_tab(delta));
        }
    });
}
