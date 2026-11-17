//! The files of a video's folder in the order the user sees them in Dolphin (when asked and possible), else by name.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::dolphin::{bus, props};
use super::natural::{NameOrder, SortingChoice};
use super::order::{self, Entry, SortRole, SortSpec};
use crate::media::is_video_file;

/// When Dolphin's order counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileManagerMode {
    /// Never: names A to Z.
    Off,
    /// Only while the folder is open in a tab of a Dolphin window.
    #[default]
    OpenTab,
    /// While any Dolphin window exists (Dolphin's stored order for the folder).
    Running,
    /// Always, read from Dolphin's stored view properties; Dolphin is never started.
    Always,
}

/// Which hidden files (a name starting with a dot) count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HiddenFiles {
    /// Never.
    Skip,
    /// Always.
    Show,
    /// Like Dolphin's "Show hidden files" for this folder (skipped when Dolphin's settings are unknown).
    #[default]
    LikeFileManager,
}

/// What to list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The open video; its folder is listed.
    pub current: PathBuf,
    /// When Dolphin's order counts.
    pub file_manager: FileManagerMode,
    /// Which hidden files count.
    pub hidden: HiddenFiles,
    /// Only the formats RustCut opens (the current file always counts).
    pub videos_only: bool,
}

/// Why the files are in name order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "reason", content = "detail")]
pub enum NamesBecause {
    /// Dolphin's order is switched off.
    Off,
    /// No Dolphin window exists.
    NotRunning,
    /// No Dolphin tab shows the folder.
    NotOpen,
    /// Dolphin sorts by something RustCut cannot rebuild (its role id).
    Unsupported(String),
    /// Dolphin could not be asked.
    Failed(String),
}

/// Where the order came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderSource {
    /// Dolphin's order.
    FileManager,
    /// Names A to Z.
    Names(NamesBecause),
}

/// The folder's files, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The files.
    pub files: Vec<PathBuf>,
    /// The open video's place in `files`.
    pub current: Option<usize>,
    /// The order that was used.
    pub sort: SortSpec,
    /// Where it came from.
    pub source: OrderSource,
}

/// Lists the folder of `request.current`; asks Dolphin when the request says so.
///
/// # Errors
/// The video has no folder or the folder cannot be read.
pub async fn list(request: &Request, paths: Option<&props::DolphinPaths>) -> Result<Listing, String> {
    let folder = request.current.parent().ok_or("the video has no folder")?.to_path_buf();
    let stored = paths.map(|p| props::stored_view(&folder, p));
    let (sort, source, choice) = decide(request.file_manager, &folder, stored.as_ref()).await;
    let hidden_shown = match request.hidden {
        HiddenFiles::Skip => false,
        HiddenFiles::Show => true,
        HiddenFiles::LikeFileManager => stored.as_ref().is_some_and(|s| s.hidden_shown),
    };
    let current = request.current.clone();
    let videos_only = request.videos_only;
    let want_types = sort.role == SortRole::Type;
    let sort_spec = sort.clone();
    let files = tokio::task::spawn_blocking(move || {
        let mut entries = read_folder(&folder, &current, hidden_shown, videos_only, want_types)?;
        let names = NameOrder::for_environment(choice);
        order::sort(&mut entries, &sort_spec, &names);
        Ok::<_, String>(entries.into_iter().map(|e| e.path).collect::<Vec<_>>())
    })
    .await
    .map_err(|e| e.to_string())??;
    let current = files.iter().position(|f| f == &request.current);
    Ok(Listing {
        files,
        current,
        sort,
        source,
    })
}

/// The order: Dolphin's when it counts and is known, else names.
async fn decide(
    mode: FileManagerMode,
    folder: &Path,
    stored: Option<&props::StoredView>,
) -> (SortSpec, OrderSource, SortingChoice) {
    let names = |because| (SortSpec::default(), OrderSource::Names(because), SortingChoice::Natural);
    let from_dolphin = match mode {
        FileManagerMode::Off => return names(NamesBecause::Off),
        FileManagerMode::Always => match stored {
            Some(stored) => stored.sort.clone(),
            None => return names(NamesBecause::Failed("Dolphin's folders are unknown".to_owned())),
        },
        FileManagerMode::OpenTab | FileManagerMode::Running => match bus::ask(folder, true).await {
            Err(error) => {
                tracing::info!("[NextInFolder] Dolphin could not be asked: {error}");
                return names(NamesBecause::Failed(error));
            }
            Ok(view) if view.windows == 0 => return names(NamesBecause::NotRunning),
            Ok(view) if mode == FileManagerMode::OpenTab && !view.open => return names(NamesBecause::NotOpen),
            Ok(view) => {
                let hidden_last = stored.is_some_and(|s| s.sort.hidden_last);
                match (view.sort, stored) {
                    (Some((role, descending)), _) => SortSpec {
                        role,
                        descending,
                        hidden_last,
                    },
                    (None, Some(stored)) => stored.sort.clone(),
                    (None, None) => SortSpec::default(),
                }
            }
        },
    };
    if let SortRole::Unsupported(role) = &from_dolphin.role {
        return names(NamesBecause::Unsupported(role.clone()));
    }
    let choice = stored.map_or(SortingChoice::Natural, |s| s.choice);
    (from_dolphin, OrderSource::FileManager, choice)
}

fn seconds(time: std::io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(-1)
}

fn read_folder(
    folder: &Path,
    current: &Path,
    hidden_shown: bool,
    videos_only: bool,
    want_types: bool,
) -> Result<Vec<Entry>, String> {
    let reader = std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut entries = Vec::new();
    for item in reader.flatten() {
        let path = item.path();
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        let hidden = name.starts_with('.');
        let is_current = path == current;
        if !is_current && ((hidden && !hidden_shown) || (videos_only && !is_video_file(&path))) {
            continue;
        }
        let type_name = if want_types {
            super::type_name::of(&path)
        } else {
            String::new()
        };
        entries.push(Entry {
            name,
            hidden,
            size: meta.len(),
            modified: seconds(meta.modified()),
            created: seconds(meta.created()),
            accessed: seconds(meta.accessed()),
            type_name,
            path,
        });
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
