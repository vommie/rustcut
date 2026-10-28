//! The Job Finalization card's target (P11.1, user decision Q-FP2): an entry of the folder tree by id ("Next to the
//! source" = id 0), or a loose folder: the folder of a loaded job that the tree does not have. A loose folder is
//! offered to the tree, never added silently, and shows as the temporary entry "Job target" (the user's answer,
//! 2026-10-28): saving without another choice keeps the job's folder; another video gives the choice from before
//! back. It is never saved as the card's choice.

use std::path::{Path, PathBuf};

use rustcut_core::folders::{FolderId, FolderTree, SOURCE_NAME, Spot, group_path};
use rustcut_core::naming::same_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Entry(FolderId),
    Loose(PathBuf),
}

impl Default for Target {
    fn default() -> Self {
        Self::Entry(FolderId::SOURCE)
    }
}

/// The tree and the chosen target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetChoice {
    pub tree: FolderTree,
    pub target: Target,
    /// The choice before a loaded job's folder became the target (it comes back with another video).
    previous: FolderId,
}

/// The temporary entry of a loaded job's folder.
pub const JOB_TARGET: &str = "Job target";

/// Where a loaded job's folder was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Source,
    Tree,
    /// Not in the tree: chosen as a loose folder (to be offered).
    Loose,
}

impl TargetChoice {
    /// The tree and the choice `id` (an entry that is gone or a group falls back to the source).
    pub fn new(tree: &FolderTree, id: FolderId) -> Self {
        let mut choice = Self {
            tree: tree.clone(),
            target: Target::Entry(id),
            previous: FolderId::SOURCE,
        };
        choice.repair();
        choice
    }

    /// An entry that is no folder any more becomes the source; a loose folder the tree now has becomes its entry.
    fn repair(&mut self) {
        match &self.target {
            Target::Entry(id) if id.is_source() => {}
            Target::Entry(id) => {
                if self
                    .tree
                    .find(*id)
                    .is_none_or(rustcut_core::folders::FolderNode::is_group)
                {
                    self.target = Target::default();
                }
            }
            Target::Loose(path) => {
                if let Some(node) = self
                    .tree
                    .folders()
                    .into_iter()
                    .find(|n| n.path.as_deref().is_some_and(|p| same_path(p, path)))
                {
                    self.target = Target::Entry(node.id);
                }
            }
        }
    }

    /// The tree changed (Settings): the choice stays on its entry while it is a folder.
    pub fn set_tree(&mut self, tree: &FolderTree) {
        tree.clone_into(&mut self.tree);
        self.repair();
    }

    /// The id the card keeps in `ui-state.toml` (a loose folder is not kept).
    pub fn saved_id(&self) -> FolderId {
        match self.target {
            Target::Entry(id) => id,
            Target::Loose(_) => FolderId::SOURCE,
        }
    }

    /// The folder a job writes into, with the source video `source`.
    pub fn folder(&self, source: Option<&Path>) -> Option<PathBuf> {
        match &self.target {
            Target::Entry(id) if id.is_source() => source.and_then(Path::parent).map(Path::to_path_buf),
            Target::Entry(id) => self.tree.find(*id).and_then(|n| n.path.clone()),
            Target::Loose(path) => Some(path.clone()),
        }
    }

    /// The choices of the card's list in order: the source, every folder of the tree, the loose folder.
    pub fn choices(&self) -> Vec<Target> {
        std::iter::once(Target::default())
            .chain(self.tree.folders().into_iter().map(|n| Target::Entry(n.id)))
            .chain(matches!(self.target, Target::Loose(_)).then(|| self.target.clone()))
            .collect()
    }

    /// A choice's text: the name, after its groups ("Clients › North").
    pub fn label(&self, target: &Target) -> String {
        match target {
            Target::Entry(id) if id.is_source() => SOURCE_NAME.to_owned(),
            Target::Entry(id) => {
                let name = self.tree.find(*id).map_or("", |n| n.name.as_str());
                let groups = group_path(&self.tree, *id);
                if groups.is_empty() {
                    name.to_owned()
                } else {
                    format!("{groups} › {name}")
                }
            }
            Target::Loose(_) => JOB_TARGET.to_owned(),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        self.choices().iter().map(|t| self.label(t)).collect()
    }

    /// The list position of the choice.
    pub fn index(&self) -> usize {
        self.choices().iter().position(|t| *t == self.target).unwrap_or(0)
    }

    pub fn select(&mut self, index: usize) {
        if let Some(target) = self.choices().into_iter().nth(index) {
            self.target = target;
        }
    }

    pub fn select_id(&mut self, id: FolderId) {
        self.target = Target::Entry(id);
        self.repair();
    }

    /// A loaded job's folder: the source's folder, a folder of the tree, or else a loose folder.
    pub fn load(&mut self, folder: &Path, source: Option<&Path>) -> Found {
        if source.and_then(Path::parent).is_some_and(|f| same_path(f, folder)) {
            self.target = Target::default();
            return Found::Source;
        }
        if let Target::Entry(id) = self.target {
            self.previous = id;
        }
        self.target = Target::Loose(folder.to_path_buf());
        self.repair();
        if matches!(self.target, Target::Loose(_)) {
            Found::Loose
        } else {
            Found::Tree
        }
    }

    pub const fn is_loose(&self) -> bool {
        matches!(self.target, Target::Loose(_))
    }

    /// The loaded job's folder, while it is the target.
    pub fn loose(&self) -> Option<&Path> {
        match &self.target {
            Target::Loose(path) => Some(path),
            Target::Entry(_) => None,
        }
    }

    /// Another video opened: a loaded job's folder gives way to the choice from before.
    pub fn other_video(&mut self) {
        if self.is_loose() {
            self.target = Target::Entry(self.previous);
            self.repair();
        }
    }

    /// The loose folder joins the tree (at the end of the top level) and stays chosen; `None` when there is none.
    pub fn adopt(&mut self) -> Option<FolderId> {
        let Target::Loose(path) = &self.target else {
            return None;
        };
        let id = self.tree.add_folder(&path.clone(), Spot::End).ok()?;
        self.target = Target::Entry(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> FolderTree {
        let mut t = FolderTree::default();
        let g = t.add_group("Clients", Spot::End).unwrap();
        t.add_folder(Path::new("/w/north"), Spot::Into(g)).unwrap();
        t.add_folder(Path::new("/out/clips"), Spot::End).unwrap();
        t
    }

    #[test]
    fn the_choice_follows_the_tree() {
        let mut c = TargetChoice::new(&tree(), FolderId(1));
        assert_eq!(c.target, Target::default(), "a group is no target");
        assert_eq!(c.labels(), [SOURCE_NAME, "Clients › north", "clips"]);
        c.select(2);
        assert_eq!(c.folder(None), Some(PathBuf::from("/out/clips")));
        assert_eq!((c.index(), c.saved_id()), (2, FolderId(3)));
        assert_eq!(c.folder(Some(Path::new("/v/a.mp4"))), Some(PathBuf::from("/out/clips")));
        c.select(0);
        assert_eq!(c.folder(Some(Path::new("/v/a.mp4"))), Some(PathBuf::from("/v")));
        assert_eq!(c.folder(None), None, "no source yet");
        // Renamed and moved: still chosen by id; removed: back to the source.
        c.select_id(FolderId(2));
        let mut t = c.tree.clone();
        t.rename(FolderId(2), "North").unwrap();
        t.move_to(FolderId(2), Spot::End).unwrap();
        c.set_tree(&t);
        assert_eq!((c.saved_id(), c.label(&c.target)), (FolderId(2), "North".to_owned()));
        t.remove(FolderId(2)).unwrap();
        c.set_tree(&t);
        assert_eq!(c.target, Target::default());
    }

    #[test]
    fn a_loaded_jobs_unknown_folder_is_offered() {
        let mut c = TargetChoice::new(&tree(), FolderId::SOURCE);
        let source = Some(Path::new("/v/a.mp4"));
        assert_eq!(c.load(Path::new("/v"), source), Found::Source);
        assert_eq!(c.load(Path::new("/out/./clips"), source), Found::Tree);
        assert_eq!(c.saved_id(), FolderId(3));
        assert_eq!(c.load(Path::new("/media/New"), source), Found::Loose);
        assert_eq!(c.folder(source), Some(PathBuf::from("/media/New")));
        assert_eq!(c.saved_id(), FolderId::SOURCE, "a loose folder is not kept");
        assert_eq!((c.index(), c.labels().len()), (3, 4));
        assert_eq!(c.label(&c.target), JOB_TARGET);
        assert_eq!(c.tree.folders().len(), 2, "not added silently");
        assert_eq!(c.adopt(), Some(FolderId(4)));
        assert_eq!((c.saved_id(), c.labels().len()), (FolderId(4), 4));
        assert_eq!(c.adopt(), None);
        // Another video: the choice from before comes back.
        c.select_id(FolderId(3));
        c.load(Path::new("/media/Away"), source);
        assert!(c.is_loose());
        c.other_video();
        assert_eq!(c.saved_id(), FolderId(3));
        c.other_video();
        assert_eq!(c.saved_id(), FolderId(3), "nothing loose: nothing changes");
        // The tree learned the folder elsewhere: the loose choice becomes its entry.
        c.load(Path::new("/media/Other"), source);
        let mut t = c.tree.clone();
        t.add_folder(Path::new("/media/Other"), Spot::End).unwrap();
        c.set_tree(&t);
        assert_eq!(c.saved_id(), FolderId(5));
    }
}
