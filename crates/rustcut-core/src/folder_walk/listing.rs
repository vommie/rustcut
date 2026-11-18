//! The files of a video's folder in the order the user sees them in Dolphin (when asked and possible), else by name.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::cache::{FolderCache, Variant};
use super::dolphin::{bus, props};
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use super::natural::{NameKey, NameOrder, SortingChoice};
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

/// Lists the folder of `request.current`; asks Dolphin when the request says so. With `cache`, a listing of the same
/// folder that no change touched since is used again (see [`super::cache`]).
///
/// # Errors
/// The video has no folder or the folder cannot be read.
pub async fn list(
    request: &Request,
    paths: Option<&props::DolphinPaths>,
    cache: Option<&FolderCache>,
) -> Result<Listing, String> {
    // Lexically clean (`a/../b` = `b`): Dolphin compares folder URLs exactly.
    let current_path = path_clean::clean(&request.current);
    let folder = current_path.parent().ok_or("the video has no folder")?.to_path_buf();
    let stored = paths.map(|p| props::stored_view(&folder, p));
    let (sort, source, choice) = decide(request.file_manager, &folder, stored.as_ref()).await;
    let hidden_shown = match request.hidden {
        HiddenFiles::Skip => false,
        HiddenFiles::Show => true,
        HiddenFiles::LikeFileManager => stored.as_ref().is_some_and(|s| s.hidden_shown),
    };
    let current = current_path.clone();
    let videos_only = request.videos_only;
    let wanted = Wanted {
        hidden: hidden_shown,
        videos_only,
        metadata: matches!(
            sort.role,
            SortRole::Size | SortRole::Modified | SortRole::Created | SortRole::Accessed
        ),
        types: sort.role == SortRole::Type,
    };
    let variant = Variant {
        sort: sort.clone(),
        choice,
        hidden: hidden_shown,
        videos_only,
    };
    if let Some(files) = cache.and_then(|c| c.get(&folder, &variant)) {
        tracing::debug!(
            "[NextInFolder] {} unchanged: its listing is used again",
            folder.display()
        );
        let current = files.iter().position(|f| f == &current_path);
        return Ok(Listing {
            files: files.to_vec(),
            current,
            sort,
            source,
        });
    }
    let ticket = cache.and_then(|c| c.begin(&folder));
    let sort_spec = sort.clone();
    let files = tokio::task::spawn_blocking(move || {
        let names = NameOrder::for_environment(choice);
        let mut entries = read_folder(&folder, &current, wanted, &names)?;
        order::sort(&mut entries, &sort_spec, &names);
        Ok::<_, String>(entries.into_iter().map(|e| e.path).collect::<Vec<_>>())
    })
    .await
    .map_err(|e| e.to_string())??;
    if let (Some(cache), Some(ticket)) = (cache, ticket) {
        cache.put(ticket, variant, std::sync::Arc::new(files.clone()));
    }
    let current = files.iter().position(|f| f == &current_path);
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
        FileManagerMode::OpenTab | FileManagerMode::Running => match ask_dolphin(folder).await {
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

/// Asks Dolphin about `folder`, and about its real path when Dolphin does not show it but the path holds a link.
async fn ask_dolphin(folder: &Path) -> Result<bus::BusView, String> {
    let view = bus::ask(folder, true).await?;
    tracing::debug!(
        "[NextInFolder] Dolphin: {} windows, open {}, sort {:?}",
        view.windows,
        view.open,
        view.sort
    );
    if view.open || view.windows == 0 {
        return Ok(view);
    }
    match tokio::fs::canonicalize(folder).await {
        Ok(real) if real != folder => bus::ask(&real, true).await,
        _ => Ok(view),
    }
}

fn seconds(time: std::io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(-1)
}

/// The files of `folder` that count, with what the order needs. A name sort reads only the folder (the file type
/// comes with each entry, no `stat`); sizes and dates are read on every core, so a slow mount answers many at once.
fn read_folder(folder: &Path, current: &Path, wanted: Wanted, names: &NameOrder) -> Result<Vec<Entry>, String> {
    let reader = std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut found = Vec::new();
    for item in reader.flatten() {
        let file_name = item.file_name();
        let is_current = current.file_name() == Some(file_name.as_os_str());
        let name = file_name.to_string_lossy().into_owned();
        let hidden = name.starts_with('.');
        // Names first: most files are left out before anything else is asked.
        if !is_current && ((hidden && !wanted.hidden) || (wanted.videos_only && !is_video_file(&name))) {
            continue;
        }
        // The entry's type costs nothing on most file systems; a link is followed (its target counts).
        let (is_file, link) = match item.file_type() {
            Ok(kind) if kind.is_symlink() => (true, true),
            Ok(kind) => (kind.is_file(), false),
            Err(_) => (true, true),
        };
        if is_file {
            found.push((name, file_name, hidden, link));
        }
    }
    let mut entries: Vec<Entry> = found
        .into_par_iter()
        .filter_map(|(name, file_name, hidden, link)| {
            let path = folder.join(file_name);
            let meta = if wanted.metadata || link {
                Some(std::fs::metadata(&path).ok()?)
            } else {
                None
            };
            if meta.as_ref().is_some_and(|m| !m.is_file()) {
                return None;
            }
            let time = |get: fn(&std::fs::Metadata) -> std::io::Result<SystemTime>| {
                meta.as_ref().map_or(-1, |m| seconds(get(m)))
            };
            Some(Entry {
                key: names.key(&name),
                hidden,
                size: meta.as_ref().map_or(0, std::fs::Metadata::len),
                modified: time(std::fs::Metadata::modified),
                created: time(std::fs::Metadata::created),
                accessed: time(std::fs::Metadata::accessed),
                type_name: NameKey::default(),
                path,
            })
        })
        .collect();
    if wanted.types {
        super::type_name::fill(&mut entries, names);
    }
    Ok(entries)
}

/// What a listing needs besides the names.
#[derive(Debug, Clone, Copy)]
struct Wanted {
    hidden: bool,
    videos_only: bool,
    /// Sizes and dates (the sort role needs them).
    metadata: bool,
    types: bool,
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
