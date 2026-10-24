//! Settings (`config.toml`) and UI state (`ui-state.toml`): typed schemas with defaults, sanitizing, versioned migrations
//! with an automatic backup first, and atomic writes (ADR-5, CLAUDE.md rule 9).

mod dialog_ui;
mod dialogs;
mod file;
mod notices;
mod paths;
mod schema;
#[cfg(test)]
mod schema_tests;
mod ui_state;
#[cfg(test)]
mod ui_state_tests;
mod unknown;

pub use dialog_ui::{DialogSize, DialogUiState};
pub use dialogs::{ConflictDialogSettings, ConflictPreselect, DialogSettings, PictureScope, TileSize};
pub use file::{FileError, LoadOutcome, MigrationStep, VersionedFile, backup_file, load, save};
pub use notices::{
    BannerAnimation, BannerAttention, BannerSettings, BellBadge, BubbleSettings, DesktopNoticeSettings,
    HistorySettings, KindRoute, NoticePlace, NoticeSettings, ToastAnimation, ToastOrder, ToastPosition, ToastSettings,
    ToastStacking,
};
pub use paths::AppPaths;
pub use schema::{
    AdvancedSettings, AppearanceSettings, AutoSection, DesktopSettings, DraftOnLoad, EditorSettings, ExportSettings,
    FilterDefaults, GeneralSettings, HashingSettings, PlayerSettings, QueueSettings, RenderSettings, RustViewSettings,
    Settings, TargetFolder, TraySettings, WarningSettings, WhenDone, WriteTiming,
};
pub use ui_state::{
    ExportPanelState, JobPanelState, LogPanelState, LogSource, ProbeState, QueuePanelState, SettingsWindowState,
    TagPanelState, TimelineState, UiState, WindowState,
};
pub use unknown::unknown_keys;
