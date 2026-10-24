//! `ui-state.toml`: what the user arranged in the window (CLAUDE.md rule 7). Written when it changes (debounced by the app) and
//! on quit; restored before the first frame.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, VecSkipError, serde_as};

use super::file::VersionedFile;
use crate::edit::{CropAspect, FilterChain};
use crate::job::{JobId, JobStateKind};
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
    /// The Job panel (the inspector of the queue's current job). A value this version cannot read counts as missing.
    #[serde_as(as = "DefaultOnError")]
    pub job: JobPanelState,
    /// The Logs panel. A value this version cannot read counts as missing.
    #[serde_as(as = "DefaultOnError")]
    pub logs: LogPanelState,
    /// Path of the last opened video's folder (the file dialog starts there).
    pub last_open_dir: Option<std::path::PathBuf>,
    /// The Settings window. A value this version cannot read counts as missing.
    #[serde_as(as = "DefaultOnError")]
    pub settings: SettingsWindowState,
    /// The dialogs' sizes and the conflict dialog's folded groups (0.0.20). A value this version cannot read counts as
    /// missing.
    #[serde_as(as = "DefaultOnError")]
    pub dialogs: super::dialog_ui::DialogUiState,
}

/// The Settings window (M4.2): its size, the page it showed last and what each page had selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsWindowState {
    /// Logical pixels.
    pub width: u32,
    pub height: u32,
    /// The page's id (`general`, `hotkeys`, ...; an unknown id shows the first page).
    pub page: String,
    /// The profile selected on the Render Profiles page.
    pub profile: Option<String>,
    /// The Hotkeys page's filter.
    pub hotkey_search: String,
    /// The Messages page's tab (0 look, 1 kinds, 2 muted) and its search (0.0.26).
    pub messages_tab: u8,
    pub messages_search: String,
}

impl SettingsWindowState {
    pub const WIDTH: std::ops::RangeInclusive<u32> = 600..=16_384;
    pub const HEIGHT: std::ops::RangeInclusive<u32> = 400..=16_384;
}

impl Default for SettingsWindowState {
    fn default() -> Self {
        Self {
            width: 900,
            height: 660,
            page: String::new(),
            profile: None,
            hotkey_search: String::new(),
            messages_tab: 0,
            messages_search: String::new(),
        }
    }
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
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueuePanelState {
    /// Processing was paused by the user.
    pub paused: bool,
    /// State filter chips that are on (empty = all). A state this version does not know is left out.
    #[serde_as(as = "DefaultOnError<VecSkipError<_>>")]
    pub state_filter: Vec<JobStateKind>,
    pub search: String,
    /// Column widths in logical pixels by column id.
    pub columns: std::collections::BTreeMap<String, u32>,
    /// One-time choice for this run of the queue (the default is `Settings.queue.when_done`).
    pub when_done: Option<WhenDone>,
    /// The selected jobs in the order they were selected (ids that are gone are dropped when the queue is known).
    #[serde_as(as = "DefaultOnError")]
    pub selected: Vec<JobId>,
    /// The job the Job and Logs panels show (the last one clicked).
    #[serde_as(as = "DefaultOnError")]
    pub current: Option<JobId>,
}

impl QueuePanelState {
    /// Most selected jobs that are remembered.
    pub const SELECTED_MAX: usize = 10_000;
    /// Longest search text that is remembered.
    pub const SEARCH_MAX: usize = 500;

    fn sanitize(&mut self, notes: &mut Vec<String>) {
        self.columns.retain(|_, width| *width >= 24 && *width <= 4_000);
        let mut seen = std::collections::HashSet::new();
        self.state_filter.retain(|k| seen.insert(*k));
        let mut seen = std::collections::HashSet::new();
        self.selected.retain(|id| seen.insert(*id));
        if self.selected.len() > Self::SELECTED_MAX {
            notes.push("queue selection shortened".into());
            self.selected.truncate(Self::SELECTED_MAX);
        }
        if self.search.chars().count() > Self::SEARCH_MAX {
            notes.push("queue search shortened".into());
            self.search = self.search.chars().take(Self::SEARCH_MAX).collect();
        }
    }
}

/// The Job panel: which of its sections are folded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobPanelState {
    /// Ids of the folded sections (`sections`, `filters`, `render`, ...; unknown ids are ignored by the app).
    pub collapsed: Vec<String>,
}

/// Which log the Logs panel shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    /// The stored log of the queue's current job.
    #[default]
    Job,
    /// RustCut's own log of this run.
    App,
    /// The lines of the RustView link and the Tag & Rate panel.
    #[serde(rename = "rustview")]
    RustView,
    /// The messages of this session (toasts, banners, the status line; M6.5).
    Messages,
}

/// The Logs panel's filter.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogPanelState {
    /// Only lines containing this text (case-insensitive).
    pub search: String,
    /// Only errors and warnings.
    pub errors_only: bool,
    /// Stay at the newest line.
    pub follow: bool,
    /// A value this version does not know reads as the job's log.
    #[serde_as(as = "DefaultOnError")]
    pub source: LogSource,
}

impl Default for LogPanelState {
    fn default() -> Self {
        Self {
            search: String::new(),
            errors_only: false,
            follow: true,
            source: LogSource::Job,
        }
    }
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
    /// Longest tag history kept (the largest `rustview.tag_history_size`).
    pub const HISTORY_MAX: usize = 100;
    /// The tag history's length when nothing else is set (RustView's default).
    pub const HISTORY_DEFAULT: usize = 20;
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
        self.queue.sanitize(&mut notes);
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
        let s = &mut self.settings;
        if !SettingsWindowState::WIDTH.contains(&s.width) || !SettingsWindowState::HEIGHT.contains(&s.height) {
            notes.push(format!("settings window size {}x{} reset", s.width, s.height));
            let fresh = SettingsWindowState::default();
            (s.width, s.height) = (fresh.width, fresh.height);
        }
        if s.messages_tab > 2 {
            notes.push(format!("settings messages tab {} reset", s.messages_tab));
            s.messages_tab = 0;
        }
        self.dialogs.sanitize(&mut notes);
        notes
    }
}
