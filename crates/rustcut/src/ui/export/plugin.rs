//! What plugins may do with the Job Finalization card (P12 block 8, Q-P12g): set the target folder unless the user
//! chose one for this video, with a mark at the folder field that names the plugin and its reason.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rustcut_core::folders::FolderId;

use super::ExportBridge;

/// Who set the target, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMark {
    /// The plugin's id (the mark leads to its Settings page).
    pub id: String,
    pub plugin: String,
    pub reason: String,
}

impl PluginMark {
    /// "Set by Auto-Folder (rule ~/Videos/Holiday)".
    pub fn tooltip(&self) -> String {
        if self.reason.trim().is_empty() {
            format!("Set by {}", self.plugin)
        } else {
            format!("Set by {} ({})", self.plugin, self.reason.trim())
        }
    }
}

/// What became of a plugin's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginTarget {
    /// It is the target now; the folder it names (to check on disk).
    Set(Option<PathBuf>),
    /// The user chose the target for this video.
    UserChose,
    /// No folder of the tree has this id (or it is a group).
    UnknownFolder(u32),
}

impl ExportBridge {
    /// A plugin's target: a folder of the tree by id, or any folder (a loose target, like a loaded job's).
    pub fn plugin_target(&self, folder: Option<u32>, path: Option<&Path>, mark: PluginMark) -> PluginTarget {
        if self.user_target.get() {
            return PluginTarget::UserChose;
        }
        {
            let mut st = self.state.borrow_mut();
            let source = st.source.clone();
            if let Some(id) = folder {
                // 0 is "Next to the source".
                let known = id == FolderId::SOURCE.0
                    || st
                        .target
                        .tree
                        .find(FolderId(id))
                        .is_some_and(|node| node.path.is_some());
                if !known {
                    return PluginTarget::UnknownFolder(id);
                }
                st.target.select_id(FolderId(id));
            } else if let Some(path) = path {
                st.target.load(path, source.as_deref());
            }
        }
        self.plugin_mark.replace(Some(mark));
        self.rescan();
        self.publish();
        let st = self.state.borrow();
        PluginTarget::Set(st.target.folder(st.source.as_deref()))
    }

    /// The target folder as a path (the plugins' context).
    pub fn target_folder(&self) -> Option<PathBuf> {
        let st = self.state.borrow();
        st.target.folder(st.source.as_deref())
    }

    /// The open video's path.
    pub fn source(&self) -> Option<PathBuf> {
        self.state.borrow().source.clone()
    }

    /// Every folder of the tree with a path, by id (a later plugin's context shows an earlier one's target).
    pub fn folder_paths(&self) -> BTreeMap<u32, PathBuf> {
        let st = self.state.borrow();
        st.target
            .tree
            .folders()
            .into_iter()
            .filter_map(|node| Some((node.id.0, node.path.clone()?)))
            .collect()
    }

    /// The plugin that set the target, if one did.
    pub fn target_plugin(&self) -> Option<String> {
        self.plugin_mark.borrow().as_ref().map(|mark| mark.id.clone())
    }

    /// The folder ids of the tree that are folders (not groups), for the plugin settings' checks.
    pub fn folder_ids(&self) -> Vec<u32> {
        self.folder_paths().into_keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_names_the_plugin_and_the_reason() {
        let mark = PluginMark {
            id: "auto-folder".into(),
            plugin: "Auto-Folder".into(),
            reason: "rule ~/Videos".into(),
        };
        assert_eq!(mark.tooltip(), "Set by Auto-Folder (rule ~/Videos)");
        let mark = PluginMark {
            id: "auto-folder".into(),
            plugin: "Auto-Folder".into(),
            reason: " ".into(),
        };
        assert_eq!(mark.tooltip(), "Set by Auto-Folder");
    }
}
