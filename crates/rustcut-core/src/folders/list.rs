//! The rows of the folder list (the Job Finalization card's drop-down, Q-FP2): "Next to the source", the favorites, the
//! last three targets, then the tree with its collapsed groups. Typing searches every level: the hits are the folders
//! whose name, path or group matches, each with its group path.

use std::collections::BTreeSet;

use super::tree::{FolderId, FolderTree, SOURCE_NAME};

/// How many recent targets the list shows.
pub const RECENT: usize = 3;

/// A heading between the list's parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Favorites,
    Recent,
    All,
}

impl Section {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Favorites => "Favorites",
            Self::Recent => "Recent",
            Self::All => "All folders",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListRow {
    Heading(Section),
    Entry {
        id: FolderId,
        /// Indentation in the tree part (0 elsewhere).
        depth: usize,
        group: bool,
        /// A group's state in the tree part.
        expanded: bool,
        /// The groups holding it ("Clients › North"), shown beside favorites, recent targets and search hits.
        context: String,
    },
}

impl ListRow {
    pub const fn id(&self) -> Option<FolderId> {
        match self {
            Self::Heading(_) => None,
            Self::Entry { id, .. } => Some(*id),
        }
    }

    /// A row that can be the target (not a heading, not a group).
    pub const fn pickable(&self) -> bool {
        matches!(self, Self::Entry { group: false, .. })
    }
}

/// What the list is made from.
#[derive(Debug, Clone, Copy)]
pub struct ListInput<'a> {
    pub tree: &'a FolderTree,
    /// The last targets, newest first.
    pub recent: &'a [FolderId],
    pub collapsed: &'a BTreeSet<FolderId>,
    pub query: &'a str,
}

/// The names of the groups holding `id`, joined with " › ".
pub fn group_path(tree: &FolderTree, id: FolderId) -> String {
    tree.groups_of(id)
        .iter()
        .filter_map(|g| tree.find(*g).map(|n| n.name.as_str()))
        .collect::<Vec<_>>()
        .join(" › ")
}

fn flat(tree: &FolderTree, id: FolderId) -> ListRow {
    ListRow::Entry {
        id,
        depth: 0,
        group: tree.find(id).is_some_and(super::tree::FolderNode::is_group),
        expanded: false,
        context: group_path(tree, id),
    }
}

pub fn rows(input: &ListInput<'_>) -> Vec<ListRow> {
    let tree = input.tree;
    let query = input.query.trim().to_lowercase();
    if !query.is_empty() {
        return search(tree, &query);
    }
    let mut rows = vec![flat(tree, FolderId::SOURCE)];
    let mut favorites = tree.favorites();
    favorites.retain(|(id, _)| !id.is_source());
    favorites.sort_by_key(|(_, place)| *place);
    if !favorites.is_empty() {
        rows.push(ListRow::Heading(Section::Favorites));
        rows.extend(favorites.iter().map(|(id, _)| flat(tree, *id)));
    }
    let recent: Vec<FolderId> = input
        .recent
        .iter()
        .copied()
        .filter(|id| !id.is_source() && tree.find(*id).is_some_and(|n| !n.is_group()))
        .take(RECENT)
        .collect();
    if !recent.is_empty() {
        rows.push(ListRow::Heading(Section::Recent));
        rows.extend(recent.iter().map(|id| flat(tree, *id)));
    }
    if !tree.entries.is_empty() {
        rows.push(ListRow::Heading(Section::All));
        for walked in tree.walk() {
            if walked.groups.iter().any(|g| input.collapsed.contains(g)) {
                continue;
            }
            let node = walked.node;
            rows.push(ListRow::Entry {
                id: node.id,
                depth: walked.depth,
                group: node.is_group(),
                expanded: node.is_group() && !input.collapsed.contains(&node.id),
                context: String::new(),
            });
        }
    }
    rows
}

/// `query` is lowercase and not empty.
fn search(tree: &FolderTree, query: &str) -> Vec<ListRow> {
    let hit = |text: &str| text.to_lowercase().contains(query);
    let mut rows = Vec::new();
    if hit(SOURCE_NAME) {
        rows.push(flat(tree, FolderId::SOURCE));
    }
    for walked in tree.walk() {
        let node = walked.node;
        let Some(path) = &node.path else { continue };
        let in_group = walked
            .groups
            .iter()
            .any(|g| tree.find(*g).is_some_and(|n| hit(&n.name)));
        if hit(&node.name) || hit(&path.to_string_lossy()) || in_group {
            rows.push(flat(tree, node.id));
        }
    }
    rows
}
