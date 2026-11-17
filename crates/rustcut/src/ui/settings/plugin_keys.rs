//! Plugin command keys in the Settings window (plugin API 1.2, Q-NF1): recording a plugin's key field (refused with a
//! bubble when RustCut or another command has the key), Settings > Hotkeys' read-only "Plugins" group with the way to
//! the plugin's page, and the Hotkeys page's refusal of a key a plugin command has.

use std::cell::RefCell;

use serde_json::Value;
use slint::{ModelRc, VecModel};

use super::SettingsBridge;
use super::handlers::{bubble_key, post_refused};
use super::hotkeys;
use super::pages::PageRef;
use super::plugins::PluginKey;
use crate::ui::keymap::{Chord, KeyPress};
use crate::ui::notices;
use crate::ui::plugins::commands::{self, CommandEntry, holder_of};
use crate::{HotkeyChipView, HotkeyRowView};

thread_local! {
    /// The plugin key field waiting for its key.
    static RECORDING: RefCell<Option<PluginKey>> = const { RefCell::new(None) };
}

/// The setting key of the plugin key field that waits for a key, if any.
pub(super) fn recording() -> Option<String> {
    RECORDING.with(|r| r.borrow().as_ref().map(|k| PluginKey::text(&k.id, &k.field)))
}

impl SettingsBridge {
    /// `id` names a plugin's key field: it waits for the next key (`false`: no plugin key, a keymap action).
    pub(super) fn record_plugin_key(&self, id: &str) -> bool {
        let Some(key) = PluginKey::parse(id) else { return false };
        self.hotkeys.borrow_mut().cancel();
        notices::clear(&bubble_key(id));
        RECORDING.with(|r| r.replace(Some(key)));
        self.publish();
        true
    }

    /// A key press while a plugin key field waits: stored, or refused with a bubble (`false`: none waits).
    pub(super) fn plugin_key_pressed(&self, press: &KeyPress<'_>) -> bool {
        let Some(key) = RECORDING.with(|r| r.borrow().clone()) else {
            return false;
        };
        // A modifier alone: keep waiting for the key.
        let Some(chord) = hotkeys::chord_of(press) else {
            return true;
        };
        RECORDING.with(RefCell::take);
        let row = PluginKey::text(&key.id, &key.field);
        let entries = commands::entries();
        let holder = holder_of(&chord, &self.keymap.borrow(), &entries, Some((&key.id, &key.field)));
        if let Some(holder) = holder {
            let reason = format!(
                "{} already belongs to {}. Choose another key, for example {}.",
                chord.label(),
                holder.describe(),
                free_example(&self.keymap.borrow(), &entries, &key)
            );
            tracing::info!("[Settings] Plugin key refused: {} is taken", chord.text());
            post_refused(&row, &reason, notices::TYPING);
        } else {
            notices::clear(&bubble_key(&row));
            self.store_value(&key.id, &key.field, Value::String(chord.text().to_owned()));
        }
        self.publish();
        true
    }

    /// Ends a plugin key field's waiting (`false`: none waited).
    pub(super) fn cancel_plugin_recording() -> bool {
        RECORDING.with(RefCell::take).is_some()
    }

    /// Removes a plugin command's key (`false`: `id` is no plugin key field).
    pub(super) fn clear_plugin_key(&self, id: &str) -> bool {
        let Some(key) = PluginKey::parse(id) else { return false };
        notices::clear(&bubble_key(id));
        self.store_value(&key.id, &key.field, Value::String(String::new()));
        self.publish();
        true
    }

    /// Settings > Hotkeys: the way to a plugin command's page.
    pub(super) fn hotkey_plugin_page(&self, id: &str) {
        if let Some(key) = PluginKey::parse(id) {
            self.show_page(PageRef::Plugin(key.id));
        }
    }

    /// Settings > Hotkeys: a key a plugin command has is refused there (its owner is the plugin's page).
    pub(super) fn plugin_owner_note(press: &KeyPress<'_>) -> Option<String> {
        let chord = hotkeys::chord_of(press)?;
        let entries = commands::entries();
        let entry = entries
            .iter()
            .find(|e| e.active_chord().is_some_and(|c| c.overlaps(&chord)))?;
        Some(format!(
            "{} is used by \"{}\" of the plugin {}. Change it on the plugin's page first.",
            chord.label(),
            entry.title,
            entry.plugin_name
        ))
    }
}

/// A key nobody has, for the bubble's example (Ctrl+Alt+F1..F12, then none).
fn free_example(keymap: &crate::ui::keymap::Keymap, entries: &[CommandEntry], own: &PluginKey) -> String {
    (1..=12)
        .filter_map(|n| Chord::parse(&format!("Control+Alt+F{n}")).ok())
        .find(|chord| holder_of(chord, keymap, entries, Some((&own.id, &own.field))).is_none())
        .map_or_else(|| "another letter with Ctrl and Alt".to_owned(), |chord| chord.label())
}

/// The read-only rows of Settings > Hotkeys' "Plugins" group that match `search` (title, plugin or key).
pub(super) fn hotkey_rows(search: &str) -> Vec<HotkeyRowView> {
    let wanted = search.trim().to_lowercase();
    commands::entries()
        .iter()
        .filter(|e| {
            wanted.is_empty()
                || [e.title.as_str(), e.plugin_name.as_str(), e.key_label().as_str()]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&wanted))
        })
        .map(|e| HotkeyRowView {
            id: PluginKey::text(&e.plugin, &e.key_field).into(),
            title: e.title.as_str().into(),
            group: format!("Plugins › {}", e.plugin_name).into(),
            keys: ModelRc::new(VecModel::from(
                e.active_chord()
                    .map(|c| HotkeyChipView {
                        text: c.text().into(),
                        label: c.label().into(),
                    })
                    .into_iter()
                    .collect::<Vec<_>>(),
            )),
            changed: false,
            defaults: slint::SharedString::default(),
            plugin: true,
        })
        .collect()
}
