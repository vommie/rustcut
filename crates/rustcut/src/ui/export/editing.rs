//! The export while a job is loaded into the editor (M4.1, user decisions Q-Q3, Q-P8b): the job's target in the card.
//! Loading is always a copy: Save makes a new job, the loaded one stays as it is. The loaded job is told by the message
//! kind `job_loaded` (D-P9m), no longer by a strip in the card.

use std::path::Path;
use std::rc::Rc;

use rustcut_core::folders::Spot;
use rustcut_core::job::TargetSpec;
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

use super::state::JobFacts;
use super::{BRIDGE, ExportBridge};
use crate::Export;
use crate::ui::editing::JobEditor;
use crate::ui::notices::{self, Kind, Notice};

/// The key of the message that offers a loaded job's folder (a banner while it is the target).
pub(super) const FOLDER_UNKNOWN: &str = "export:folder-unknown";

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
        let loose = self.state.borrow_mut().load_target(target, &facts);
        if loose {
            self.offer_folder(&target.folder);
        }
        self.rescan();
    }

    /// The loaded job's folder is not in the tree: it is chosen as it is, and a message offers to add it (Q-FP2).
    fn offer_folder(&self, folder: &Path) {
        self.job_target_banner.set(true);
        tracing::info!(folder = %folder.display(), "[Export] The job's folder is not in the target folders");
        let weak = BRIDGE.with(|b| b.borrow().clone());
        let path = folder.to_path_buf();
        notices::post(
            Notice::new(Kind::FolderUnknown, "The job's folder is not in the target folders")
                .detail(format!(
                    "{} stays the target (\"Job target\") until you choose another folder or open another video.",
                    folder.display()
                ))
                .key(FOLDER_UNKNOWN)
                .action("Add to target folders", move || {
                    if let Some(bridge) = weak.upgrade() {
                        bridge.adopt_folder(&path);
                    }
                }),
        );
    }

    /// "Add to target folders": the folder joins the tree at the end of the top level and stays chosen.
    fn adopt_folder(&self, folder: &Path) {
        let mut added = false;
        let saved = self
            .writer
            .update(|s| added = s.export.folders.add_folder(folder, Spot::End).is_ok());
        if !saved {
            Self::tell(
                Notice::new(Kind::SettingsReadOnly, "The folder is not added")
                    .detail("config.toml is from a newer RustCut."),
            );
            return;
        }
        if added {
            tracing::info!(folder = %folder.display(), "[Export] The job's folder joined the target folders");
        }
    }

    /// A job was loaded or the link to it ended.
    pub fn editing_changed(&self) {
        self.publish();
    }
}
