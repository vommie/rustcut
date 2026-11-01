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

/// What an entry shows: its first letter, a built-in icon or an own picture (the user's choice, P12 3.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookKind {
    #[default]
    Letter,
    Icon,
    Own,
}

/// What makes an entry recognizable (P12 3.2): the choice beside the icon and the own picture it keeps, so a change of
/// the choice loses nothing. "Icon" or "Own" with nothing kept shows the letter and stays what it is.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    #[serde_as(as = "DefaultOnError")]
    pub kind: LookKind,
    /// A built-in icon by its Tabler name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A picture of the picture store by its hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl Look {
    /// The picture the entry shows now (`None` = its first letter).
    pub fn shown(&self) -> Option<Picture> {
        match self.kind {
            LookKind::Letter => None,
            LookKind::Icon => self.icon.clone().map(Picture::Icon),
            LookKind::Own => self.image.clone().map(Picture::Stored),
        }
    }

    /// Nothing chosen and nothing kept (not written to the file).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
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
    #[serde(skip_serializing_if = "Look::is_empty")]
    pub look: Look,
    /// `#RRGGBB`; none = the theme's colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A favorite's place on the wheel's first level (0 = top, clockwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<u8>,
    /// The name the wheel shows instead of `name` (0.0.84, the user's answer; none = the name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
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
    #[serde(skip_serializing_if = "Look::is_empty")]
    pub look: Look,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<u8>,
    /// The name the wheel shows (0.0.84; none = "Next to the source").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
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

    pub const fn look(self) -> &'a Look {
        match self {
            Self::Source(s) => &s.look,
            Self::Node(n) => &n.look,
        }
    }

    /// The picture the entry shows now (`None` = its first letter).
    pub fn picture(self) -> Option<Picture> {
        self.look().shown()
    }

    pub fn color(self) -> Option<&'a str> {
        match self {
            Self::Source(s) => s.color.as_deref(),
            Self::Node(n) => n.color.as_deref(),
        }
    }

    /// The name the wheel shows: the short name when there is one.
    pub fn wheel_name(self) -> &'a str {
        let short = match self {
            Self::Source(s) => s.short_name.as_deref(),
            Self::Node(n) => n.short_name.as_deref(),
        };
        short.unwrap_or_else(|| self.name())
    }

    pub fn short_name(self) -> Option<&'a str> {
        match self {
            Self::Source(s) => s.short_name.as_deref(),
            Self::Node(n) => n.short_name.as_deref(),
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

    /// The hashes of the stored pictures the tree keeps, shown or not (the picture store keeps these, removes the rest).
    pub fn stored_pictures(&self) -> std::collections::HashSet<String> {
        std::iter::once(&self.source.look)
            .chain(self.walk().into_iter().map(|w| &w.node.look))
            .filter_map(|look| look.image.clone())
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
