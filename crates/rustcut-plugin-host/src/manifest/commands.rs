//! `[[commands]]` of `plugin.toml` (API 1.2): what a plugin can be told to do by a key, a File menu entry or a menu
//! bar button.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{FieldKind, Manifest, valid_icon, valid_id};

/// One command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Lower case letters, digits and dashes (`next-file`); handed to the plugin's `run`.
    pub id: String,
    /// The menu entry's and the Hotkeys row's text: "Next File".
    pub title: String,
    /// The button's tooltip and the Hotkeys row's description.
    #[serde(default)]
    pub description: String,
    /// A Tabler icon name RustCut knows, for the menu bar button.
    #[serde(default)]
    pub icon: String,
    /// The `key` field (kind `key`) that holds the command's key.
    pub key: String,
    /// Shown in the File menu while the plugin is on.
    #[serde(default)]
    pub menu: bool,
    /// A `switch` field: while it is on, the command has a button in the menu bar.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub button: String,
    /// The command discards the editor's unsaved sections (it opens another video): with unsaved sections the key,
    /// the entry and the button must be held for the hold time of Settings > General.
    #[serde(default)]
    pub discards_sections: bool,
}

/// The rules of `[[commands]]`.
pub(super) fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.commands.len() > 8 {
        return Err("at most 8 commands".to_owned());
    }
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for command in &manifest.commands {
        let id = &command.id;
        if !valid_id(id) {
            return Err(format!(
                "command id \"{id}\" must be 2 to 40 lower case letters, digits and dashes"
            ));
        }
        if !ids.insert(id.as_str()) {
            return Err(format!("command \"{id}\" is declared twice"));
        }
        if command.title.trim().is_empty() || command.title.chars().count() > 40 {
            return Err(format!("command \"{id}\": title must be 1 to 40 characters"));
        }
        if !command.icon.is_empty() && (!valid_icon(&command.icon) || command.icon == "icon.svg") {
            return Err(format!("command \"{id}\": icon must be a Tabler icon name"));
        }
        match manifest.field(&command.key).map(|f| &f.kind) {
            Some(FieldKind::Key { .. }) => {}
            _ => {
                return Err(format!(
                    "command \"{id}\": key \"{}\" must name a field of kind \"key\"",
                    command.key
                ));
            }
        }
        if !keys.insert(command.key.as_str()) {
            return Err(format!(
                "command \"{id}\": key field \"{}\" belongs to another command",
                command.key
            ));
        }
        if !command.button.is_empty()
            && !matches!(
                manifest.field(&command.button).map(|f| &f.kind),
                Some(FieldKind::Switch { .. })
            )
        {
            return Err(format!(
                "command \"{id}\": button \"{}\" must name a field of kind \"switch\"",
                command.button
            ));
        }
    }
    Ok(())
}
