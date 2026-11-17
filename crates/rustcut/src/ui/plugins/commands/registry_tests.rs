use std::sync::Arc;

use rustcut_core::plugins::{Manifest, PluginInfo, PluginState, Source, Stats};
use rustcut_core::settings::{PluginEntry, PluginSettings};
use serde_json::json;

use super::*;

const MANIFEST: &str = r#"
id = "ID"
name = "NAME"
version = "1.0.0"
api = "1.2"

[[cards]]
title = "Keys"
[[cards.fields]]
key = "next_key"
label = "Next"
kind = "key"
default = "Control+PageDown"
[[cards.fields]]
key = "buttons"
label = "Buttons"
kind = "switch"
default = true

[[commands]]
id = "next"
title = "Next File"
key = "next_key"
menu = true
button = "buttons"
discards_sections = true
"#;

fn plugin(id: &str, name: &str, state: PluginState) -> PluginInfo {
    let manifest = Manifest::parse(&MANIFEST.replace("ID", id).replace("NAME", name)).expect("manifest");
    PluginInfo {
        id: id.to_owned(),
        manifest: Some(Arc::new(manifest)),
        dir: "/plugins".into(),
        source: Source::Installed,
        state,
        stats: Stats::default(),
        update: None,
    }
}

fn press(text: &str, control: bool) -> KeyPress<'_> {
    KeyPress {
        text,
        control,
        alt: false,
        shift: false,
        meta: false,
        repeat: false,
    }
}

#[test]
fn a_running_plugins_command_has_its_default_key_and_button() {
    let list = [plugin("next-in-folder", "Next in Folder", PluginState::Ready)];
    let entries = build(&list, &PluginSettings::default(), &Keymap::default());
    let [entry] = entries.as_slice() else {
        panic!("one command: {entries:?}")
    };
    assert_eq!(entry.key_text, "Control+PageDown");
    assert!(entry.button && entry.menu && entry.discards_sections);
    assert!(entry.taken_by.is_none());
    let page_down = char::from(slint::platform::Key::PageDown).to_string();
    assert_eq!(
        lookup(&entries, &press(&page_down, true)).map(|e| e.id.as_str()),
        Some("next")
    );
    assert!(lookup(&entries, &press(&page_down, false)).is_none());
}

#[test]
fn stopped_plugins_have_no_commands() {
    let list = [
        plugin("aa", "A", PluginState::Off),
        plugin("bb", "B", PluginState::Paused("x".into())),
    ];
    assert!(build(&list, &PluginSettings::default(), &Keymap::default()).is_empty());
}

#[test]
fn a_taken_key_stays_unbound_and_names_its_holder() {
    let mut settings = PluginSettings::default();
    let entry = |key: &str| PluginEntry {
        values: json!({ "next_key": key, "buttons": false })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        ..PluginEntry::default()
    };
    settings.entries.insert("first".into(), entry("Control+O"));
    settings.entries.insert("second".into(), entry("Control+PageDown"));
    let list = [
        plugin("first", "First", PluginState::Ready),
        plugin("second", "Second", PluginState::Ready),
        plugin("third", "Third", PluginState::Ready),
    ];
    let keymap = Keymap::default();
    let entries = build(&list, &settings, &keymap);
    assert_eq!(entries[0].taken_by, Some(Holder::Action(Action::Open)));
    assert!(!entries[0].button);
    assert!(entries[1].taken_by.is_none());
    // The third uses the default Ctrl+PageDown, which the second has.
    assert!(matches!(&entries[2].taken_by, Some(Holder::Command { plugin, .. }) if plugin == "second"));
    assert_eq!(entries[2].key_label(), "");

    let chord = Chord::parse("Control+PageDown").expect("chord");
    assert!(holder_of(&chord, &keymap, &entries, Some(("second", "next_key"))).is_none());
    let holder = holder_of(&chord, &keymap, &entries, Some(("third", "next_key"))).expect("held");
    assert_eq!(holder.describe(), "\"Next File\" of Second");
}
