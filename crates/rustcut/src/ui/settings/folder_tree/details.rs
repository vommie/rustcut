//! The Target Folders page's details without Slint (P11.3): what the selected entry has, the wheel's places for its
//! star, the delete button's tooltip (a group goes with everything in it, Q-FP2) and the built-in icons for a search.

use rustcut_core::folders::{EntryRef, FolderId, FolderTree, LookKind, Picture, SOURCE_NAME};

use crate::FolderPictureView;
use crate::ui::folders::pictures::Pictures;

/// The details' choice as Slint counts it.
pub const fn kind_index(kind: LookKind) -> i32 {
    match kind {
        LookKind::Letter => 0,
        LookKind::Icon => 1,
        LookKind::Own => 2,
    }
}

/// The choice for Slint's index.
pub const fn kind_of(index: i32) -> LookKind {
    match index {
        1 => LookKind::Icon,
        2 => LookKind::Own,
        _ => LookKind::Letter,
    }
}

/// The own picture an entry keeps, shown or not.
pub fn own_view(entry: EntryRef<'_>, pictures: &Pictures) -> FolderPictureView {
    let shown = entry
        .look()
        .image
        .clone()
        .and_then(|hash| pictures.image(&Picture::Stored(hash)));
    FolderPictureView {
        has_image: shown.is_some(),
        mono: shown.as_ref().is_some_and(|s| s.mono),
        image: shown.map(|s| s.image).unwrap_or_default(),
        ..FolderPictureView::default()
    }
}

/// How many entries a group holds, all levels.
pub fn contents(tree: &FolderTree, id: FolderId) -> usize {
    tree.walk().iter().filter(|w| w.groups.contains(&id)).count()
}

/// The delete button's tooltip for the selected entry (empty: nothing to delete).
pub fn delete_tip(tree: &FolderTree, id: Option<FolderId>) -> String {
    let Some(id) = id.filter(|id| !id.is_source()) else {
        return "\"Next to the source\" stays".to_owned();
    };
    let Some(node) = tree.find(id) else {
        return String::new();
    };
    if !node.is_group() {
        return format!("Remove \"{}\" from the list (the folder itself stays)", node.name);
    }
    match contents(tree, id) {
        0 => format!("Delete the group \"{}\"", node.name),
        1 => format!("Delete the group \"{}\" and the 1 entry in it", node.name),
        n => format!("Delete the group \"{}\" and all {n} entries in it", node.name),
    }
}

/// One place of the wheel preview: who has it and whether it is the selected entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub occupant: Option<FolderId>,
    pub mine: bool,
}

/// The first level's places (`count`) with their favorites; `selected` marks its own.
pub fn places(tree: &FolderTree, selected: Option<FolderId>, count: u8) -> Vec<Place> {
    let favorites = tree.favorites();
    (0..count)
        .map(|place| {
            let occupant = favorites.iter().find(|(_, p)| *p == place).map(|(id, _)| *id);
            Place {
                occupant,
                mine: occupant.is_some() && occupant == selected,
            }
        })
        .collect()
}

/// The name of an entry for the details' title.
pub fn title(tree: &FolderTree, id: FolderId) -> String {
    tree.entry(id).map_or_else(String::new, |e| {
        if id.is_source() {
            SOURCE_NAME.to_owned()
        } else {
            e.name().to_owned()
        }
    })
}

/// The built-in icons matching `search` (all for an empty one), in rows of `per_row`.
pub fn icon_rows<'a>(names: impl Iterator<Item = &'a str>, search: &str, per_row: usize) -> Vec<Vec<&'a str>> {
    let search = search.trim().to_lowercase();
    let found: Vec<&str> = names.filter(|n| search.is_empty() || n.contains(&search)).collect();
    found.chunks(per_row.max(1)).map(<[&str]>::to_vec).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rustcut_core::folders::Spot;

    use super::*;

    #[test]
    fn tips_places_and_icons() {
        let mut t = FolderTree::default();
        let g = t.add_group("Clients", Spot::End).unwrap();
        let inner = t.add_group("Old", Spot::Into(g)).unwrap();
        t.add_folder(Path::new("/a"), Spot::Into(inner)).unwrap();
        let f = t.add_folder(Path::new("/v/clips"), Spot::End).unwrap();
        let empty = t.add_group("Empty", Spot::End).unwrap();
        assert_eq!(
            delete_tip(&t, Some(g)),
            "Delete the group \"Clients\" and all 2 entries in it"
        );
        assert_eq!(
            delete_tip(&t, Some(inner)),
            "Delete the group \"Old\" and the 1 entry in it"
        );
        assert_eq!(delete_tip(&t, Some(empty)), "Delete the group \"Empty\"");
        assert_eq!(
            delete_tip(&t, Some(f)),
            "Remove \"clips\" from the list (the folder itself stays)"
        );
        assert_eq!(delete_tip(&t, Some(FolderId::SOURCE)), "\"Next to the source\" stays");
        t.put_on_place(f, 2).unwrap();
        let p = places(&t, Some(f), 4);
        assert_eq!(p.len(), 4);
        assert_eq!(
            p.get(2),
            Some(&Place {
                occupant: Some(f),
                mine: true
            })
        );
        assert_eq!(
            p.first(),
            Some(&Place {
                occupant: None,
                mine: false
            })
        );
        assert_eq!(title(&t, FolderId::SOURCE), SOURCE_NAME);
        assert_eq!(kind_of(kind_index(LookKind::Own)), LookKind::Own);
        let names = ["folder", "folder-open", "heart", "home"];
        assert_eq!(
            icon_rows(names.into_iter(), "", 3),
            [vec!["folder", "folder-open", "heart"], vec!["home"]]
        );
        assert_eq!(
            icon_rows(names.into_iter(), " FOLD", 3),
            [vec!["folder", "folder-open"]]
        );
    }
}
