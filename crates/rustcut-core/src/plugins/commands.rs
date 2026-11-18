//! Plugin commands (API 1.2, 1.3): a command runs on its plugin's worker; the folder listing it asks for comes from
//! [`crate::folder_walk`] on the runtime (the worker thread waits for it, never the UI). A step (1.3) walks the ordered
//! paths here and hands the plugin only the file it landed on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustcut_plugin_host::commands::{
    self as host, FolderFile, FolderListing, FolderOrder, FolderRequest, FolderStep, Navigator,
};

use crate::folder_walk::{self, NamesBecause, OrderSource};

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
    /// The listing of the last folder while nothing changed in it.
    pub cache: Arc<folder_walk::FolderCache>,
}

impl Navigator for CoreNavigator {
    fn list_folder(&self, video: &Path, request: FolderRequest) -> Result<FolderListing, String> {
        let listing = self.listing(video, request)?;
        Ok(convert(listing, &self.job_sources))
    }

    fn step_folder(
        &self,
        video: &Path,
        request: FolderRequest,
        forward: bool,
        skip_with_job: bool,
        wrap: bool,
    ) -> Result<FolderStep, String> {
        let listing = self.listing(video, request)?;
        let jobs = job_set(&self.job_sources);
        let files = &listing.files;
        let has_job = |i: usize| files.get(i).is_some_and(|p| jobs.contains(p));
        let step = rustcut_plugin_host::step::step(files.len(), listing.current, forward, skip_with_job, wrap, has_job);
        let file = |i: usize| files.get(i).map(|path| folder_file(path.clone(), &jobs));
        Ok(host::folder_step(step, files.len(), file, order_of(&listing)))
    }
}

impl CoreNavigator {
    fn listing(&self, video: &Path, request: FolderRequest) -> Result<folder_walk::Listing, String> {
        let request = walk_request(video, request);
        self.runtime
            .block_on(folder_walk::list(&request, self.dolphin.as_ref(), Some(&self.cache)))
    }
}

/// A plugin's folder request for `video` as [`folder_walk::list`] takes it.
#[must_use]
pub fn walk_request(video: &Path, request: FolderRequest) -> folder_walk::Request {
    folder_walk::Request {
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
    }
}

/// The jobs' sources, cleaned once (the listing's paths are clean already): a set, not files × jobs.
fn job_set(job_sources: &[PathBuf]) -> HashSet<PathBuf> {
    job_sources.iter().map(path_clean::clean).collect()
}

fn folder_file(path: PathBuf, jobs: &HashSet<PathBuf>) -> FolderFile {
    FolderFile {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        has_job: jobs.contains(&path),
        path,
    }
}

fn order_of(listing: &folder_walk::Listing) -> FolderOrder {
    let (source, detail) = match &listing.source {
        OrderSource::FileManager => (host::OrderSource::FileManager, String::new()),
        OrderSource::Names(NamesBecause::Off) => (host::OrderSource::NamesOff, String::new()),
        OrderSource::Names(NamesBecause::NotRunning) => (host::OrderSource::NamesNotRunning, String::new()),
        OrderSource::Names(NamesBecause::NotOpen) => (host::OrderSource::NamesNotOpen, String::new()),
        OrderSource::Names(NamesBecause::Unsupported(role)) => (host::OrderSource::NamesUnsupported, role.clone()),
        OrderSource::Names(NamesBecause::Failed(why)) => (host::OrderSource::NamesFailed, why.clone()),
    };
    let role = match source {
        host::OrderSource::NamesUnsupported => detail.clone(),
        _ => listing.sort.role.id().to_owned(),
    };
    FolderOrder {
        source,
        role,
        descending: listing.sort.descending,
        detail,
    }
}

fn convert(listing: folder_walk::Listing, job_sources: &[PathBuf]) -> FolderListing {
    let jobs = job_set(job_sources);
    let order = order_of(&listing);
    let current = listing.current.and_then(|i| u32::try_from(i).ok());
    let files = listing.files.into_iter().map(|path| folder_file(path, &jobs)).collect();
    FolderListing { files, current, order }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder_walk::order::{SortRole, SortSpec};

    fn listing() -> folder_walk::Listing {
        folder_walk::Listing {
            files: vec![
                PathBuf::from("/v/a.mp4"),
                PathBuf::from("/v/b.mp4"),
                PathBuf::from("/v/c.mp4"),
            ],
            current: Some(1),
            sort: SortSpec {
                role: SortRole::Name,
                descending: false,
                hidden_last: false,
            },
            source: OrderSource::Names(NamesBecause::Unsupported("rating".into())),
        }
    }

    #[test]
    fn jobs_and_unsupported_roles_reach_the_plugin() {
        let converted = convert(listing(), &[PathBuf::from("/v/./a.mp4")]);
        assert!(converted.files[0].has_job);
        assert!(!converted.files[1].has_job);
        assert_eq!(converted.files[1].name, "b.mp4");
        assert_eq!(converted.current, Some(1));
        assert_eq!(converted.order.source, host::OrderSource::NamesUnsupported);
        assert_eq!(converted.order.role, "rating");
    }

    #[test]
    fn a_step_hands_over_only_its_file() {
        let listing = listing();
        let jobs = job_set(&[PathBuf::from("/v/c.mp4")]);
        let files = &listing.files;
        let step = rustcut_plugin_host::step::step(files.len(), listing.current, true, false, false, |i| {
            files.get(i).is_some_and(|p| jobs.contains(p))
        });
        let file = |i: usize| files.get(i).map(|p| folder_file(p.clone(), &jobs));
        let folder = host::folder_step(step, files.len(), file, order_of(&listing));
        assert_eq!(folder.position, 3);
        assert_eq!(folder.total, 3);
        assert!(folder.file.is_some_and(|f| f.has_job && f.name == "c.mp4"));
    }
}
