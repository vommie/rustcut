//! The export while a queued job is loaded into the editor (M4.1, user decisions Q-Q2, Q-Q3): the card's banner, the
//! job's target in the card, the card's state set aside and put back, and the save that updates the job
//! (`QueueCommand::UpdateSpec`) when it writes the same file, or makes a new job otherwise ("Save as new" always does).

use std::rc::Rc;

use rustcut_core::job::{JobSpec, TargetSpec};
use rustcut_core::naming::{file_name, same_path};
use rustcut_core::queue::{QueueCommand, QueueError};
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

use super::ExportBridge;
use super::save::Draft;
use super::state::{ExportDraft, JobFacts};
use crate::Export;
use crate::ui::editing::{JobEditor, SaveAs};
use crate::ui::notices::{self, Kind, Notice};

impl ExportBridge {
    /// Connects the editing of queued jobs.
    pub fn set_editing(&self, editing: &Rc<JobEditor>) {
        *self.editing.borrow_mut() = Rc::downgrade(editing);
        self.publish();
    }

    /// The Save key as the tooltips show it.
    pub fn save_key(&self) -> String {
        self.window
            .upgrade()
            .map(|w| w.global::<Export<'_>>().get_key_save().to_string())
            .unwrap_or_default()
    }

    /// The card's state as it is (a draft set aside while a job is edited).
    pub fn draft_state(&self) -> ExportDraft {
        self.state.borrow().draft()
    }

    /// Puts a draft's card back (after its video opened again).
    pub fn put_draft_state(&self, draft: ExportDraft) {
        self.state.borrow_mut().put_draft(draft);
        self.rescan();
    }

    /// The target of a job loaded into the editor (after its source opened and the other panels took the job).
    pub fn load_target(&self, target: &TargetSpec) {
        let facts = self.facts().unwrap_or_else(|| JobFacts {
            profile: String::new(),
            codec: "",
            container: rustcut_core::render::Container::default(),
            sections: 0,
            duration: Timestamp::ZERO,
        });
        let added = self.state.borrow_mut().load_target(target, &facts);
        if added {
            tracing::info!(folder = %target.folder.display(), "[Export] The job's folder joined the target folders");
            self.store_folders();
        }
        self.rescan();
    }

    /// The editing started or ended.
    pub fn editing_changed(&self) {
        self.publish();
    }

    /// "Save as new" of the banner.
    pub(super) fn save_as_new(self: &Rc<Self>) {
        self.save_with(true);
    }

    /// The banner's state (from `publish`).
    pub(super) fn publish_editing(&self, e: &Export<'_>, file: Option<&str>) {
        let editing = self.editing.borrow().upgrade();
        let Some(info) = editing.as_ref().and_then(|ed| ed.info()) else {
            e.set_editing(false);
            return;
        };
        let target = self
            .state
            .borrow()
            .target_folder()
            .zip(file)
            .map(|(folder, file)| folder.join(file));
        let updates = target
            .as_deref()
            .and_then(|t| editing.as_ref().and_then(|ed| ed.save_as(t, false)))
            == Some(SaveAs::Update(info.id));
        let banner = editing.and_then(|ed| ed.banner()).unwrap_or_default();
        let how = if updates {
            format!("Save updates job {} (it waits again afterwards)", info.id)
        } else if info.updatable {
            format!(
                "The file name differs from job {}'s: Save makes a new job, the job stays",
                info.id
            )
        } else {
            format!("Job {} is done: Save makes a new job from it", info.id)
        };
        e.set_editing(true);
        e.set_editing_tooltip(format!("{banner}\n{how}").into());
        e.set_editing_text(banner.into());
        e.set_editing_updates(updates);
        e.set_editing_changed(self.draft(false).is_ok_and(|d| !same_job(&d.spec, &info.spec)));
    }

    /// The save of an edited job that writes the same file: the job gets the new spec.
    pub(super) fn update_job(self: &Rc<Self>, draft: Draft) {
        let (Some(queue), Some(edit)) = (
            self.queue.upgrade(),
            self.editing.borrow().upgrade().and_then(|e| e.info()),
        ) else {
            return;
        };
        self.saving.set(true);
        self.publish();
        let mut spec = draft.spec;
        spec.target.overwrite = edit.overwrite;
        let file = file_name(&spec.target.stem, draft.facts.container);
        let handle = queue.handle().clone();
        let id = edit.id;
        let task = self.runtime.spawn(async move {
            handle
                .request(|reply| QueueCommand::UpdateSpec {
                    id,
                    spec: Box::new(spec),
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
                Ok(()) => {
                    notices::clear_kind(Kind::SaveFailed);
                    Self::tell(Notice::new(Kind::JobUpdated, format!("Job {id} updated")).detail(file));
                    if let Some(editing) = bridge.editing.borrow().upgrade() {
                        editing.job_saved();
                    }
                }
                Err(e) => Self::tell(
                    Notice::new(Kind::SaveFailed, format!("Job {id} was not updated"))
                        .detail(format!("{e}. \"Save as new\" keeps the changes as a new job.")),
                ),
            }
            bridge.rescan();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; the update is not confirmed");
        }
    }

    /// Whether a save of `target` updates the edited job.
    pub(super) fn updates(&self, target: &std::path::Path, force_new: bool) -> bool {
        self.editing
            .borrow()
            .upgrade()
            .and_then(|e| e.save_as(target, force_new))
            .is_some_and(|how| matches!(how, SaveAs::Update(_)))
    }
}

/// The same job apart from what the panels learn later (the probe's summary, the source hash).
fn same_job(a: &JobSpec, b: &JobSpec) -> bool {
    a.source == b.source
        && a.sections == b.sections
        && a.filters == b.filters
        && a.profile == b.profile
        && same_path(&a.target_path(), &b.target_path())
        && a.metadata.tags == b.metadata.tags
        && a.metadata.rating == b.metadata.rating
}
