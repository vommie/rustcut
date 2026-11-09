//! The values RustCut and a plugin exchange, on RustCut's side (the WIT types of `rustcut-plugin-api` without the
//! generated bindings, so the rest of RustCut never sees them).

use std::path::PathBuf;

/// How important a log line or a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warning,
    Error,
}

/// A tag of RustView's tag tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: u32,
    pub name: String,
    /// From the root down, the tag's own name last.
    pub path: Vec<String>,
}

/// What the editor holds when an event happens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub video: Option<PathBuf>,
    pub rating: Option<u8>,
    pub tags: Vec<Tag>,
    pub target: Option<PathBuf>,
    /// The id of the folder of Settings > Target Folders that is the target (0 = next to the source); `None` for a
    /// target outside that list. Plugins read it through the host's `target-folder` (API 1.1).
    pub target_folder: Option<u32>,
}

/// Who opened a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenedBy {
    User,
    Job,
}

/// Something that happened in RustCut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    VideoOpened(OpenedBy),
    TagAssigned(Tag),
    TagRemoved(Tag),
}

impl Event {
    /// A short name for logs and statistics.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::VideoOpened(_) => "video-opened",
            Self::TagAssigned(_) => "tag-assigned",
            Self::TagRemoved(_) => "tag-removed",
        }
    }
}

/// Where a message appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    None,
    Toast,
    Banner,
}

/// A message a plugin wants to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub place: Place,
    pub level: Level,
    pub title: String,
    pub detail: String,
}

/// A target folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A folder of Settings > Target Folders, by id.
    Folder(u32),
    /// Any folder on disk.
    Path(PathBuf),
}

/// What RustCut should do after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `mark`: whether RustCut shows the plugin's mark at the folder field (off after the host's `hide-marks`).
    SetTarget {
        target: Target,
        reason: String,
        mark: bool,
    },
    /// `mark`: whether RustCut shows the plugin's mark at the rating (off after the host's `hide-marks`).
    SetRating {
        stars: u8,
        reason: String,
        mark: bool,
    },
    Notify(Notice),
}

/// A finding about settings the user is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub field: String,
    pub row: Option<u32>,
    pub level: Level,
    pub message: String,
}

/// A line a plugin wrote: through the host's `log`, or on stderr (a panic's message lands there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}
