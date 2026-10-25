use proptest::prelude::*;
use slint::platform::Key;

use super::*;

fn press(text: &str) -> KeyPress<'_> {
    KeyPress {
        text,
        control: false,
        alt: false,
        shift: false,
        meta: false,
        repeat: false,
    }
}

fn special(key: Key) -> String {
    char::from(key).to_string()
}

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap()
}

#[test]
fn the_defaults_parse_and_do_not_conflict() {
    let (keymap, notes) = Keymap::from_table(&KeymapTable::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(keymap.conflicts().is_empty());
    for action in Action::ALL {
        assert_eq!(keymap.chords(action).len(), action.default_keys().len(), "{action:?}");
        assert_eq!(Action::from_id(action.id()), Some(action));
    }
    assert_eq!(keymap, Keymap::default());
}

#[test]
fn presses_find_their_actions() {
    let keymap = Keymap::default();
    let lookup = |p: KeyPress<'_>| keymap.lookup(&p);
    assert_eq!(lookup(press("i")), Some(Action::SetStart));
    assert_eq!(lookup(press(&special(Key::Home))), Some(Action::SetStart));
    assert_eq!(lookup(press(&special(Key::End))), Some(Action::SetEnd));
    assert_eq!(
        lookup(KeyPress {
            shift: true,
            ..press("I")
        }),
        Some(Action::GoToStart)
    );
    // Caps Lock writes an uppercase letter without Shift: still the plain key.
    assert_eq!(lookup(press("I")), Some(Action::SetStart));
    assert_eq!(lookup(press(&special(Key::ScrollLock))), Some(Action::AddSection));
    // `+` is Shift+= on a US layout and its own key on a German one: both add a section.
    assert_eq!(lookup(press("+")), Some(Action::AddSection));
    assert_eq!(
        lookup(KeyPress {
            shift: true,
            ..press("+")
        }),
        Some(Action::AddSection)
    );
    assert_eq!(
        lookup(KeyPress {
            control: true,
            ..press("+")
        }),
        Some(Action::ZoomIn)
    );
    assert_eq!(lookup(press(",")), Some(Action::FrameBack));
    assert_eq!(lookup(press(&special(Key::PageDown))), Some(Action::FrameForward));
    assert_eq!(lookup(press(" ")), Some(Action::TogglePause));
    assert_eq!(
        lookup(KeyPress {
            shift: true,
            ..press(&special(Key::LeftArrow))
        }),
        Some(Action::SeekBackLong)
    );
    assert_eq!(lookup(press(&special(Key::LeftArrow))), Some(Action::SeekBack));
    assert_eq!(
        lookup(KeyPress {
            control: true,
            ..press("s")
        }),
        Some(Action::Save)
    );
    assert_eq!(lookup(press(&special(Key::F9))), Some(Action::Save));
    assert_eq!(
        lookup(KeyPress {
            control: true,
            ..press("0")
        }),
        Some(Action::ZoomFit)
    );
    assert_eq!(lookup(press("s")), None);
    assert_eq!(
        lookup(KeyPress {
            alt: true,
            ..press("i")
        }),
        None
    );
}

#[test]
fn chords_parse_aliases_and_write_a_canonical_text() {
    assert_eq!(chord("ctrl+shift+s").text(), "Control+Shift+s");
    assert_eq!(chord("Shift + Control + S").text(), "Control+Shift+S");
    assert_eq!(chord("pgup").text(), "PageUp");
    assert_eq!(chord("Control++").text(), "Control+Plus");
    assert_eq!(chord("+").text(), "Plus");
    assert_eq!(chord("f9").text(), "F9");
    assert_eq!(chord("Super+Left").text(), "Meta+LeftArrow");
    assert_eq!(chord("Shift?+Comma").text(), "Shift?+Comma");
    assert!(chord("Control+S").overlaps(&chord("ctrl+s")));
    assert_eq!(chord("Control+S").label(), "Ctrl+S");
    for bad in [
        "",
        "Control+",
        "Hyper+X",
        "Control+Control+X",
        "Shift+Plus",
        "NoSuchKey",
        "ab",
    ] {
        assert!(Chord::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn overlapping_chords_are_conflicts() {
    // Comma ignores Shift; the literal `,` needs Shift released: they share the plain press.
    assert!(chord("Comma").overlaps(&chord(",")));
    assert!(chord("Shift?+A").overlaps(&chord("Shift+A")));
    assert!(!chord("A").overlaps(&chord("Shift+A")));
    assert!(!chord("Control+A").overlaps(&chord("A")));
    let mut keymap = Keymap::default();
    assert_eq!(
        keymap.bind(Action::ToggleMute, chord("i")),
        Err(Conflict {
            chord: "i".into(),
            other: Action::SetStart
        })
    );
    assert!(keymap.bind(Action::ToggleMute, chord("Control+M")).is_ok());
    assert!(
        keymap.bind(Action::ToggleMute, chord("ctrl+m")).is_ok(),
        "the same chord again is no conflict"
    );
    assert_eq!(keymap.chords(Action::ToggleMute).len(), 2);
    assert!(keymap.unbind(Action::SetStart, "I"));
    assert!(!keymap.unbind(Action::SetStart, "I"));
    assert!(keymap.bind(Action::ToggleMute, chord("I")).is_ok());
    assert!(keymap.conflicts().is_empty());
}

#[test]
fn the_stored_table_is_sanitized() {
    let mut table = KeymapTable::new();
    table.insert("toggle_mute".into(), Some(vec!["Home".into(), "Hyper+Q".into()]));
    table.insert("add_section".into(), Some(vec![]));
    table.insert("reload".into(), None);
    table.insert("teleport".into(), Some(vec!["T".into()]));
    let (keymap, notes) = Keymap::from_table(&table);
    assert_eq!(notes.len(), 4, "{notes:?}");
    // The configured action keeps Home; the default of set_start loses it.
    assert_eq!(texts(&keymap, Action::ToggleMute), ["Home"]);
    assert_eq!(texts(&keymap, Action::SetStart), ["I"]);
    assert!(keymap.chords(Action::AddSection).is_empty());
    assert_eq!(texts(&keymap, Action::Reload), ["F5"]);
    assert!(keymap.conflicts().is_empty());
}

#[test]
fn the_table_round_trips() {
    let mut keymap = Keymap::default();
    keymap.unbind(Action::Save, "F9");
    keymap.bind(Action::Reload, chord("ctrl+r")).unwrap();
    let (again, notes) = Keymap::from_table(&keymap.to_table());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(again, keymap);
}

#[test]
fn menu_entries_show_the_first_key() {
    let keymap = Keymap::default();
    assert_eq!(keymap.first_label(Action::Open), "Ctrl+O");
    let mut unbound = keymap;
    unbound.unbind(Action::Open, "Control+O");
    assert_eq!(unbound.first_label(Action::Open), "", "an unbound action shows no key");
}

fn texts(keymap: &Keymap, action: Action) -> Vec<String> {
    keymap.chords(action).iter().map(|c| c.text().to_owned()).collect()
}

fn arb_key() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("I".to_owned()),
        Just("Home".to_owned()),
        Just("Shift+I".to_owned()),
        Just("Plus".to_owned()),
        Just("Control+Plus".to_owned()),
        Just("Comma".to_owned()),
        Just(",".to_owned()),
        Just("Space".to_owned()),
        Just("F5".to_owned()),
        Just("Shift?+K".to_owned()),
        Just("K".to_owned()),
        Just("bogus key".to_owned()),
        Just(String::new()),
    ]
}

fn arb_table() -> impl Strategy<Value = KeymapTable> {
    let ids: Vec<String> = Action::ALL
        .iter()
        .map(|a| a.id().to_owned())
        .chain(["unknown".to_owned()])
        .collect();
    prop::collection::btree_map(
        prop::sample::select(ids),
        prop::option::of(prop::collection::vec(arb_key(), 0..4)),
        0..12,
    )
}

proptest! {
    #[test]
    fn any_stored_table_gives_a_conflict_free_keymap_that_survives_saving(table in arb_table()) {
        let (keymap, _) = Keymap::from_table(&table);
        prop_assert!(keymap.conflicts().is_empty());
        let (again, notes) = Keymap::from_table(&keymap.to_table());
        prop_assert!(notes.is_empty(), "{:?}", notes);
        prop_assert_eq!(again, keymap);
    }
}
