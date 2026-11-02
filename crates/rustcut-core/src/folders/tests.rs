use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;

fn id(n: u32) -> FolderId {
    FolderId(n)
}

/// Clients (group 1: North 2, South 3, Old (group 4: 2024 5)), Holiday 6, Drone 7.
fn sample() -> FolderTree {
    let mut t = FolderTree::default();
    let clients = t.add_group("Clients", Spot::End).unwrap();
    t.add_folder(Path::new("/w/north"), Spot::Into(clients)).unwrap();
    t.add_folder(Path::new("/w/south"), Spot::Into(clients)).unwrap();
    let old = t.add_group("Old", Spot::Into(clients)).unwrap();
    t.add_folder(Path::new("/a/2024"), Spot::Into(old)).unwrap();
    t.add_folder(Path::new("/v/holiday"), Spot::End).unwrap();
    t.add_folder(Path::new("/v/drone"), Spot::End).unwrap();
    t
}

fn names(t: &FolderTree) -> Vec<String> {
    t.walk()
        .iter()
        .map(|w| format!("{}{}", "-".repeat(w.depth), w.node.name))
        .collect()
}

#[test]
fn a_tree_is_built_and_walked_in_order() {
    let t = sample();
    assert_eq!(
        names(&t),
        ["Clients", "-north", "-south", "-Old", "--2024", "holiday", "drone"]
    );
    assert_eq!(t.next_id(), id(8));
    assert_eq!(t.groups_of(id(5)), [id(1), id(4)]);
    assert_eq!(group_path(&t, id(5)), "Clients › Old");
    assert_eq!(t.folders().len(), 5);
    assert_eq!(t.find_path(Path::new("/v/drone")).map(|n| n.id), Some(id(7)));
    assert_eq!(t.entry(FolderId::SOURCE).map(EntryRef::name), Some(SOURCE_NAME));
}

#[test]
fn edits_keep_the_rules() {
    let mut t = sample();
    assert_eq!(
        t.add_folder(Path::new("/v/drone"), Spot::End),
        Err(EditError::DuplicatePath)
    );
    assert_eq!(
        t.add_folder(Path::new("/x"), Spot::Into(id(7))),
        Err(EditError::NotGroup)
    );
    assert_eq!(t.add_group(" ", Spot::End), Err(EditError::EmptyName));
    assert_eq!(t.rename(id(6), " Crete "), Ok(true));
    assert_eq!(t.rename(id(6), "Crete"), Ok(false));
    assert_eq!(t.rename(FolderId::SOURCE, "x"), Err(EditError::Source));
    // A name that was the folder's own name follows a new path; an own name stays.
    assert_eq!(t.set_path(id(7), PathBuf::from("/v/air")), Ok(true));
    assert_eq!(t.find(id(7)).unwrap().name, "air");
    assert_eq!(t.set_path(id(6), PathBuf::from("/v/kreta")), Ok(true));
    assert_eq!(t.find(id(6)).unwrap().name, "Crete");
    assert_eq!(
        t.set_path(id(6), PathBuf::from("/v/air")),
        Err(EditError::DuplicatePath)
    );
    assert_eq!(t.set_path(id(1), PathBuf::from("/g")), Err(EditError::Group));
    // Moves: into a group, before an entry, never into itself.
    assert_eq!(t.move_to(id(7), Spot::Into(id(4))), Ok(true));
    assert_eq!(t.move_to(id(1), Spot::Into(id(4))), Err(EditError::IntoItself));
    assert_eq!(t.move_to(id(1), Spot::Into(id(1))), Err(EditError::IntoItself));
    assert_eq!(t.move_to(id(6), Spot::Before(id(1))), Ok(true));
    assert_eq!(t.move_to(id(6), Spot::Before(id(6))), Ok(false));
    assert_eq!(t.move_to(id(2), Spot::After(id(3))), Ok(true));
    assert_eq!(
        names(&t),
        ["Crete", "Clients", "-south", "-north", "-Old", "--2024", "--air"]
    );
    // A group goes with its contents.
    let gone = t.remove(id(4)).unwrap();
    assert_eq!(gone.children.len(), 2);
    assert_eq!(names(&t), ["Crete", "Clients", "-south", "-north"]);
    assert_eq!(t.remove(FolderId::SOURCE), Err(EditError::Source));
    assert_eq!(t.remove(id(4)), Err(EditError::Unknown));
    // Colors and pictures, the source too.
    assert_eq!(t.set_color(id(6), Some("#A0B0C0")), Ok(true));
    assert_eq!(t.find(id(6)).unwrap().color.as_deref(), Some("#a0b0c0"));
    assert_eq!(t.set_color(id(6), Some("red")), Ok(true), "not a color: none");
    assert_eq!(t.find(id(6)).unwrap().color, None);
    assert_eq!(t.set_icon(FolderId::SOURCE, "folder"), Ok(true));
    assert_eq!(t.source.look.shown(), Some(Picture::Icon("folder".into())));
    assert_eq!(t.set_icon(FolderId::SOURCE, "folder"), Ok(false));
    // The wheel's short name (0.0.84): empty = none.
    assert_eq!(t.set_short_name(id(6), "  Crete "), Ok(true));
    assert_eq!(t.entry(id(6)).map(EntryRef::wheel_name), Some("Crete"));
    assert_eq!(t.set_short_name(id(6), ""), Ok(true));
    assert_eq!(
        t.entry(id(6)).map(EntryRef::wheel_name),
        Some("Crete"),
        "the name itself"
    );
    assert_eq!(t.set_short_name(FolderId::SOURCE, "Here"), Ok(true));
    assert_eq!(t.entry(FolderId::SOURCE).map(EntryRef::wheel_name), Some("Here"));
    assert_eq!(t.entry(FolderId::SOURCE).map(EntryRef::name), Some(SOURCE_NAME));
}

#[test]
fn favorites_have_their_own_places() {
    let mut t = sample();
    assert_eq!(t.set_favorite(id(6), true, 4), Ok(true));
    assert_eq!(t.set_favorite(FolderId::SOURCE, true, 4), Ok(true));
    assert_eq!(t.set_favorite(id(6), true, 4), Ok(false), "already one");
    assert_eq!(t.favorites(), [(FolderId::SOURCE, 1), (id(6), 0)]);
    assert_eq!(t.put_on_place(id(7), 3), Ok(true));
    assert_eq!(t.set_favorite(id(1), true, 4), Ok(true));
    assert_eq!(t.set_favorite(id(2), true, 4), Err(EditError::NoFreePlace));
    // Swapping: Drone takes Holiday's place, Holiday Drone's.
    assert_eq!(t.put_on_place(id(7), 0), Ok(true));
    assert_eq!(t.find(id(6)).unwrap().place, Some(3));
    // A newcomer on a taken place: the holder loses its star.
    assert_eq!(t.put_on_place(id(5), 0), Ok(true));
    assert_eq!(t.find(id(7)).unwrap().place, None);
    assert_eq!(t.set_favorite(id(5), false, 4), Ok(true));
    assert_eq!(t.find(id(5)).unwrap().place, None);
}

#[test]
fn sanitize_repairs_a_hand_edited_tree() {
    let text = r##"
        [source]
        color = "blue"
        place = 40

        [[entries]]
        id = 3
        name = "A"
        path = "/a"
        place = 2
        [[entries.children]]
        id = 3
        name = " inner "
        path = "/a/in"
        place = 2

        [[entries]]
        id = 0
        name = ""
        path = "/a"

        [[entries]]
        name = ""
        color = "#123456"
        [[entries.children]]
        id = 9
        name = "B"
        path = "/b"
        bogus = 1
        [[entries.children]]
        id = "broken"
    "##;
    let mut t: FolderTree = toml::from_str(text).unwrap();
    let notes = t.sanitize();
    assert!(!notes.is_empty());
    assert_eq!(t.source, SourceEntry::default());
    assert_eq!(names(&t), ["A", "inner", "Group", "-B"]);
    let ids: BTreeSet<u32> = t.walk().iter().map(|w| w.node.id.0).collect();
    assert_eq!(ids.len(), 4, "unique ids");
    assert!(!ids.contains(&0));
    assert_eq!(t.find_path(Path::new("/a/in")).unwrap().place, None, "a repeated place");
    let again = t.clone();
    assert!(t.sanitize().is_empty(), "stable");
    assert_eq!(t, again);
}

#[test]
fn a_tree_survives_toml() {
    let mut t = sample();
    t.set_favorite(id(6), true, 8).unwrap();
    t.set_image(id(2), Some("ab12".into())).unwrap();
    t.set_color(FolderId::SOURCE, Some("#ff0000")).unwrap();
    let text = toml::to_string(&t).unwrap();
    let back: FolderTree = toml::from_str(&text).unwrap();
    assert_eq!(back, t);
    // An unknown picture kind counts as none.
    let odd: FolderTree =
        toml::from_str("[[entries]]\nid = 1\nname = \"x\"\npath = \"/x\"\npicture = { video = \"1\" }\n").unwrap();
    assert_eq!(odd.entries.first().unwrap().look.shown(), None);
}

fn shown(level: &Level) -> Vec<String> {
    level
        .slots
        .iter()
        .map(|s| match s {
            Slot::Empty => "-".to_owned(),
            Slot::Entry(id) => id.0.to_string(),
            Slot::More(ids) => format!("more{ids:?}"),
        })
        .collect()
}

#[test]
fn wheel_levels_place_entries() {
    let mut t = sample();
    let fixed = Layout {
        places: 4,
        divided: false,
    };
    // No favorites: an empty first level (the user's answer, 2026-10-28).
    let start = level(&t, &LevelKey::Start, fixed).unwrap();
    assert_eq!(shown(&start), ["-", "-", "-", "-"]);
    t.put_on_place(id(1), 1).unwrap();
    t.put_on_place(id(6), 2).unwrap();
    let start = level(&t, &LevelKey::Start, fixed).unwrap();
    assert_eq!(shown(&start), ["-", "1", "6", "-"], "the source only with a star");
    assert_eq!(start.slots.get(1).unwrap().opens(&t), Some(LevelKey::Group(id(1))));
    assert_eq!(start.slots.get(2).unwrap().opens(&t), None);
    t.set_favorite(id(1), false, 4).unwrap();
    t.set_favorite(id(6), false, 4).unwrap();
    let clients = level(&t, &LevelKey::Group(id(1)), fixed).unwrap();
    assert_eq!(shown(&clients), ["2", "3", "4", "-"]);
    let divided = Layout {
        places: 4,
        divided: true,
    };
    assert_eq!(
        shown(&level(&t, &LevelKey::Group(id(1)), divided).unwrap()),
        ["2", "3", "4"]
    );
    // More entries than places: "More ..." with the rest, itself a level.
    for n in 0..4 {
        t.add_folder(Path::new(&format!("/m/{n}")), Spot::Into(id(1))).unwrap();
    }
    let clients = level(&t, &LevelKey::Group(id(1)), fixed).unwrap();
    assert_eq!(
        shown(&clients),
        [
            "2",
            "3",
            "4",
            "more[FolderId(8), FolderId(9), FolderId(10), FolderId(11)]"
        ]
    );
    let more = clients.slots.last().unwrap().opens(&t).unwrap();
    assert_eq!(shown(&level(&t, &more, fixed).unwrap()), ["8", "9", "10", "11"]);
    assert_eq!(level(&t, &LevelKey::Group(id(6)), fixed), None, "not a group");
    // Favorites stand on their places; one beyond the count takes a free place.
    t.put_on_place(id(6), 2).unwrap();
    t.put_on_place(id(1), 0).unwrap();
    t.put_on_place(id(7), 9).unwrap();
    assert_eq!(
        shown(&level(&t, &LevelKey::Start, fixed).unwrap()),
        ["1", "7", "6", "-"]
    );
    assert_eq!(shown(&level(&t, &LevelKey::Start, divided).unwrap()), ["1", "6", "7"]);
    t.put_on_place(id(2), 1).unwrap();
    t.put_on_place(id(3), 3).unwrap();
    assert_eq!(
        shown(&level(&t, &LevelKey::Start, fixed).unwrap()),
        ["1", "2", "6", "more[FolderId(3), FolderId(7)]"],
        "no free place left: the last one becomes More"
    );
}

fn listed(rows: &[ListRow]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            ListRow::Heading(s) => format!("[{}]", s.title()),
            ListRow::Entry {
                id,
                depth,
                group,
                expanded,
                context,
            } => format!(
                "{}{}{}{}{}",
                "-".repeat(*depth),
                id.0,
                if *group { "g" } else { "" },
                if *expanded { "+" } else { "" },
                if context.is_empty() {
                    String::new()
                } else {
                    format!(" ({context})")
                }
            ),
        })
        .collect()
}

#[test]
fn the_list_shows_source_favorites_recent_and_the_tree() {
    let mut t = sample();
    let mut collapsed = BTreeSet::new();
    let input = |t: &FolderTree, collapsed: &BTreeSet<FolderId>, recent: &[FolderId], query: &str| {
        listed(&rows(&ListInput {
            tree: &t.clone(),
            recent,
            collapsed: &collapsed.clone(),
            query,
        }))
    };
    assert_eq!(
        input(&t, &collapsed, &[], ""),
        ["0", "[All folders]", "1g+", "-2", "-3", "-4g+", "--5", "6", "7"]
    );
    t.set_favorite(id(5), true, 8).unwrap();
    t.set_favorite(FolderId::SOURCE, true, 8).unwrap();
    collapsed.insert(id(4));
    let mut recent = Vec::new();
    for r in [id(3), FolderId::SOURCE, id(1), id(6), id(7), id(3)] {
        remember_recent(&mut recent, r);
    }
    assert_eq!(recent, [id(3), id(7), id(6), id(1)]);
    assert_eq!(
        input(&t, &collapsed, &recent, ""),
        [
            "0",
            "[Favorites]",
            "5 (Clients › Old)",
            "[Recent]",
            "3 (Clients)",
            "7",
            "6",
            "[All folders]",
            "1g+",
            "-2",
            "-3",
            "-4g",
            "6",
            "7"
        ]
    );
    // Search: every level, by name, path or group.
    assert_eq!(input(&t, &collapsed, &recent, "OLD"), ["5 (Clients › Old)"]);
    assert_eq!(input(&t, &collapsed, &recent, "/v/"), ["6", "7"]);
    assert_eq!(input(&t, &collapsed, &recent, "next"), ["0"]);
    assert!(input(&t, &collapsed, &recent, "zzz").is_empty());
}

#[test]
fn a_change_of_the_picture_choice_keeps_the_icon_and_the_picture() {
    let mut t = FolderTree::default();
    let a = t.add_folder(Path::new("/a"), Spot::End).unwrap();
    t.set_image(a, Some("hash".into())).unwrap();
    t.set_icon(a, "folder").unwrap();
    assert_eq!(t.entry(a).unwrap().picture(), Some(Picture::Icon("folder".into())));
    t.set_picture_kind(a, LookKind::Letter).unwrap();
    assert_eq!(t.entry(a).unwrap().picture(), None, "the letter shows");
    assert!(t.stored_pictures().contains("hash"), "kept, so the store keeps it");
    t.set_picture_kind(a, LookKind::Own).unwrap();
    assert_eq!(t.entry(a).unwrap().picture(), Some(Picture::Stored("hash".into())));
    // Deleted: the choice stays and the letter shows.
    t.set_image(a, None).unwrap();
    assert_eq!(t.entry(a).unwrap().look().kind, LookKind::Own);
    assert_eq!(t.entry(a).unwrap().picture(), None);
    assert!(t.stored_pictures().is_empty());
    assert_eq!(t.entry(a).unwrap().look().icon.as_deref(), Some("folder"));
}

#[test]
fn a_place_holds_what_it_leads_to() {
    let mut t = FolderTree::default();
    let g = t.add_group("Clients", Spot::End).unwrap();
    let f = t.add_folder(Path::new("/a"), Spot::Into(g)).unwrap();
    let other = t.add_folder(Path::new("/b"), Spot::End).unwrap();
    assert!(Slot::Entry(g).holds(&t, f), "its group");
    assert!(Slot::Entry(f).holds(&t, f));
    assert!(!Slot::Entry(other).holds(&t, f));
    assert!(Slot::More(vec![other, g]).holds(&t, f), "More ... with its group");
    assert!(!Slot::Empty.holds(&t, f));
}

#[test]
fn an_id_is_never_given_twice() {
    let mut t = sample();
    assert_eq!(t.next_id(), id(8));
    let new = t.add_folder(Path::new("/v/new"), Spot::End).unwrap();
    assert_eq!(new, id(8));
    assert!(t.remove(new).is_ok(), "the entry with the highest id goes");
    let again = t.add_folder(Path::new("/v/again"), Spot::End).unwrap();
    assert_eq!(again, id(9), "the deleted entry's id stays unused");
    let group = t.add_group("Later", Spot::End).unwrap();
    assert_eq!(group, id(10));
    let text = toml::to_string(&t).unwrap();
    assert!(text.contains("last_id = 10"), "{text}");
    let back: FolderTree = toml::from_str(&text).unwrap();
    assert_eq!(back.next_id(), id(11), "the counter survives a restart");
}

#[test]
fn a_file_without_the_counter_starts_above_its_ids() {
    let old = "[[entries]]\nid = 3\nname = \"a\"\npath = \"/a\"\n\n[[entries]]\nid = 7\nname = \"b\"\npath = \"/b\"\n";
    let mut t: FolderTree = toml::from_str(old).unwrap();
    assert_eq!(t.last_id, 0, "written before 0.0.105");
    assert!(t.sanitize().is_empty(), "nothing to repair");
    assert_eq!(t.last_id, 7);
    assert_eq!(t.add_folder(Path::new("/c"), Spot::End).unwrap(), id(8));
}
