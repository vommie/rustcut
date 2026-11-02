//! Settings of the plugins (P12 block 8, user decisions Q-P12g). Nothing here existed before, so older files read as
//! these defaults without a migration. Each plugin's own values live under `[plugins.entries.<id>.values]` and stay
//! when the plugin is removed, so a reinstall finds them again.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::{DefaultOnError, serde_as};

/// A plugin's call time limit, in milliseconds.
pub const PLUGIN_TIME_LIMIT_MS: std::ops::RangeInclusive<u32> = 100..=30_000;
/// A plugin's memory limit, in MB.
pub const PLUGIN_MEMORY_MB: std::ops::RangeInclusive<u32> = 16..=1024;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Off: no plugin runs; each plugin keeps its own switch.
    pub enabled: bool,
    /// The order plugins run in, top first (a later plugin sees the earlier ones' changes and wins). Installed plugins
    /// missing here run after these, by name.
    pub order: Vec<String>,
    /// Plugin folders loaded straight from their build (a `plugin.toml` with `wasm` pointing to the build output);
    /// reloaded when the build changes.
    pub dev_folders: Vec<PathBuf>,
    /// Where newer plugin files are looked for ("check for updates"); `None` = nowhere.
    pub update_folder: Option<PathBuf>,
    /// Per plugin id; an unreadable entry reads as the defaults instead of failing the whole file.
    #[serde_as(as = "BTreeMap<_, DefaultOnError>")]
    pub entries: BTreeMap<String, PluginEntry>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            order: Vec::new(),
            dev_folders: Vec::new(),
            update_folder: None,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginEntry {
    pub enabled: bool,
    /// How long one call may take before it is stopped.
    pub time_limit_ms: u32,
    /// How much memory the plugin may use.
    pub memory_mb: u32,
    /// The plugin's own settings (the fields of its `plugin.toml`), stored as TOML and handed over as JSON.
    #[serde(with = "values_toml", skip_serializing_if = "Map::is_empty")]
    pub values: Map<String, Value>,
}

impl Default for PluginEntry {
    fn default() -> Self {
        Self {
            enabled: true,
            time_limit_ms: 1000,
            memory_mb: 64,
            values: Map::new(),
        }
    }
}

impl PluginSettings {
    /// The entry of a plugin, or the defaults.
    pub fn entry(&self, id: &str) -> PluginEntry {
        self.entries.get(id).cloned().unwrap_or_default()
    }

    /// Brings the limits into range and drops ids that cannot be plugin ids; true when something changed.
    pub fn sanitize(&mut self) -> bool {
        let mut changed = false;
        let before = self.entries.len();
        self.entries.retain(|id, _| rustcut_plugin_host::manifest::valid_id(id));
        changed |= before != self.entries.len();
        for entry in self.entries.values_mut() {
            let time = entry
                .time_limit_ms
                .clamp(*PLUGIN_TIME_LIMIT_MS.start(), *PLUGIN_TIME_LIMIT_MS.end());
            let memory = entry
                .memory_mb
                .clamp(*PLUGIN_MEMORY_MB.start(), *PLUGIN_MEMORY_MB.end());
            changed |= time != entry.time_limit_ms || memory != entry.memory_mb;
            entry.time_limit_ms = time;
            entry.memory_mb = memory;
        }
        let mut seen = std::collections::HashSet::new();
        let before = self.order.len();
        self.order
            .retain(|id| rustcut_plugin_host::manifest::valid_id(id) && seen.insert(id.clone()));
        changed |= before != self.order.len();
        changed
    }
}

/// JSON values stored as a TOML table (TOML has no `null`: such members are left out and read back as missing).
mod values_toml {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Map, Value};

    pub(super) fn serialize<S: Serializer>(values: &Map<String, Value>, serializer: S) -> Result<S::Ok, S::Error> {
        rustcut_plugin_host::values::to_toml(values).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Map<String, Value>, D::Error> {
        let table = toml::Table::deserialize(deserializer)?;
        Ok(rustcut_plugin_host::values::from_toml(&table))
    }
}
