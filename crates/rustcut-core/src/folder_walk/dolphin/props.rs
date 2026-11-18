//! Dolphin's stored view properties, read without Dolphin (`ViewProperties` in Dolphin's `viewproperties.cpp`).
//!
//! `dolphinrc [General] GlobalViewProps` (default on) = one style for all folders, kept on
//! `<data>/dolphin/view_properties/global`. Otherwise each folder keeps its own: in the extended attribute
//! `user.kde.fm.viewproperties#1` of the folder (or a `.directory` file in it) for writable folders below home, else
//! under `view_properties/local/<sha1 of the folder's URL>`. A folder without stored properties (or with properties
//! older than `ViewPropsTimestamp`) shows the global ones; Downloads starts newest first. The value is KConfig text
//! with the groups `[Dolphin]` (sort) and `[Settings]` (hidden files).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha1::{Digest as _, Sha1};

use super::super::natural::SortingChoice;
use super::super::order::{SortRole, SortSpec};

/// The extended attribute Dolphin keeps a folder's view properties in.
pub const XATTR: &str = "user.kde.fm.viewproperties#1";

/// Where Dolphin's files live for this user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DolphinPaths {
    /// `$XDG_CONFIG_HOME` (holds `dolphinrc`).
    pub config_home: PathBuf,
    /// `$XDG_DATA_HOME` (holds `dolphin/view_properties`).
    pub data_home: PathBuf,
    /// The home folder.
    pub home: PathBuf,
    /// The XDG download folder, Dolphin's one special folder for files.
    pub downloads: Option<PathBuf>,
}

impl DolphinPaths {
    /// The paths of the user running RustCut.
    #[must_use]
    pub fn of_user() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        let downloads = directories::UserDirs::new().and_then(|u| u.download_dir().map(Path::to_path_buf));
        Some(Self {
            config_home: base.config_dir().to_path_buf(),
            data_home: base.data_dir().to_path_buf(),
            home: base.home_dir().to_path_buf(),
            downloads,
        })
    }
}

/// What Dolphin stored for a folder, with Dolphin's defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredView {
    /// Role, direction, hidden last.
    pub sort: SortSpec,
    /// "Show hidden files" is on for this folder.
    pub hidden_shown: bool,
    /// Natural or code point name order (`SortingChoice`).
    pub choice: SortingChoice,
}

/// KConfig text as groups of keys (`[A][B]` is the group `A][B`; `Key[$e]` is `Key`).
#[must_use]
pub fn parse_kconfig(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inner.clone_into(&mut current);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.split('[').next().unwrap_or_default().trim();
        groups
            .entry(current.clone())
            .or_default()
            .insert(key.to_owned(), unescape(value.trim()));
    }
    groups
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// The `[Dolphin]` and `[Settings]` groups stored at `place` (a folder: its `.directory` wins over its attribute).
fn read_at(place: &Path) -> Option<HashMap<String, HashMap<String, String>>> {
    let from_file = std::fs::read_to_string(place.join(".directory"))
        .ok()
        .map(|text| parse_kconfig(&text))
        .filter(|groups| groups.contains_key("Dolphin") || groups.contains_key("Settings"));
    from_file.or_else(|| {
        let bytes = xattr::get(place, XATTR).ok().flatten()?;
        Some(parse_kconfig(&String::from_utf8_lossy(&bytes)))
    })
}

/// Dolphin's `directoryHashForUrl`: SHA-1 of the encoded file URL, base64, `/` -> `-`.
#[must_use]
pub fn folder_hash(folder: &Path) -> Option<String> {
    let url = super::file_url::file_url(folder)?;
    let digest = Sha1::digest(url.as_bytes());
    Some(
        base64::engine::general_purpose::STANDARD
            .encode(digest)
            .replace('/', "-"),
    )
}

/// KConfig's `y,m,d,h,min,s.ms` as numbers, comparable as a tuple.
fn timestamp(value: &str) -> Vec<f64> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect()
}

fn get<'a>(groups: &'a HashMap<String, HashMap<String, String>>, group: &str, key: &str) -> Option<&'a str> {
    groups.get(group).and_then(|g| g.get(key)).map(String::as_str)
}

fn flag(value: Option<&str>, default: bool) -> bool {
    value.map_or(default, |v| matches!(v.trim(), "true" | "1"))
}

/// What Dolphin would show for `folder`, from its files.
#[must_use]
pub fn stored_view(folder: &Path, paths: &DolphinPaths) -> StoredView {
    let rc = std::fs::read_to_string(paths.config_home.join("dolphinrc"))
        .map(|t| parse_kconfig(&t))
        .unwrap_or_default();
    let choice = SortingChoice::from_config(get(&rc, "General", "SortingChoice").unwrap_or_default());
    let global_mode = flag(get(&rc, "General", "GlobalViewProps"), true);
    let store = paths.data_home.join("dolphin").join("view_properties");
    let global = read_at(&store.join("global"));
    let own = if global_mode {
        None
    } else {
        let below_home = folder.starts_with(&paths.home);
        let writable = rustix::fs::access(folder, rustix::fs::Access::WRITE_OK).is_ok();
        let place = if below_home && writable {
            Some(folder.to_path_buf())
        } else {
            folder_hash(folder).map(|hash| store.join("local").join(hash))
        };
        let since = get(&rc, "General", "ViewPropsTimestamp").map(timestamp);
        place.and_then(|p| read_at(&p)).filter(|groups| {
            let stamp = get(groups, "Dolphin", "Timestamp").map(timestamp);
            match (stamp, &since) {
                (Some(stamp), Some(since)) => stamp >= *since,
                _ => true,
            }
        })
    };
    let downloads = !global_mode && own.is_none() && paths.downloads.as_deref() == Some(folder);
    if downloads {
        return StoredView {
            sort: SortSpec {
                role: SortRole::Modified,
                descending: true,
                hidden_last: false,
            },
            hidden_shown: false,
            choice,
        };
    }
    let groups = own.or(global).unwrap_or_default();
    let role = SortRole::from_dolphin(get(&groups, "Dolphin", "SortRole").unwrap_or("text"));
    let descending = get(&groups, "Dolphin", "SortOrder").is_some_and(|v| v.trim() == "1");
    StoredView {
        sort: SortSpec {
            role,
            descending,
            hidden_last: flag(get(&groups, "Dolphin", "SortHiddenLast"), false),
        },
        hidden_shown: flag(get(&groups, "Settings", "HiddenFilesShown"), false),
        choice,
    }
}

#[cfg(test)]
#[path = "props_tests.rs"]
mod tests;
