//! Settings (`config.toml`) and UI state (`ui-state.toml`): typed schemas with defaults, sanitizing, versioned migrations
//! with an automatic backup first, and atomic writes (ADR-5, CLAUDE.md rule 9).

mod file;
mod paths;
mod schema;
mod ui_state;

pub use file::{FileError, LoadOutcome, MigrationStep, VersionedFile, backup_file, load, save};
pub use paths::AppPaths;
pub use schema::{
    AdvancedSettings, AutoSection, DesktopSettings, EditorSettings, ExportSettings, FilterDefaults, GeneralSettings,
    NotificationSettings, PlayerSettings, QueueSettings, RenderSettings, RustViewSettings, Settings, TargetFolder,
    TraySettings, WarningSettings, WhenDone, WriteTiming,
};
pub use ui_state::{ExportPanelState, QueuePanelState, UiState, WindowState};
