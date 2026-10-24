//! `config.toml`: everything the Settings window changes. Every field has a default (`#[serde(default)]`), so files of
//! older versions load; renamed or reshaped keys need a [`super::MigrationStep`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_with::formats::PreferMany;
use serde_with::{DefaultOnError, OneOrMany, VecSkipError, serde_as};

use super::file::VersionedFile;
use crate::edit::{Deinterlacer, DeshakeParams};
use crate::naming::DEFAULT_TEMPLATE;
use crate::render::{DEFAULT_PROFILE_ID, RenderProfile, is_builtin_id};

#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub player: PlayerSettings,
    pub editor: EditorSettings,
    pub filters: FilterDefaults,
    pub export: ExportSettings,
    pub render: RenderSettings,
    pub queue: QueueSettings,
    pub rustview: RustViewSettings,
    pub warnings: WarningSettings,
    pub desktop: DesktopSettings,
    pub appearance: AppearanceSettings,
    pub advanced: AdvancedSettings,
    /// Key bindings by action id (`set_start = ["I", "Home"]`); a missing action keeps its default, an empty list unbinds
    /// it. Owned, validated and sanitized by the app's keymap (key names are Slint's); the core only stores it. A value
    /// that is neither a string nor a list of strings reads as missing, so a typo never resets the other settings.
    #[serde_as(as = "DefaultOnError<BTreeMap<_, DefaultOnError<Option<OneOrMany<_, PreferMany>>>>>")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub keymap: BTreeMap<String, Option<Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// A second start forwards its files to the running window.
    pub single_instance: bool,
    /// Ask before quitting while a job renders.
    pub confirm_quit_while_rendering: bool,
    /// How long a button or menu entry that deletes or discards something has to be held, in milliseconds (user
    /// decision Q-Q4); 0 = a click is enough.
    pub hold_to_confirm_ms: u16,
}

impl GeneralSettings {
    /// Shortest and longest hold besides 0.
    pub const HOLD_TO_CONFIRM_MS: std::ops::RangeInclusive<u16> = 300..=10_000;
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            single_instance: true,
            confirm_quit_while_rendering: true,
            hold_to_confirm_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// Start playing when a video opens.
    pub auto_play: bool,
    /// mpv `hwdec` (`auto-copy`, `no`, `vaapi`, ...).
    pub hwdec: String,
    /// Background around the video (`#RRGGBB`).
    pub background: String,
    /// Volume change per mouse wheel step on the video, in percent.
    pub wheel_volume_step: u8,
    /// Mute the last second of a video while previewing (against crackling at the end).
    pub mute_last_second: bool,
    /// Normalize the loudness of the preview audio (`loudnorm`; the render is not affected).
    pub loudnorm_preview: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            auto_play: true,
            hwdec: "auto-copy".into(),
            background: "#000000".into(),
            wheel_volume_step: 5,
            mute_last_second: false,
            loudnorm_preview: false,
        }
    }
}

/// Automatic section when saving a job without sections (PyCutRv "auto section").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoSection {
    /// Saving without sections is refused.
    #[default]
    Off,
    WholeVideo,
    /// From the current start mark to the current end mark.
    CurrentRange,
}

/// What happens to the editor's draft (the open video with its sections, marks, filters, settings, name and tags)
/// when a queued job is loaded into the editor (user decision Q-Q3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftOnLoad {
    /// The job replaces it without asking (PyCutRv).
    #[default]
    Replace,
    /// Ask first when the draft has sections or marks.
    Ask,
    /// Keep it aside and restore it when the editing of the job ends.
    SetAside,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub auto_section: AutoSection,
    /// Seconds for the short seek keys (arrow keys; PyCutRv: 2).
    pub seek_step_secs: u16,
    /// Seconds for the long seek keys (Shift + arrow keys; PyCutRv: 10).
    pub seek_step_long_secs: u16,
    /// A value this version does not know reads as the default.
    #[serde_as(as = "DefaultOnError")]
    pub draft_on_load: DraftOnLoad,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            auto_section: AutoSection::Off,
            seek_step_secs: 2,
            seek_step_long_secs: 10,
            draft_on_load: DraftOnLoad::Replace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterDefaults {
    /// Deinterlacer selected when the filter is switched on.
    pub deinterlacer: Deinterlacer,
    /// Defaults of the deshake filter.
    pub deshake: DeshakeParams,
}

impl Default for FilterDefaults {
    fn default() -> Self {
        Self {
            deinterlacer: Deinterlacer::Bwdif,
            deshake: DeshakeParams::default(),
        }
    }
}

/// A named target folder of the export panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetFolder {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    /// Named target folders in the order of the selection list.
    pub target_folders: Vec<TargetFolder>,
    /// File-name template (minijinja, see `naming`).
    pub name_template: String,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            target_folders: Vec::new(),
            name_template: DEFAULT_TEMPLATE.into(),
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderSettings {
    /// The user's own profiles (built-ins are not stored). A profile that cannot be read (a hand-edited typo) is skipped
    /// and dropped at the next save, so one broken entry never resets the other settings.
    #[serde_as(as = "DefaultOnError<VecSkipError<_>>")]
    pub profiles: Vec<RenderProfile>,
    /// Profile selected for new jobs.
    pub default_profile: String,
    /// Niceness of ffmpeg (0 normal to 19 lowest).
    pub nice: u8,
    /// Idle I/O priority for ffmpeg (the desktop stays responsive while rendering).
    pub idle_io: bool,
    /// A render without progress for this long is stopped as stalled (0 = never).
    pub stall_timeout_secs: u32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            default_profile: DEFAULT_PROFILE_ID.into(),
            nice: 10,
            idle_io: true,
            stall_timeout_secs: 300,
        }
    }
}

/// What happens when the queue has rendered its last job.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhenDone {
    #[default]
    Nothing,
    Notify,
    Quit,
    Suspend,
    PowerOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSettings {
    /// Jobs rendered at the same time (1 = one after another).
    pub workers: u8,
    /// Start with processing paused when jobs are waiting.
    pub pause_on_start: bool,
    pub when_done: WhenDone,
    /// Countdown before quit, suspend or power off, in seconds (can be aborted).
    pub when_done_countdown_secs: u8,
}

impl Default for QueueSettings {
    fn default() -> Self {
        Self {
            workers: 1,
            pause_on_start: false,
            when_done: WhenDone::Nothing,
            when_done_countdown_secs: 5,
        }
    }
}

/// When tags and rating reach RustView (ADR-6, user Q3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteTiming {
    /// When the job is queued (a pending stub that RustView completes once the file exists).
    #[default]
    AtEnqueue,
    /// Only after the render was verified.
    AfterRender,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RustViewSettings {
    pub enabled: bool,
    /// The `rustview` binary; `None` = found in `PATH`.
    pub binary: Option<PathBuf>,
    pub write_timing: WriteTiming,
    /// Seconds a `rustview api` call may take.
    pub timeout_secs: u8,
    /// Tags the Tag & Rate panel's history keeps (RustView's default: 20).
    pub tag_history_size: u8,
}

impl RustViewSettings {
    /// Smallest and largest tag history.
    pub const TAG_HISTORY: std::ops::RangeInclusive<u8> = 1..=100;
}

impl Default for RustViewSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            binary: None,
            write_timing: WriteTiming::AtEnqueue,
            timeout_secs: 10,
            tag_history_size: 20,
        }
    }
}

/// The confirmations and warnings the user can switch off (PyCutRv's five, plus RustCut's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WarningSettings {
    /// Ask when the target file exists. Off: the job takes the next free name without asking (the next counter
    /// number when the counter is on, else "Name (n)"); an existing file is never replaced unasked.
    pub target_exists: bool,
    /// Ask when another job in the queue writes the same file. Off: the job is saved anyway (the queue never renders
    /// two jobs with the same target at once).
    pub target_queued: bool,
    /// Show the files in the target folder whose names start with the typed name (a hint, never a question).
    pub base_name_in_folder: bool,
    pub no_tags: bool,
    pub no_rating: bool,
    pub known_source: bool,
}

impl Default for WarningSettings {
    fn default() -> Self {
        Self {
            target_exists: true,
            target_queued: true,
            base_name_in_folder: true,
            no_tags: true,
            no_rating: true,
            known_source: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub job_finished: bool,
    pub job_failed: bool,
    pub queue_done: bool,
    /// Seconds a notification stays (0 = until dismissed).
    pub timeout_secs: u16,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            job_finished: false,
            job_failed: true,
            queue_done: true,
            timeout_secs: 8,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraySettings {
    pub enabled: bool,
    /// Minimizing the window while rendering hides it to the tray.
    pub minimize_to_tray: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    pub notifications: NotificationSettings,
    pub tray: TraySettings,
    /// Queue progress on the taskbar entry (Unity LauncherEntry).
    pub taskbar_progress: bool,
    /// Keep the system awake while rendering (logind inhibitor).
    pub inhibit_sleep: bool,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            notifications: NotificationSettings::default(),
            tray: TraySettings::default(),
            taskbar_progress: true,
            inhibit_sleep: true,
        }
    }
}

/// How the windows look and explain themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// Tooltips on buttons and options.
    pub tooltips: bool,
    /// How long the pointer rests before a tooltip shows, in milliseconds.
    pub tooltip_delay_ms: u16,
}

impl AppearanceSettings {
    /// Shortest and longest tooltip delay.
    pub const TOOLTIP_DELAY_MS: std::ops::RangeInclusive<u16> = 0..=5_000;
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            tooltips: true,
            tooltip_delay_ms: 600,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// `None` = found in `PATH`.
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    /// `tracing` filter (`info`, `debug`, `rustcut=debug,info`).
    pub log_level: String,
    /// Days of log files kept.
    pub log_keep_days: u16,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            ffmpeg: None,
            ffprobe: None,
            log_level: "info".into(),
            log_keep_days: 14,
        }
    }
}

impl VersionedFile for Settings {
    const FILE_NAME: &'static str = "config.toml";
    const VERSION: u32 = 1;

    fn sanitize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut fix = |note: &str, broken: bool| {
            if broken {
                notes.push(note.to_owned());
            }
            broken
        };
        if fix("queue.workers raised to 1", self.queue.workers == 0) {
            self.queue.workers = 1;
        }
        if fix("queue.workers limited to 16", self.queue.workers > 16) {
            self.queue.workers = 16;
        }
        if fix("render.nice limited to 19", self.render.nice > 19) {
            self.render.nice = 19;
        }
        if fix("advanced.log_keep_days raised to 1", self.advanced.log_keep_days == 0) {
            self.advanced.log_keep_days = 1;
        }
        if fix("rustview.timeout_secs raised to 1", self.rustview.timeout_secs == 0) {
            self.rustview.timeout_secs = 1;
        }
        let history = &mut self.rustview.tag_history_size;
        if fix(
            "rustview.tag_history_size brought into range",
            !RustViewSettings::TAG_HISTORY.contains(history),
        ) {
            *history = (*history).clamp(*RustViewSettings::TAG_HISTORY.start(), *RustViewSettings::TAG_HISTORY.end());
        }
        let delay = &mut self.appearance.tooltip_delay_ms;
        if fix(
            "appearance.tooltip_delay_ms limited to 5000",
            !AppearanceSettings::TOOLTIP_DELAY_MS.contains(delay),
        ) {
            *delay = *AppearanceSettings::TOOLTIP_DELAY_MS.end();
        }
        if fix(
            "player.wheel_volume_step limited to 1-25",
            !(1..=25).contains(&self.player.wheel_volume_step),
        ) {
            self.player.wheel_volume_step = self.player.wheel_volume_step.clamp(1, 25);
        }
        if fix(
            "editor.seek_step_secs limited to 1-3600",
            !(1..=3_600).contains(&self.editor.seek_step_secs),
        ) {
            self.editor.seek_step_secs = self.editor.seek_step_secs.clamp(1, 3_600);
        }
        if fix(
            "editor.seek_step_long_secs limited to 1-3600",
            !(1..=3_600).contains(&self.editor.seek_step_long_secs),
        ) {
            self.editor.seek_step_long_secs = self.editor.seek_step_long_secs.clamp(1, 3_600);
        }
        let hold = self.general.hold_to_confirm_ms;
        if fix(
            "general.hold_to_confirm_ms brought into range",
            hold != 0 && !GeneralSettings::HOLD_TO_CONFIRM_MS.contains(&hold),
        ) {
            self.general.hold_to_confirm_ms = hold.clamp(
                *GeneralSettings::HOLD_TO_CONFIRM_MS.start(),
                *GeneralSettings::HOLD_TO_CONFIRM_MS.end(),
            );
        }
        if fix(
            "export.name_template was empty",
            self.export.name_template.trim().is_empty(),
        ) {
            self.export.name_template = DEFAULT_TEMPLATE.into();
        }
        let deshake = self.filters.deshake.sanitized();
        if fix("filters.deshake brought into range", deshake != self.filters.deshake) {
            self.filters.deshake = deshake;
        }
        let before = self.render.profiles.len();
        let mut seen = std::collections::HashSet::new();
        self.render
            .profiles
            .retain(|p| !is_builtin_id(&p.id) && seen.insert(p.id.clone()));
        fix(
            "render.profiles: duplicate or reserved ids dropped",
            self.render.profiles.len() != before,
        );
        let known = is_builtin_id(&self.render.default_profile)
            || self.render.profiles.iter().any(|p| p.id == self.render.default_profile);
        if fix("render.default_profile was unknown", !known) {
            self.render.default_profile = DEFAULT_PROFILE_ID.into();
        }
        notes
    }
}
