//! The Target Folders page's tree without Slint (P11.3): the shown rows ("Next to the source" first, then the tree
//! without the entries of folded groups) and where a dragged entry lands: before or after a row, or into a group (the
//! middle half of a group's row).

use std::collections::BTreeSet;

use rustcut_core::folders::{FolderId, FolderTree, SOURCE_NAME, Spot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub id: FolderId,
    pub depth: usize,
    pub group: bool,
    pub expanded: bool,
    pub children: usize,
    pub name: String,
    pub path: Option<std::path::PathBuf>,
    pub place: Option<u8>,
}

impl TreeRow {
    pub const fn is_source(&self) -> bool {
        self.id.is_source()
    }
}

pub fn rows(tree: &FolderTree, collapsed: &BTreeSet<FolderId>) -> Vec<TreeRow> {
    let source = TreeRow {
        id: FolderId::SOURCE,
        depth: 0,
        group: false,
        expanded: false,
        children: 0,
        name: SOURCE_NAME.to_owned(),
        path: None,
        place: tree.source.place,
    };
    std::iter::once(source)
        .chain(
            tree.walk()
                .into_iter()
                .filter(|w| !w.groups.iter().any(|g| collapsed.contains(g)))
                .map(|w| TreeRow {
                    id: w.node.id,
                    depth: w.depth,
                    group: w.node.is_group(),
                    expanded: w.node.is_group() && !collapsed.contains(&w.node.id),
                    children: w.node.children.len(),
                    name: w.node.name.clone(),
                    path: w.node.path.clone(),
                    place: w.node.place,
                }),
        )
        .collect()
}

/// Where a drop lands and how the page shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropTarget {
    pub spot: Spot,
    /// The line between rows (content y) and its indentation (depth); `None` when the drop goes into a group.
    pub line: Option<(f32, usize)>,
    /// The group row that takes the drop (its index among the rows).
    pub into: Option<usize>,
}

/// The target of `dragged` with the pointer at content y `y` over rows of height `pitch`; `None` = nowhere (on itself
/// or inside its own group).
pub fn drop_target(tree: &FolderTree, rows: &[TreeRow], dragged: FolderId, y: f32, pitch: f32) -> Option<DropTarget> {
    if pitch <= 0.0 || rows.is_empty() || dragged.is_source() {
        return None;
    }
    let position = (y / pitch).max(0.0);
    let index: usize = az::saturating_cast(position.floor());
    let fraction = position - position.floor();
    let inside = |id: FolderId| id == dragged || tree.groups_of(id).contains(&dragged);
    let Some(row) = rows.get(index) else {
        // Below the last row: the end of the top level.
        let bottom = az::cast::<usize, f32>(rows.len()) * pitch;
        return Some(DropTarget {
            spot: Spot::End,
            line: Some((bottom, 0)),
            into: None,
        });
    };
    let top = az::cast::<usize, f32>(index) * pitch;
    if row.is_source() {
        // Nothing goes before "Next to the source": the first entry of the tree is the first place.
        let first = rows.get(1).filter(|r| r.depth == 0).map(|r| r.id);
        return Some(DropTarget {
            spot: first.map_or(Spot::End, Spot::Before),
            line: Some((top + pitch, 0)),
            into: None,
        });
    }
    if inside(row.id) {
        return None;
    }
    if row.group && (0.25..0.75).contains(&fraction) {
        return Some(DropTarget {
            spot: Spot::Into(row.id),
            line: None,
            into: Some(index),
        });
    }
    if fraction < 0.5 {
        return Some(DropTarget {
            spot: Spot::Before(row.id),
            line: Some((top, row.depth)),
            into: None,
        });
    }
    // Below an open group with entries the line stands above its first entry.
    if row.expanded
        && let Some(next) = rows.get(index + 1).filter(|n| n.depth > row.depth)
    {
        return (!inside(next.id)).then_some(DropTarget {
            spot: Spot::Before(next.id),
            line: Some((top + pitch, next.depth)),
            into: None,
        });
    }
    Some(DropTarget {
        spot: Spot::After(row.id),
        line: Some((top + pitch, row.depth)),
        into: None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Clients (1: North 2, Old (3: 2024 4)), Holiday 5.
    fn tree() -> FolderTree {
        let mut t = FolderTree::default();
        let c = t.add_group("Clients", Spot::End).unwrap();
        t.add_folder(Path::new("/n"), Spot::Into(c)).unwrap();
        let old = t.add_group("Old", Spot::Into(c)).unwrap();
        t.add_folder(Path::new("/a/2024"), Spot::Into(old)).unwrap();
        t.add_folder(Path::new("/h"), Spot::End).unwrap();
        t
    }

    fn ids(rows: &[TreeRow]) -> Vec<u32> {
        rows.iter().map(|r| r.id.0).collect()
    }

    #[test]
    fn rows_hide_folded_groups() {
        let t = tree();
        let all = rows(&t, &BTreeSet::new());
        assert_eq!(ids(&all), [0, 1, 2, 3, 4, 5]);
        assert!(all.first().unwrap().is_source());
        assert_eq!(all.get(4).unwrap().depth, 2);
        let folded = rows(&t, &BTreeSet::from([FolderId(3)]));
        assert_eq!(ids(&folded), [0, 1, 2, 3, 5]);
        assert!(!folded.get(3).unwrap().expanded && folded.get(3).unwrap().group);
        assert_eq!(ids(&rows(&t, &BTreeSet::from([FolderId(1)]))), [0, 1, 5]);
    }

    #[test]
    fn drops_land_before_after_or_into() {
        let t = tree();
        let r = rows(&t, &BTreeSet::new());
        let at = |dragged: u32, y: f32| drop_target(&t, &r, FolderId(dragged), y, 10.0).map(|d| d.spot);
        assert_eq!(
            at(5, 2.0),
            Some(Spot::Before(FolderId(1))),
            "on the source: the first place"
        );
        assert_eq!(at(5, 11.0), Some(Spot::Before(FolderId(1))));
        assert_eq!(at(5, 15.0), Some(Spot::Into(FolderId(1))), "a group's middle");
        assert_eq!(
            at(5, 19.0),
            Some(Spot::Before(FolderId(2))),
            "below an open group: its first entry"
        );
        assert_eq!(at(5, 28.0), Some(Spot::After(FolderId(2))));
        assert_eq!(at(2, 52.0), Some(Spot::Before(FolderId(5))));
        assert_eq!(at(2, 99.0), Some(Spot::End), "below the rows");
        assert_eq!(at(1, 25.0), None, "into its own group");
        assert_eq!(at(3, 45.0), None, "its own entry");
        assert_eq!(at(0, 25.0), None, "the source does not move");
        let d = drop_target(&t, &r, FolderId(5), 15.0, 10.0).unwrap();
        assert_eq!((d.line, d.into), (None, Some(1)));
        let d = drop_target(&t, &r, FolderId(5), 28.0, 10.0).unwrap();
        assert_eq!(d.line, Some((30.0, 1)));
    }
}
