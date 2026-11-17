//! Which plugin commands exist now (API 1.2) and which keys they have: one list for the main window (keys, File menu,
//! menu bar buttons), Settings > Hotkeys (the read-only "Plugins" group) and the plugins' key fields (conflicts).
//!
//! A command exists while its plugin runs. Its key is the value of its `key` field; a key that one of RustCut's own
//! actions or an earlier command (in the plugins' order) already has is not bound and the plugin's page says so: the
//! user chooses another (Q-NF1: refused, never taken away).

use rustcut_core::plugins::{FieldKind, PluginInfo, PluginState, values};
use rustcut_core::settings::PluginSettings;
use serde_json::Value;

use crate::ui::keymap::{Action, Chord, KeyPress, Keymap};

/// One command of a running plugin.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub plugin: String,
    pub plugin_name: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    /// The `key` field holding the command's key.
    pub key_field: String,
    /// The key as stored (canonical text), empty = none.
    pub key_text: String,
    /// The parsed key; `None` when there is none or it cannot be read.
    pub chord: Option<Chord>,
    /// Who has the key already; then the command has no key.
    pub taken_by: Option<Holder>,
    pub menu: bool,
    /// The menu bar button is switched on.
    pub button: bool,
    pub discards_sections: bool,
}

impl CommandEntry {
    /// The key that runs the command (none while it is taken).
    pub fn active_chord(&self) -> Option<&Chord> {
        if self.taken_by.is_some() {
            None
        } else {
            self.chord.as_ref()
        }
    }

    /// The menu's and tooltip's key text ("" when none).
    pub fn key_label(&self) -> String {
        self.active_chord().map(Chord::label).unwrap_or_default()
    }
}

/// Who has a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Action(Action),
    Command {
        plugin: String,
        plugin_name: String,
        title: String,
        key_field: String,
    },
}

impl Holder {
    /// "\"Open\"" or "\"Next File\" of Next in Folder".
    pub fn describe(&self) -> String {
        match self {
            Self::Action(action) => format!("\"{}\"", action.title()),
            Self::Command { plugin_name, title, .. } => format!("\"{title}\" of {plugin_name}"),
        }
    }
}

/// The commands of the running plugins in the plugins' order, their keys checked.
pub fn build(list: &[PluginInfo], settings: &PluginSettings, keymap: &Keymap) -> Vec<CommandEntry> {
    let mut entries: Vec<CommandEntry> = Vec::new();
    for info in list.iter().filter(|p| p.state == PluginState::Ready) {
        let Some(manifest) = &info.manifest else { continue };
        if manifest.commands.is_empty() {
            continue;
        }
        let stored = settings
            .entries
            .get(&info.id)
            .map(|e| e.values.clone())
            .unwrap_or_default();
        let values = values::effective(manifest, &stored);
        for command in &manifest.commands {
            let key_text = match manifest.field(&command.key).map(|f| &f.kind) {
                Some(FieldKind::Key { .. }) => values.get(&command.key).and_then(Value::as_str).unwrap_or_default(),
                _ => "",
            }
            .to_owned();
            let chord = Chord::parse(&key_text).ok();
            let taken_by = chord.as_ref().and_then(|chord| {
                keymap
                    .holder(chord)
                    .map(Holder::Action)
                    .or_else(|| command_holder(&entries, chord))
            });
            let button =
                !command.button.is_empty() && values.get(&command.button).and_then(Value::as_bool) == Some(true);
            entries.push(CommandEntry {
                plugin: info.id.clone(),
                plugin_name: info.name().to_owned(),
                id: command.id.clone(),
                title: command.title.clone(),
                description: command.description.clone(),
                icon: command.icon.clone(),
                key_field: command.key.clone(),
                key_text,
                chord,
                taken_by,
                menu: command.menu,
                button,
                discards_sections: command.discards_sections,
            });
        }
    }
    entries
}

fn command_holder(entries: &[CommandEntry], chord: &Chord) -> Option<Holder> {
    entries
        .iter()
        .find(|e| e.active_chord().is_some_and(|c| c.overlaps(chord)))
        .map(as_holder)
}

fn as_holder(entry: &CommandEntry) -> Holder {
    Holder::Command {
        plugin: entry.plugin.clone(),
        plugin_name: entry.plugin_name.clone(),
        title: entry.title.clone(),
        key_field: entry.key_field.clone(),
    }
}

/// The command `press` runs.
pub fn lookup<'a>(entries: &'a [CommandEntry], press: &KeyPress<'_>) -> Option<&'a CommandEntry> {
    entries
        .iter()
        .find(|e| e.active_chord().is_some_and(|c| c.matches(press)))
}

/// Who already has `chord`, except the key field `own` (`plugin`, `field`) itself: RustCut's actions first, then
/// the commands with a working key.
pub fn holder_of(
    chord: &Chord,
    keymap: &Keymap,
    entries: &[CommandEntry],
    own: Option<(&str, &str)>,
) -> Option<Holder> {
    keymap.holder(chord).map(Holder::Action).or_else(|| {
        entries
            .iter()
            .filter(|e| own != Some((e.plugin.as_str(), e.key_field.as_str())))
            .find(|e| e.active_chord().is_some_and(|c| c.overlaps(chord)))
            .map(as_holder)
    })
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
