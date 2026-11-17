//! Plugin commands (API 1.2): a command runs on its plugin's worker; the folder listing it asks for comes from
//! [`crate::folder_walk`] on the runtime (the worker thread waits for it, never the UI).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustcut_plugin_host::commands::{self as host, FolderFile, FolderListing, FolderOrder, FolderRequest, Navigator};

use crate::folder_walk::{self, NamesBecause, OrderSource};
use crate::naming::same_path;

/// What a command's run brought back, with the plugin's name for its messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub id: String,
    pub name: String,
    pub outcome: host::CommandOutcome,
}

/// Lists folders for commands with RustCut's knowledge: the queue's sources and Dolphin's folders.
pub(super) struct CoreNavigator {
    pub runtime: tokio::runtime::Handle,
    /// The sources of every job in the queue (any state).
    pub job_sources: Arc<Vec<PathBuf>>,
    pub dolphin: Option<folder_walk::dolphin::props::DolphinPaths>,
}

impl Navigator for CoreNavigator {
    fn list_folder(&self, video: &Path, request: FolderRequest) -> Result<FolderListing, String> {
        let request = folder_walk::Request {
            current: video.to_path_buf(),
            file_manager: match request.file_manager {
                host::FileManagerMode::Off => folder_walk::FileManagerMode::Off,
                host::FileManagerMode::OpenTab => folder_walk::FileManagerMode::OpenTab,
                host::FileManagerMode::Running => folder_walk::FileManagerMode::Running,
                host::FileManagerMode::Always => folder_walk::FileManagerMode::Always,
            },
            hidden: match request.hidden {
                host::HiddenFiles::Skip => folder_walk::HiddenFiles::Skip,
                host::HiddenFiles::Show => folder_walk::HiddenFiles::Show,
                host::HiddenFiles::LikeFileManager => folder_walk::HiddenFiles::LikeFileManager,
            },
            videos_only: request.videos_only,
        };
        let listing = self
            .runtime
            .block_on(folder_walk::list(&request, self.dolphin.as_ref()))?;
        Ok(convert(listing, &self.job_sources))
    }
}

fn convert(listing: folder_walk::Listing, job_sources: &[PathBuf]) -> FolderListing {
    let files = listing
        .files
        .into_iter()
        .map(|path| FolderFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            has_job: job_sources.iter().any(|source| same_path(source, &path)),
            path,
        })
        .collect();
    let (source, detail) = match listing.source {
        OrderSource::FileManager => (host::OrderSource::FileManager, String::new()),
        OrderSource::Names(NamesBecause::Off) => (host::OrderSource::NamesOff, String::new()),
        OrderSource::Names(NamesBecause::NotRunning) => (host::OrderSource::NamesNotRunning, String::new()),
        OrderSource::Names(NamesBecause::NotOpen) => (host::OrderSource::NamesNotOpen, String::new()),
        OrderSource::Names(NamesBecause::Unsupported(role)) => (host::OrderSource::NamesUnsupported, role),
        OrderSource::Names(NamesBecause::Failed(why)) => (host::OrderSource::NamesFailed, why),
    };
    let role = match source {
        host::OrderSource::NamesUnsupported => detail.clone(),
        _ => listing.sort.role.id().to_owned(),
    };
    let current = listing.current.and_then(|i| u32::try_from(i).ok());
    FolderListing {
        files,
        current,
        order: FolderOrder {
            source,
            role,
            descending: listing.sort.descending,
            detail,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder_walk::order::{SortRole, SortSpec};

    #[test]
    fn jobs_and_unsupported_roles_reach_the_plugin() {
        let listing = folder_walk::Listing {
            files: vec![PathBuf::from("/v/a.mp4"), PathBuf::from("/v/b.mp4")],
            current: Some(1),
            sort: SortSpec {
                role: SortRole::Name,
                descending: false,
                hidden_last: false,
            },
            source: OrderSource::Names(NamesBecause::Unsupported("rating".into())),
        };
        let converted = convert(listing, &[PathBuf::from("/v/./a.mp4")]);
        assert!(converted.files[0].has_job);
        assert!(!converted.files[1].has_job);
        assert_eq!(converted.files[1].name, "b.mp4");
        assert_eq!(converted.current, Some(1));
        assert_eq!(converted.order.source, host::OrderSource::NamesUnsupported);
        assert_eq!(converted.order.role, "rating");
    }
}
