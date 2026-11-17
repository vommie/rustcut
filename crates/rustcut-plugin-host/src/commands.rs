//! Plugin commands (API 1.2, the world `plugin-with-commands`): what RustCut hands a command call and what comes back,
//! on RustCut's side. The folder listing itself is RustCut's (`Navigator`); the sandbox only passes it through.

use std::path::PathBuf;
use std::sync::Arc;

use crate::types::{Action, Context};

/// When a file manager's order counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileManagerMode {
    Off,
    OpenTab,
    Running,
    Always,
}

/// Which hidden files count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenFiles {
    Skip,
    Show,
    LikeFileManager,
}

/// What a plugin asks to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderRequest {
    pub file_manager: FileManagerMode,
    pub hidden: HiddenFiles,
    pub videos_only: bool,
}

/// One file of the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFile {
    pub path: PathBuf,
    pub name: String,
    pub has_job: bool,
}

/// Where the order came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSource {
    FileManager,
    NamesOff,
    NamesNotRunning,
    NamesNotOpen,
    NamesUnsupported,
    NamesFailed,
}

/// The order that was used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderOrder {
    pub source: OrderSource,
    pub role: String,
    pub descending: bool,
    pub detail: String,
}

/// The open video's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderListing {
    pub files: Vec<FolderFile>,
    pub current: Option<u32>,
    pub order: FolderOrder,
}

/// Lists the open video's folder for a command (RustCut's side; called on the plugin's thread, may block briefly).
pub trait Navigator: Send + Sync {
    /// The files of `video`'s folder.
    ///
    /// # Errors
    /// The folder cannot be read; the text is handed to the plugin.
    fn list_folder(&self, video: &std::path::Path, request: FolderRequest) -> Result<FolderListing, String>;
}

/// A command to run.
#[derive(Clone)]
pub struct CommandCall {
    /// The command's `id` in `plugin.toml`.
    pub command: String,
    pub context: Context,
    /// Lists the folder when the plugin asks; `None` answers every `list-folder` with an error.
    pub navigator: Option<Arc<dyn Navigator>>,
}

impl std::fmt::Debug for CommandCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandCall")
            .field("command", &self.command)
            .field("context", &self.context)
            .field("navigator", &self.navigator.is_some())
            .finish()
    }
}

/// What a command asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutcome {
    pub actions: Vec<Action>,
    /// A video to open (the last `open-video` of the call).
    pub open: Option<PathBuf>,
    /// Bubbles to show, in order.
    pub bubbles: Vec<String>,
}
