//! The Job Finalization card's target folder field (P11.4, user decision Q-FP2): the shared folder select
//! (`ui::folders::select`) with the card's tree, recent targets and target. Choosing only sets the folder; Save stays
//! separate.

use std::path::Path;
use std::rc::Rc;

use rustcut_core::folders::FolderId;
use slint::ComponentHandle as _;

use super::ExportBridge;
use super::target::Target;
use crate::Export;
use crate::ui::folders::select::list::JOB_TARGET;
use crate::ui::folders::select::{FolderSelect, Session};

/// The card's name as the owner of a folder field.
pub const OWNER: &str = "export";

impl ExportBridge {
    /// The field: the target's picture, name and path.
    pub(super) fn publish_field(&self, folder: Option<&Path>) {
        let Some(window) = self.window.upgrade() else { return };
        let st = self.state.borrow();
        let label = st.target.label(&st.target.target);
        let tooltip = folder.map_or_else(
            || "The folder of the source video".to_owned(),
            |f| f.display().to_string(),
        );
        let view = match &st.target.target {
            Target::Entry(id) => self.select.entry_field(&st.target.tree, *id, label, tooltip),
            Target::Loose(path) => {
                let mut view = FolderSelect::loose_field(path, label);
                view.tooltip = tooltip.into();
                view
            }
        };
        window.global::<Export<'_>>().set_folder_field(view);
    }

    /// The field was clicked: the list opens with the target highlighted.
    pub(super) fn open_list(&self) {
        let st = self.state.borrow();
        let current = if st.target.is_loose() {
            JOB_TARGET
        } else {
            st.target.saved_id()
        };
        let weak = std::rc::Weak::clone(&self.me);
        self.select.open(Session {
            owner: OWNER,
            tree: st.target.tree.clone(),
            recent: st.recent.clone(),
            current,
            job_target: st.target.loose().map(Path::to_path_buf),
            on_pick: Rc::new(move |id| {
                if let Some(bridge) = weak.upgrade() {
                    bridge.picked(id);
                }
            }),
        });
    }

    fn picked(&self, id: FolderId) {
        // "Job target" is the loaded job's folder: it stays.
        if id == JOB_TARGET {
            return;
        }
        tracing::info!(id = id.0, "[Export] Target folder chosen");
        self.choose_target(id);
    }

    /// The job's target folder, when it is an entry of the tree.
    pub fn current_target(&self) -> Option<FolderId> {
        match self.state.borrow().target.target {
            Target::Entry(id) => Some(id),
            Target::Loose(_) => None,
        }
    }

    /// Another target (the list, the folder wheel); the job is not saved.
    pub fn choose_target(&self, id: FolderId) {
        self.state.borrow_mut().target.select_id(id);
        self.rescan();
        self.publish();
    }

    /// The tree or the recent targets changed while the list is open.
    pub(super) fn refresh_list(&self) {
        let st = self.state.borrow();
        self.select
            .refresh(OWNER, &st.target.tree, &st.recent, st.target.loose());
    }

    /// The target folder tree and the recent targets (the move dialog offers the same folders).
    pub fn folder_choices(&self) -> (rustcut_core::folders::FolderTree, Vec<FolderId>) {
        let st = self.state.borrow();
        (st.target.tree.clone(), st.recent.clone())
    }

    /// The shared folder select (the move dialog uses it too).
    pub fn select(&self) -> Rc<FolderSelect> {
        Rc::clone(&self.select)
    }
}
