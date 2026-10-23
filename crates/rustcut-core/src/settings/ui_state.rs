//! `ui-state.toml`: what the user arranged in the window (CLAUDE.md rule 7). Written when it changes (debounced by the app) and
//! on quit; restored before the first frame.

use serde::{Deserialize, Serialize};

use super::file::VersionedFile;
use crate::settings::WhenDone;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub window: WindowState,
    /// The dock layout (sidebars, panel groups, tabs, splits, collapsed and maximized states). Owned and sanitized by the
    /// app's `DockLayout` model (M1.2); the core only stores it.
    pub dock: Option<toml::Table>,
    pub export: ExportPanelState,
    pub queue: QueuePanelState,
    pub timeline: TimelineState,
    /// Path of the last opened video's folder (the file dialog starts there).
    pub last_open_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    /// Position of the window frame in physical screen pixels when known (X11); `None` lets the window manager place it.
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

/// Controls of the Export panel that keep their value between videos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportPanelState {
    /// Index into `Settings.export.target_folders`.
    pub target_folder: Option<usize>,
    /// The counter suffix is on and its value.
    pub counter_enabled: bool,
    pub counter: u32,
    /// Raise the counter after each save.
    pub auto_increment: bool,
    /// Clear the section list after a save.
    pub clear_sections_after_save: bool,
    /// Show the filters in the player.
    pub preview_filters: bool,
    /// Keep the filters for the next video.
    pub keep_filters: bool,
    /// Render profile of the next job (id).
    pub profile: Option<String>,
    pub muted: bool,
    /// Player volume in percent.
    pub volume: u8,
}

impl Default for ExportPanelState {
    fn default() -> Self {
        Self {
            target_folder: None,
            counter_enabled: false,
            counter: 1,
            auto_increment: true,
            clear_sections_after_save: true,
            preview_filters: true,
            keep_filters: false,
            profile: None,
            muted: false,
            volume: 100,
        }
    }
}

/// The queue panel's view settings and its processing switches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueuePanelState {
    /// Processing was paused by the user.
    pub paused: bool,
    /// State filter chips that are on (empty = all).
    pub state_filter: Vec<crate::job::JobStateKind>,
    pub search: String,
    /// Column widths in logical pixels by column id.
    pub columns: std::collections::BTreeMap<String, u32>,
    /// One-time choice for this run of the queue (the default is `Settings.queue.when_done`).
    pub when_done: Option<WhenDone>,
}

/// The timeline under the video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineState {
    /// Zoom factor: the visible part is the duration divided by it (1 = the whole video).
    pub zoom: f64,
}

impl TimelineState {
    /// Largest zoom factor that is stored (the app limits it further by the shortest visible span).
    pub const MAX_ZOOM: f64 = 100_000.0;
}

impl Default for TimelineState {
    fn default() -> Self {
        Self { zoom: 1.0 }
    }
}

/// Window positions beyond this (physical pixels, any direction) are not on any screen.
const MAX_POSITION: i32 = 32_768;

impl VersionedFile for UiState {
    const FILE_NAME: &'static str = "ui-state.toml";
    const VERSION: u32 = 1;

    fn sanitize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let w = &mut self.window;
        if !(640..=16_384).contains(&w.width) || !(400..=16_384).contains(&w.height) {
            notes.push(format!("window size {}x{} reset", w.width, w.height));
            *w = WindowState {
                x: w.x,
                y: w.y,
                maximized: w.maximized,
                ..WindowState::default()
            };
        }
        let far = |v: Option<i32>| v.is_some_and(|v| !(-MAX_POSITION..=MAX_POSITION).contains(&v));
        if far(self.window.x) || far(self.window.y) || self.window.x.is_some() != self.window.y.is_some() {
            notes.push("window position reset".into());
            (self.window.x, self.window.y) = (None, None);
        }
        if self.export.volume > 150 {
            notes.push("volume limited to 150".into());
            self.export.volume = 150;
        }
        self.queue.columns.retain(|_, width| *width >= 24 && *width <= 4_000);
        let zoom = self.timeline.zoom;
        if !zoom.is_finite() || !(1.0..=TimelineState::MAX_ZOOM).contains(&zoom) {
            notes.push(format!("timeline zoom {zoom} reset"));
            self.timeline.zoom = if zoom.is_finite() {
                zoom.clamp(1.0, TimelineState::MAX_ZOOM)
            } else {
                1.0
            };
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{load, save};

    #[test]
    fn round_trip_with_an_opaque_dock_table() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = UiState::default();
        let mut dock = toml::Table::new();
        dock.insert("left_width".into(), toml::Value::Integer(320));
        state.dock = Some(dock);
        state.queue.state_filter = vec![crate::job::JobStateKind::Failed];
        state.queue.columns.insert("name".into(), 240);
        save(dir.path(), &state).unwrap();
        assert_eq!(
            load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
            state
        );
    }

    #[test]
    fn absurd_values_are_reset() {
        let mut state = UiState::default();
        state.window.width = 5;
        state.window.maximized = true;
        state.export.volume = 255;
        state.queue.columns.insert("x".into(), 1);
        assert_eq!(state.sanitize().len(), 2);
        assert_eq!((state.window.width, state.window.maximized), (1280, true));
        assert!(state.queue.columns.is_empty());
    }

    #[test]
    fn the_timeline_zoom_is_kept_in_range_and_old_files_load() {
        let mut state = UiState::default();
        state.timeline.zoom = 0.2;
        assert_eq!(state.sanitize().len(), 1);
        assert_eq!(state.timeline.zoom, 1.0);
        state.timeline.zoom = 1e9;
        state.sanitize();
        assert_eq!(state.timeline.zoom, TimelineState::MAX_ZOOM);
        state.timeline.zoom = f64::NAN;
        state.sanitize();
        assert_eq!(state.timeline.zoom, 1.0);

        // A file of 0.0.5 has no `[timeline]`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("ui-state.toml"),
            "version = 1\n[window]\nwidth = 1000\n",
        )
        .unwrap();
        let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
        assert_eq!((value.window.width, value.timeline.zoom), (1000, 1.0));
    }

    #[test]
    fn positions_off_every_screen_or_half_known_are_dropped() {
        let mut state = UiState::default();
        (state.window.x, state.window.y) = (Some(-40), Some(1200));
        assert!(state.sanitize().is_empty());
        state.window.x = Some(900_000);
        assert_eq!(state.sanitize(), vec!["window position reset".to_owned()]);
        assert_eq!((state.window.x, state.window.y), (None, None));
        state.window.y = Some(10);
        assert_eq!(state.sanitize().len(), 1);
        assert_eq!(state.window.y, None);
    }
}
