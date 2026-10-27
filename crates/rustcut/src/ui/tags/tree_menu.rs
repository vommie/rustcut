//! The tag tree's context menu on its free space: RustView's "Tags Root" menu without "Add Root Tag..." (RustCut never
//! changes RustView's tags): Expand All, Collapse All, Refresh Tag Tree, and the switch between Normal Mode and Compact
//! Mode (`rustview.compact_tree`, the same setting as in Settings > RustView). The refresh asks RustView for its tree
//! now (`SyncCommand::RefreshTree`); the panel changes only when the tree differs, in place, so the choice, the locked
//! tags and the expanded branches of tags that still exist stay.

use std::rc::Rc;

use rustcut_core::rustview::{RustViewError, SyncCommand, TagTree};

use super::{TagPanel, TagsBridge, with_bridge};
use crate::icons::Icon;
use crate::ui::menu::{ContextMenuBridge, MenuAction, MenuItem};
use crate::ui::notices::{Kind, Notice};

/// An entry of the tree's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeAction {
    ExpandAll,
    CollapseAll,
    Refresh,
    /// Switch Compact Mode on (`true`) or back to Normal Mode.
    Compact(bool),
}

impl TreeAction {
    const ALL: [Self; 5] = [
        Self::ExpandAll,
        Self::CollapseAll,
        Self::Refresh,
        Self::Compact(true),
        Self::Compact(false),
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::ExpandAll => "tags.expand_all",
            Self::CollapseAll => "tags.collapse_all",
            Self::Refresh => "tags.refresh",
            Self::Compact(true) => "tags.compact_mode",
            Self::Compact(false) => "tags.normal_mode",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// The menu while the tree is in Compact Mode (`compact`) or not; Expand and Collapse need a tag with children.
pub fn tree_menu(compact: bool, has_branches: bool) -> Vec<MenuItem> {
    let action = |a: TreeAction, title: &str, icon: Icon| MenuItem::Action(MenuAction::new(a.id(), title, Some(icon)));
    let branch = |a: TreeAction, title: &str, icon: Icon| {
        MenuItem::Action(MenuAction::new(a.id(), title, Some(icon)).enabled(has_branches))
    };
    vec![
        MenuItem::Heading("Tags Root".into()),
        branch(TreeAction::ExpandAll, "Expand All", Icon::TreeExpandAll),
        branch(TreeAction::CollapseAll, "Collapse All", Icon::TreeCollapseAll),
        MenuItem::Separator,
        action(TreeAction::Refresh, "Refresh Tag Tree", Icon::TreeRefresh),
        MenuItem::Separator,
        if compact {
            action(TreeAction::Compact(false), "Normal Mode", Icon::TreeNormal)
        } else {
            action(TreeAction::Compact(true), "Compact Mode", Icon::TreeCompact)
        },
    ]
}

impl TagPanel {
    /// Every tag expanded (RustView's `expand_all`).
    pub fn expand_all(&mut self) {
        let ids: Vec<u32> = self.tags().map(|t| t.id).collect();
        self.expanded.extend(ids);
    }

    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    /// Whether some tag has children (Expand All and Collapse All change something).
    pub fn has_branches(&self) -> bool {
        self.tags().any(|t| t.parent_id.is_some())
    }

    /// Whether `tree` holds the tags the panel shows (a refresh that changed nothing).
    pub fn same_tree(&self, tree: &TagTree) -> bool {
        self.has_tree() && self.tags().eq(tree.tags.iter())
    }
}

impl TagsBridge {
    pub fn set_menu(&self, menu: &Rc<ContextMenuBridge>) {
        *self.menu.borrow_mut() = Rc::downgrade(menu);
    }

    /// `rustview.compact_tree` (at the start and whenever the setting changes).
    pub fn set_compact(&self, on: bool) {
        if self.compact.replace(on) != on {
            tracing::info!(compact = on, "[Tags] Tree mode");
            self.publish();
        }
    }

    pub(super) fn compact(&self) -> bool {
        self.compact.get()
    }

    /// A right click on the tree's free space (window coordinates).
    pub(super) fn show_tree_menu(self: &Rc<Self>, x: f32, y: f32) {
        let Some(menu) = self.menu.borrow().upgrade() else {
            return;
        };
        let items = tree_menu(self.compact.get(), self.state.borrow().has_branches());
        let weak = Rc::downgrade(self);
        menu.show(&items, x, y, move |id| {
            if let (Some(b), Some(action)) = (weak.upgrade(), TreeAction::from_id(id)) {
                b.run_tree_action(action);
            }
        });
    }

    fn run_tree_action(&self, action: TreeAction) {
        match action {
            TreeAction::ExpandAll => self.change(TagPanel::expand_all),
            TreeAction::CollapseAll => self.change(TagPanel::collapse_all),
            TreeAction::Refresh => self.refresh_tree(),
            TreeAction::Compact(on) => {
                // The setting is the one place; `app::live` hands the change back through `set_compact`.
                if !self.writer.update(|s| s.rustview.compact_tree = on) {
                    Self::tell(
                        Notice::new(Kind::SettingsReadOnly, "Not saved")
                            .detail("config.toml is from a newer RustCut: the choice is kept for this run only."),
                    );
                }
            }
        }
    }

    /// Asks RustView for its tag tree now.
    pub(super) fn refresh_tree(&self) {
        tracing::info!("[Tags] Refresh Tag Tree: RustView is asked now");
        let handle = self.sync.handle.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = handle
                .request(|reply| SyncCommand::RefreshTree { reply })
                .await
                .unwrap_or_else(|_gone| Err(RustViewError::Unavailable("the RustView sync has stopped".into())));
            with_bridge(&ui, move |b| b.refreshed(answer));
        });
    }

    fn refreshed(&self, answer: Result<std::sync::Arc<TagTree>, RustViewError>) {
        match answer {
            Ok(tree) if self.state.borrow().same_tree(&tree) => {
                tracing::info!(
                    tags = tree.tags.len(),
                    "[Tags] Refresh Tag Tree: the panel shows this tree already"
                );
            }
            Ok(tree) => self.tree_answer(Ok(tree)),
            Err(e) => {
                Self::tell(Notice::new(Kind::TagsFailed, "The tag tree was not refreshed").detail(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rustcut_core::rustview::TagItem;
    use rustcut_core::settings::TagPanelState;

    use super::*;

    fn titles(items: &[MenuItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                MenuItem::Action(a) => format!("{}{}", a.title, if a.enabled { "" } else { " (off)" }),
                MenuItem::Separator => "-".into(),
                MenuItem::Heading(h) => format!("[{h}]"),
                MenuItem::Submenu { title, .. } => format!("{title} >"),
            })
            .collect()
    }

    fn tree(tags: &[(u32, Option<u32>)]) -> Arc<TagTree> {
        let tags: Vec<TagItem> = tags
            .iter()
            .map(|&(id, parent_id)| TagItem {
                id,
                parent_id,
                name: format!("t{id}"),
            })
            .collect();
        Arc::new(TagTree {
            success: true,
            total_tags: tags.len(),
            tags,
        })
    }

    #[test]
    fn the_menu_is_rustviews_without_add_root_tag_plus_the_mode_switch() {
        assert_eq!(
            titles(&tree_menu(false, true)),
            [
                "[Tags Root]",
                "Expand All",
                "Collapse All",
                "-",
                "Refresh Tag Tree",
                "-",
                "Compact Mode"
            ]
        );
        let compact = titles(&tree_menu(true, false));
        assert_eq!(compact.first().map(String::as_str), Some("[Tags Root]"));
        assert_eq!(compact.get(1).map(String::as_str), Some("Expand All (off)"));
        assert_eq!(compact.last().map(String::as_str), Some("Normal Mode"));
        for action in TreeAction::ALL {
            assert_eq!(TreeAction::from_id(action.id()), Some(action));
        }
        assert_eq!(TreeAction::from_id("tags.add_root"), None);
    }

    #[test]
    fn expand_and_collapse_all_and_an_unchanged_tree() {
        let mut p = TagPanel::new(&TagPanelState::default());
        let first = tree(&[(1, None), (2, Some(1)), (3, Some(2)), (4, None)]);
        assert!(!p.same_tree(&first), "no tree yet");
        p.set_tree(Arc::clone(&first));
        assert!(p.has_branches());
        p.expand_all();
        assert_eq!(p.rows().len(), 4, "every branch open");
        p.collapse_all();
        assert_eq!(p.rows().len(), 2, "only the roots");

        p.toggle_expanded(1);
        p.toggle_expanded(2);
        assert!(p.same_tree(&tree(&[(1, None), (2, Some(1)), (3, Some(2)), (4, None)])));
        // A refreshed tree without tag 2 keeps the expansion of the tags that still exist.
        p.set_tree(tree(&[(1, None), (3, Some(1)), (4, None)]));
        assert_eq!(p.expanded, [1].into_iter().collect());
        assert!(!p.same_tree(&first));
    }
}
