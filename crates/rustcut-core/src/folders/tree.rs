//! The folder tree's data (`config.toml`'s `[export.folders]`): groups (menu entries without a path, never a target),
//! folders (a path), the fixed entry "Next to the source", and what makes an entry recognizable (picture, color) and a
//! favorite (its place on the wheel's first level). Every folder stands once in the tree (user decision Q-FP2).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, VecSkipError, serde_as};

/// An entry's id: given out once (the highest + 1); 0 is "Next to the source" (D-FPa). The export card, the recent
/// targets and the list's collapsed groups refer to entries by id, so renaming or moving one never loses them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderId(pub u32);

impl FolderId {
    /// "Next to the source".
    pub const SOURCE: Self = Self(0);

    pub const fn is_source(self) -> bool {
        self.0 == 0
    }
}

/// An entry's picture (Q-FP2): a built-in icon by its Tabler name, or a picture of the picture store by its hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Picture {
    Icon(String),
    Stored(String),
}

/// The name "Next to the source" shows with (fixed: it cannot be renamed).
pub const SOURCE_NAME: &str = "Next to the source";

/// The most places a wheel level can have (the setting's range is 4-12).
pub const MAX_PLACES: u8 = 12;

/// A group (no path) or a folder (a path) of the tree.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderNode {
    pub id: FolderId,
    pub name: String,
    /// `None` = a group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// A value this version cannot read counts as missing.
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<Picture>,
    /// `#RRGGBB`; none = the theme's colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A favorite's place on the wheel's first level (0 = top, clockwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<u8>,
    /// A group's entries (a folder has none). An entry that cannot be read is skipped, so one typo never loses the
    /// others.
    #[serde_as(as = "DefaultOnError<VecSkipError<_>>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FolderNode>,
}

impl FolderNode {
    /// A folder named after its own last part.
    pub fn folder(id: FolderId, path: &Path) -> Self {
        Self {
            id,
            name: default_name(path),
            path: Some(path.to_path_buf()),
            ..Self::default()
        }
    }

    pub fn group(id: FolderId, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            ..Self::default()
        }
    }

    pub const fn is_group(&self) -> bool {
        self.path.is_none()
    }
}

/// The folder's own name, or the whole path for `/`.
pub fn default_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// What "Next to the source" keeps: it always stands first and cannot be moved, renamed or deleted (D-FPb), but it may
/// be a favorite and have a picture and a color.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceEntry {
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<Picture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<u8>,
}

/// The whole tree.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderTree {
    pub source: SourceEntry,
    #[serde_as(as = "DefaultOnError<VecSkipError<_>>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<FolderNode>,
}

/// An entry as the pickers show it: the source or a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryRef<'a> {
    Source(&'a SourceEntry),
    Node(&'a FolderNode),
}

impl<'a> EntryRef<'a> {
    pub const fn id(self) -> FolderId {
        match self {
            Self::Source(_) => FolderId::SOURCE,
            Self::Node(n) => n.id,
        }
    }

    pub fn name(self) -> &'a str {
        match self {
            Self::Source(_) => SOURCE_NAME,
            Self::Node(n) => &n.name,
        }
    }

    pub const fn picture(self) -> Option<&'a Picture> {
        match self {
            Self::Source(s) => s.picture.as_ref(),
            Self::Node(n) => n.picture.as_ref(),
        }
    }

    pub fn color(self) -> Option<&'a str> {
        match self {
            Self::Source(s) => s.color.as_deref(),
            Self::Node(n) => n.color.as_deref(),
        }
    }

    pub const fn place(self) -> Option<u8> {
        match self {
            Self::Source(s) => s.place,
            Self::Node(n) => n.place,
        }
    }

    /// A group (only a node can be one).
    pub const fn is_group(self) -> bool {
        match self {
            Self::Source(_) => false,
            Self::Node(n) => n.is_group(),
        }
    }
}

/// One entry of a depth-first walk: the node, its depth (0 = top level) and the ids of its groups from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walked<'a> {
    pub node: &'a FolderNode,
    pub depth: usize,
    pub groups: Vec<FolderId>,
}

impl FolderTree {
    /// A tree of top-level folders in this order (ids 1..n; the migration's rule, D-FPa).
    pub fn from_folders<'a>(folders: impl IntoIterator<Item = (&'a str, &'a Path)>) -> Self {
        let entries = folders
            .into_iter()
            .zip(1..)
            .map(|((name, path), id)| FolderNode {
                name: name.to_owned(),
                ..FolderNode::folder(FolderId(id), path)
            })
            .collect();
        Self {
            source: SourceEntry::default(),
            entries,
        }
    }

    /// Every node depth-first in the tree's order.
    pub fn walk(&self) -> Vec<Walked<'_>> {
        fn go<'a>(nodes: &'a [FolderNode], groups: &mut Vec<FolderId>, out: &mut Vec<Walked<'a>>) {
            for node in nodes {
                out.push(Walked {
                    node,
                    depth: groups.len(),
                    groups: groups.clone(),
                });
                if !node.children.is_empty() {
                    groups.push(node.id);
                    go(&node.children, groups, out);
                    groups.pop();
                }
            }
        }
        let mut out = Vec::new();
        go(&self.entries, &mut Vec::new(), &mut out);
        out
    }

    /// The folders (not the groups) depth-first.
    pub fn folders(&self) -> Vec<&FolderNode> {
        self.walk()
            .into_iter()
            .map(|w| w.node)
            .filter(|n| !n.is_group())
            .collect()
    }

    pub fn find(&self, id: FolderId) -> Option<&FolderNode> {
        fn go(nodes: &[FolderNode], id: FolderId) -> Option<&FolderNode> {
            nodes
                .iter()
                .find_map(|n| if n.id == id { Some(n) } else { go(&n.children, id) })
        }
        go(&self.entries, id)
    }

    pub fn find_mut(&mut self, id: FolderId) -> Option<&mut FolderNode> {
        fn go(nodes: &mut [FolderNode], id: FolderId) -> Option<&mut FolderNode> {
            for n in nodes {
                if n.id == id {
                    return Some(n);
                }
                if let Some(found) = go(&mut n.children, id) {
                    return Some(found);
                }
            }
            None
        }
        go(&mut self.entries, id)
    }

    /// The entry with this id (the source too).
    pub fn entry(&self, id: FolderId) -> Option<EntryRef<'_>> {
        if id.is_source() {
            Some(EntryRef::Source(&self.source))
        } else {
            self.find(id).map(EntryRef::Node)
        }
    }

    /// The folder with this path (compared as written; the callers compare files with `naming::same_path`).
    pub fn find_path(&self, path: &Path) -> Option<&FolderNode> {
        self.walk()
            .into_iter()
            .map(|w| w.node)
            .find(|n| n.path.as_deref() == Some(path))
    }

    /// The ids of the groups holding `id`, from the top (empty for a top-level entry or an unknown id).
    pub fn groups_of(&self, id: FolderId) -> Vec<FolderId> {
        self.walk()
            .into_iter()
            .find(|w| w.node.id == id)
            .map(|w| w.groups)
            .unwrap_or_default()
    }

    /// The next free id.
    pub fn next_id(&self) -> FolderId {
        let highest = self.walk().iter().map(|w| w.node.id.0).max().unwrap_or(0);
        FolderId(highest.saturating_add(1))
    }

    /// Every picture the tree uses (the picture store keeps these, P11.2).
    pub fn pictures(&self) -> Vec<&Picture> {
        self.source
            .picture
            .iter()
            .chain(self.walk().into_iter().filter_map(|w| w.node.picture.as_ref()))
            .collect()
    }

    /// The hashes of the stored pictures the tree uses (the picture store keeps these, removes the rest).
    pub fn stored_pictures(&self) -> std::collections::HashSet<String> {
        self.pictures()
            .into_iter()
            .filter_map(|p| match p {
                Picture::Stored(hash) => Some(hash.clone()),
                Picture::Icon(_) => None,
            })
            .collect()
    }

    /// The favorites with their places: the source first when it is one, then the tree's order.
    pub fn favorites(&self) -> Vec<(FolderId, u8)> {
        self.source
            .place
            .map(|p| (FolderId::SOURCE, p))
            .into_iter()
            .chain(
                self.walk()
                    .into_iter()
                    .filter_map(|w| w.node.place.map(|p| (w.node.id, p))),
            )
            .collect()
    }
}
