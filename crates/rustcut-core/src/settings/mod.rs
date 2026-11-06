//! Settings (`config.toml`) and UI state (`ui-state.toml`): typed schemas with defaults, sanitizing, versioned migrations
//! with an automatic backup first, and atomic writes (ADR-5, see AGENTS.md).

mod dialog_ui;
mod dialogs;
mod file;
mod filters;
mod folders;
mod notices;
mod paths;
mod player;
mod plugins;
mod schema;
#[cfg(test)]
mod schema_tests;
mod thumbnails;
mod ui_state;
#[cfg(test)]
mod ui_state_tests;
mod unknown;
mod wheel;

pub use dialog_ui::{DialogSize, DialogUiState};
pub use dialogs::{ConflictDialogSettings, ConflictPreselect, DialogSettings, PictureScope, TileSize};
pub use file::{FileError, LoadOutcome, MigrationStep, VersionedFile, backup_file, load, save};
pub use filters::FilterDefaults;
pub use notices::{
    BannerAnimation, BannerAttention, BannerSettings, BellBadge, BubbleSettings, DesktopNoticeSettings,
    HistorySettings, KindRoute, NoticePlace, NoticeSettings, ToastAnimation, ToastOrder, ToastPosition, ToastSettings,
    ToastStacking,
};
pub use paths::AppPaths;
pub use player::{MAX_HIDE_POINTER_MS, MIN_HIDE_POINTER_MS, Normalization, PlayerSettings};
pub use plugins::{PLUGIN_MEMORY_MB, PLUGIN_TIME_LIMIT_MS, PluginEntry, PluginSettings};
pub use schema::{
    AdvancedSettings, AppearanceSettings, AutoSection, DesktopSettings, EditorSettings, ExportSettings,
    GeneralSettings, HashingSettings, OnJobLoad, QueueSettings, RenderSettings, RustViewSettings, Settings,
    TaskbarShows, TraySettings, WarningSettings, WhenDone, WriteTiming,
};
pub use thumbnails::{
    MAX_THUMB_DELAY_MS, MAX_THUMB_PORTRAIT_PERCENT, MAX_THUMB_WORKERS, MIN_THUMB_PORTRAIT_PERCENT, ThumbDensity,
    ThumbLook, ThumbMissing, ThumbPlace, ThumbPortrait, ThumbProgress, ThumbZone, ThumbnailSettings,
};
pub use ui_state::{
    ExportPanelState, JobPanelState, LogPanelState, LogSource, ProbeState, QueuePanelState, SettingsWindowState,
    TagPanelState, TimelineState, UiState, WindowState,
};
pub use unknown::unknown_keys;
pub use wheel::{
    ColorMode, FolderWheelSettings, PictureShape, SpokePictures, WheelButton, WheelLabels, WheelLevels, WheelMiddle,
    WheelOpening, WheelSize,
};
