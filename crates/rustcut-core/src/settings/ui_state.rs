//! `ui-state.toml`: what the user arranged in the window (CLAUDE.md rule 7). Written when it changes (debounced by the app) and
//! on quit; restored before the first frame.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

use super::file::VersionedFile;
use crate::edit::{CropAspect, FilterChain};
use crate::render::{DEFAULT_PROBE_SECS, PROBE_SECS, RenderProfile};
use crate::settings::WhenDone;

#[serde_as]
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
    /// The Tag & Rate panel. A value this version cannot read counts as missing (the other parts stay).
    #[serde_as(as = "DefaultOnError")]
    pub tags: TagPanelState,
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
#[serde_as]
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
    /// Keep the filters for the next video (and the next start).
    pub keep_filters: bool,
    /// The filter chain kept for the next video: written only while `keep_filters` is on. A value this version cannot
    /// read counts as missing (the other settings stay).
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<FilterChain>,
    /// The aspect ratio a crop keeps while it is dragged on the video.
    #[serde_as(as = "DefaultOnError")]
    pub crop_aspect: CropAspect,
    /// Render profile of the next job (id): the profile the Render Settings panel's working copy came from.
    pub profile: Option<String>,
    /// The Render Settings panel's working copy (the last used settings; user decision Q-E4). A value this version
    /// cannot read counts as missing (the panel starts from the profile).
    #[serde_as(as = "DefaultOnError")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderProfile>,
    /// The probe render's switches.
    #[serde_as(as = "DefaultOnError")]
    pub probe: ProbeState,
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
            filters: None,
            crop_aspect: CropAspect::Free,
            profile: None,
            render: None,
            probe: ProbeState::default(),
            muted: false,
            volume: 100,
        }
    }
}

/// The probe render's switches in the Render Settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeState {
    /// Length in seconds.
    pub seconds: u16,
    /// Render with the profile's own speed (and estimate the job's size) instead of the fastest one.
    pub exact: bool,
}

impl Default for ProbeState {
    fn default() -> Self {
        Self {
            seconds: DEFAULT_PROBE_SECS,
            exact: false,
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

/// The Tag & Rate panel (M3.2): the tree's expanded branches, the tag filter, the tag history, hidden branches, locked
/// tags and what the last saved job had. Tag ids are RustView's; ids RustView no longer knows are dropped by the app once
/// the tag tree is loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagPanelState {
    pub expanded: Vec<u32>,
    pub filter: String,
    pub regex: bool,
    /// Recently used tags, newest first (at most [`TagPanelState::HISTORY_MAX`]).
    pub history: Vec<u32>,
    pub history_collapsed: bool,
    /// Height of the tag history's body in logical pixels.
    pub history_height: u32,
    /// Branches hidden from the tree (the tag and everything below it).
    pub hidden: Vec<u32>,
    /// Tags that stay assigned from one job to the next (right click, user decision Q-G2).
    pub locked: Vec<u32>,
    /// Tags and rating of the last saved job ("apply last tags", "last rating").
    pub last_tags: Vec<u32>,
    pub last_rating: Option<u8>,
}

impl TagPanelState {
    /// Longest tag history (RustView's default).
    pub const HISTORY_MAX: usize = 20;
    /// Smallest and largest height of the tag history's body (RustView's limits).
    pub const HISTORY_HEIGHT: std::ops::RangeInclusive<u32> = 40..=500;

    fn sanitize(&mut self, notes: &mut Vec<String>) {
        for list in [
            &mut self.expanded,
            &mut self.history,
            &mut self.hidden,
            &mut self.locked,
            &mut self.last_tags,
        ] {
            let mut seen = std::collections::HashSet::new();
            list.retain(|id| seen.insert(*id));
        }
        if self.history.len() > Self::HISTORY_MAX {
            notes.push(format!("tag history shortened to {}", Self::HISTORY_MAX));
            self.history.truncate(Self::HISTORY_MAX);
        }
        if !Self::HISTORY_HEIGHT.contains(&self.history_height) {
            notes.push(format!("tag history height {} brought into range", self.history_height));
            self.history_height = self
                .history_height
                .clamp(*Self::HISTORY_HEIGHT.start(), *Self::HISTORY_HEIGHT.end());
        }
        if self.last_rating.is_some_and(|r| r > 5) {
            notes.push("last rating dropped".into());
            self.last_rating = None;
        }
    }
}

impl Default for TagPanelState {
    fn default() -> Self {
        Self {
            expanded: Vec::new(),
            filter: String::new(),
            regex: false,
            history: Vec::new(),
            history_collapsed: false,
            history_height: 96,
            hidden: Vec::new(),
            locked: Vec::new(),
            last_tags: Vec::new(),
            last_rating: None,
        }
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
        let seconds = self.export.probe.seconds;
        if !PROBE_SECS.contains(&seconds) {
            notes.push(format!("probe length {seconds} s brought into range"));
            self.export.probe.seconds = seconds.clamp(*PROBE_SECS.start(), *PROBE_SECS.end());
        }
        let zoom = self.timeline.zoom;
        if !zoom.is_finite() || !(1.0..=TimelineState::MAX_ZOOM).contains(&zoom) {
            notes.push(format!("timeline zoom {zoom} reset"));
            self.timeline.zoom = if zoom.is_finite() {
                zoom.clamp(1.0, TimelineState::MAX_ZOOM)
            } else {
                1.0
            };
        }
        self.tags.sanitize(&mut notes);
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
    fn the_tag_panel_round_trips_is_sanitized_and_old_files_load() {
        let dir = tempfile::tempdir().unwrap();
        let state = UiState {
            tags: TagPanelState {
                expanded: vec![1, 2],
                filter: "be.*".into(),
                regex: true,
                history: vec![5, 3],
                history_collapsed: true,
                history_height: 120,
                hidden: vec![9],
                locked: vec![3],
                last_tags: vec![3, 5],
                last_rating: Some(4),
            },
            ..UiState::default()
        };
        save(dir.path(), &state).unwrap();
        assert_eq!(
            load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
            state
        );

        let mut bad = UiState::default();
        bad.tags.history = (0..30).chain([1, 1]).collect();
        bad.tags.locked = vec![4, 4, 2];
        bad.tags.history_height = 9;
        bad.tags.last_rating = Some(9);
        assert_eq!(bad.sanitize().len(), 3);
        assert_eq!(bad.tags.history.len(), TagPanelState::HISTORY_MAX);
        assert_eq!(bad.tags.locked, vec![4, 2]);
        assert_eq!((bad.tags.history_height, bad.tags.last_rating), (40, None));

        // A file of 0.0.11 has no `[tags]`; a `[tags]` this version cannot read keeps the rest of the file.
        for tags in ["", "[tags]\nhistory = \"many\"\n"] {
            std::fs::write(
                dir.path().join("ui-state.toml"),
                format!("version = 1\n[timeline]\nzoom = 4.0\n{tags}"),
            )
            .unwrap();
            let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
            assert_eq!((read.timeline.zoom, read.tags.clone()), (4.0, TagPanelState::default()));
        }
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
    fn kept_filters_round_trip_and_bad_values_read_as_missing() {
        use crate::edit::{Crop, Filter, FilterKind, Rotation};
        let dir = tempfile::tempdir().unwrap();
        let mut state = UiState::default();
        let mut chain = FilterChain::default();
        chain.set(Filter::Crop(Crop::new(2, 4, 6, 8)));
        chain.set_enabled(FilterKind::Crop, true);
        chain.set(Filter::Rotate {
            rotation: Rotation::CounterClockwise,
        });
        chain.move_up(FilterKind::Rotate);
        state.export.keep_filters = true;
        state.export.filters = Some(chain);
        state.export.crop_aspect = CropAspect::Wide;
        save(dir.path(), &state).unwrap();
        let text = std::fs::read_to_string(dir.path().join("ui-state.toml")).unwrap();
        assert!(text.contains("crop_aspect = \"16:9\""), "{text}");
        assert_eq!(
            load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
            state
        );

        // A file of 0.0.6 has neither field; a broken chain or aspect does not reset the rest.
        std::fs::write(
            dir.path().join("ui-state.toml"),
            "version = 1\n[export]\nvolume = 70\nkeep_filters = true\ncrop_aspect = \"7:3\"\nfilters = 5\n",
        )
        .unwrap();
        let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
        assert_eq!(value.export.volume, 70);
        assert!(value.export.keep_filters);
        assert_eq!(value.export.filters, None);
        assert_eq!(value.export.crop_aspect, CropAspect::Free);
    }

    #[test]
    fn the_render_working_copy_and_probe_switches() {
        use crate::render::{VideoEncoder, builtin_profiles};
        let dir = tempfile::tempdir().unwrap();
        let mut state = UiState::default();
        let mut working = builtin_profiles().remove(0);
        working.set_encoder(VideoEncoder::X264);
        state.export.profile = Some("av1-balanced".into());
        state.export.render = Some(working);
        state.export.probe = ProbeState {
            seconds: 20,
            exact: true,
        };
        save(dir.path(), &state).unwrap();
        assert_eq!(
            load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
            state
        );

        // 0.0.8 wrote neither; a broken copy or switch does not reset the rest.
        std::fs::write(
            dir.path().join("ui-state.toml"),
            "version = 1\n[export]\nvolume = 60\nprofile = \"x265-quality\"\nprobe = 3\n\
             [export.render]\nid = 1\n",
        )
        .unwrap();
        let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
        assert_eq!(value.export.volume, 60);
        assert_eq!(value.export.profile.as_deref(), Some("x265-quality"));
        assert_eq!(value.export.render, None);
        assert_eq!(value.export.probe, ProbeState::default());

        let mut state = UiState::default();
        state.export.probe.seconds = 600;
        assert_eq!(state.sanitize().len(), 1);
        assert_eq!(state.export.probe.seconds, 60);
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
