//! `plugin.toml`: who a plugin is, what it needs and which settings it has.

mod schema;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use schema::{Card, ChoiceOption, Condition, Field, FieldKind, PathSelect};

/// The limits of settings values.
pub mod schema_limits {
    pub use super::schema::{MAX_ROWS, MAX_TEXT};
}

/// The plugin API version this RustCut implements (the WIT package version of `rustcut-plugin-api`).
pub const API_VERSION: semver::Version = semver::Version::new(1, 0, 0);

/// The manifest's file name inside a plugin folder and a plugin file.
pub const MANIFEST_FILE: &str = "plugin.toml";

/// The component's file name inside an installed plugin and a plugin file.
pub const WASM_FILE: &str = "plugin.wasm";

/// A plugin's `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Lower case letters, digits and dashes, 2 to 40 characters: `auto-folder`. Also the settings key.
    pub id: String,
    /// The name RustCut shows: "Auto-Folder".
    pub name: String,
    pub version: semver::Version,
    /// The plugin API version the plugin was built against: "1.0".
    pub api: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub repository: String,
    /// A Tabler icon name RustCut knows (`folder-bolt`) or the file `icon.svg` next to the manifest.
    #[serde(default)]
    pub icon: String,
    /// The component, relative to the manifest's folder. Installed plugins always use `plugin.wasm`; a development
    /// folder points to its build output (`../../target/wasm32-wasip2/release/auto_folder.wasm`).
    #[serde(default = "default_wasm")]
    pub wasm: PathBuf,
    /// The plugin's settings as cards on its Settings page.
    #[serde(default)]
    pub cards: Vec<Card>,
}

fn default_wasm() -> PathBuf {
    PathBuf::from(WASM_FILE)
}

/// Why a manifest cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("plugin.toml is not valid: {0}")]
    Parse(String),
    #[error("plugin.toml: {0}")]
    Invalid(String),
}

impl Manifest {
    /// Reads and checks `dir/plugin.toml`.
    ///
    /// # Errors
    /// The file cannot be read, is no valid manifest or breaks a rule of [`Manifest::validate`].
    pub fn read(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| ManifestError::Read { path, source })?;
        Self::parse(&text)
    }

    /// Parses and checks a manifest's text.
    ///
    /// # Errors
    /// See [`Manifest::read`].
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(text).map_err(|err| ManifestError::Parse(err.message().to_owned()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The manifest as TOML (for a plugin file, with `wasm` reset to `plugin.wasm`).
    ///
    /// # Errors
    /// Only if TOML cannot express a value, which the schema rules out.
    pub fn to_toml(&self) -> Result<String, ManifestError> {
        toml::to_string_pretty(self).map_err(|err| ManifestError::Invalid(err.to_string()))
    }

    /// The API version as a semantic version (`"1.0"` -> 1.0.0).
    ///
    /// # Errors
    /// `api` is not `MAJOR.MINOR` or `MAJOR.MINOR.PATCH`.
    pub fn api_version(&self) -> Result<semver::Version, ManifestError> {
        let text = if self.api.matches('.').count() == 1 {
            format!("{}.0", self.api)
        } else {
            self.api.clone()
        };
        semver::Version::parse(&text)
            .map_err(|err| ManifestError::Invalid(format!("api \"{}\" is not a version like \"1.0\": {err}", self.api)))
    }

    /// Whether this RustCut can run the plugin: the same major API version and no newer minor one.
    #[must_use]
    pub fn api_compatible(&self) -> bool {
        self.api_version()
            .is_ok_and(|v| v.major == API_VERSION.major && (v.minor, v.patch) <= (API_VERSION.minor, API_VERSION.patch))
    }

    /// Checks the rules a manifest must follow.
    ///
    /// # Errors
    /// The first broken rule, in words for the plugin's author.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let invalid = |text: String| Err(ManifestError::Invalid(text));
        if !valid_id(&self.id) {
            return invalid(format!(
                "id \"{}\" must be 2 to 40 lower case letters, digits and dashes, starting with a letter",
                self.id
            ));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            return invalid("name must be 1 to 60 characters".to_owned());
        }
        self.api_version()?;
        if self.wasm.as_os_str().is_empty() {
            return invalid("wasm must name a file".to_owned());
        }
        if !self.icon.is_empty() && !valid_icon(&self.icon) {
            return invalid(format!(
                "icon \"{}\" must be a Tabler icon name or \"icon.svg\"",
                self.icon
            ));
        }
        schema::validate(&self.cards).map_err(ManifestError::Invalid)
    }

    /// Every field of every card.
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.cards.iter().flat_map(|card| card.fields.iter())
    }
}

/// `auto-folder`: lower case ASCII letters, digits and dashes, starting with a letter, no double or trailing dash.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    let len = id.len();
    (2..=40).contains(&len)
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_icon(icon: &str) -> bool {
    icon == "icon.svg"
        || (icon.len() <= 60
            && icon
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}
