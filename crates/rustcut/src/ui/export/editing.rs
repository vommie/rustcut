//! The export while a job is loaded into the editor (M4.1, user decisions Q-Q3, Q-P8b): the card's banner and the
//! job's target in the card. Loading is always a copy: Save makes a new job, the loaded one stays as it is.

use std::rc::Rc;

use rustcut_core::job::TargetSpec;
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

use super::ExportBridge;
use super::state::JobFacts;
use crate::Export;
use crate::ui::editing::JobEditor;

impl ExportBridge {
    /// Connects the loading of queued jobs.
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

    /// A job was loaded or the link to it ended.
    pub fn editing_changed(&self) {
        self.publish();
    }

    /// The banner's state (from `publish`).
    pub(super) fn publish_editing(&self, e: &Export<'_>) {
        let editing = self.editing.borrow().upgrade();
        let Some(info) = editing.as_ref().and_then(|ed| ed.info()) else {
            e.set_editing(false);
            return;
        };
        let banner = editing.and_then(|ed| ed.banner()).unwrap_or_default();
        e.set_editing(true);
        e.set_editing_tooltip(
            format!(
                "{banner}\nSaving makes a new job; job {} stays as it is. If the new job replaces it, remove or hold \
                 job {} in the queue yourself.",
                info.id, info.id
            )
            .into(),
        );
        e.set_editing_text(banner.into());
    }
}
