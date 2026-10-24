//! The catalog of message kinds (M6.5, user decisions Q-N8 and Q-N10): every message RustCut shows has a kind with its
//! id in `config.toml`, a title for the Settings window, its level, where it appears by default, where it may appear,
//! whether it also goes to the desktop, and whether it is critical (never muted, its place fixed).

use rustcut_core::settings::NoticePlace;

/// Severity of a message: its color, its icon and how long a toast stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

impl Level {
    /// Hints and confirmations; only these can be muted (Q-N8).
    pub const fn is_quiet(self) -> bool {
        matches!(self, Self::Info | Self::Success)
    }

    /// The number the Slint side uses (0 info, 1 success, 2 warning, 3 error).
    pub const fn index(self) -> i32 {
        self as i32
    }
}

/// The area a kind belongs to (the groups of the Settings window's list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Saving,
    Source,
    Sections,
    Filters,
    Probe,
    Codecs,
    Queue,
    Editing,
    Tags,
    Settings,
    Logs,
    App,
}

impl Group {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Saving => "Saving",
            Self::Source => "Source",
            Self::Sections => "Sections",
            Self::Filters => "Filters",
            Self::Probe => "Probe render",
            Self::Codecs => "Codecs",
            Self::Queue => "Queue",
            Self::Editing => "Editing jobs",
            Self::Tags => "Tags",
            Self::Settings => "Settings",
            Self::Logs => "Logs",
            Self::App => "RustCut",
        }
    }
}

/// What the catalog knows about a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindInfo {
    /// The key in `config.toml` (`[notices.kinds.<id>]`, `muted`).
    pub id: &'static str,
    pub title: &'static str,
    pub group: Group,
    /// The usual level; a single message may have another one (an auto-crop that found nothing is a hint).
    pub level: Level,
    /// Where it appears unless the user chose another place.
    pub place: NoticePlace,
    /// The places the user can choose (the default among them).
    pub places: &'static [NoticePlace],
    /// Whether it also goes to the desktop unless the user chose otherwise.
    pub desktop: bool,
    /// Never muted, always at its default place (Q-N10).
    pub critical: bool,
    /// A title and a detail for the Settings window's test button.
    pub sample: (&'static str, &'static str),
}

use NoticePlace::{Banner, Nothing, Status, Toast};

const EVENT: &[NoticePlace] = &[Toast, Banner, Status, Nothing];
const LOUD: &[NoticePlace] = &[Toast, Banner, Nothing];
const QUIET: &[NoticePlace] = &[Status, Toast, Nothing];
const STATE: &[NoticePlace] = &[Banner, Nothing];
const FIXED_BANNER: &[NoticePlace] = &[Banner];

macro_rules! kinds {
    ($( $(#[$doc:meta])* $variant:ident = $id:literal, $title:literal, $group:ident, $level:ident, $place:ident,
       $places:expr, desktop $desktop:literal, critical $critical:literal, ($st:literal, $sd:literal); )*) => {
        /// A kind of message (the catalog's rows, in the Settings window's order).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Kind {
            $( $(#[$doc])* $variant, )*
        }

        impl Kind {
            /// Every kind, in the order of the Settings window.
            pub const ALL: &'static [Self] = &[$( Self::$variant, )*];

            pub const fn info(self) -> &'static KindInfo {
                match self {
                    $( Self::$variant => &KindInfo {
                        id: $id,
                        title: $title,
                        group: Group::$group,
                        level: Level::$level,
                        place: $place,
                        places: $places,
                        desktop: $desktop,
                        critical: $critical,
                        sample: ($st, $sd),
                    }, )*
                }
            }
        }
    };
}

kinds! {
    JobQueued = "job_queued", "Job queued", Saving, Success, Toast, EVENT, desktop false, critical false,
        ("Job 3 queued", "basic_h264_720p - 03.mkv");
    JobUpdated = "job_updated", "Edited job updated", Saving, Success, Toast, EVENT, desktop false, critical false,
        ("Job 3 updated", "basic_h264_720p - 03.mkv");
    /// The job cannot be saved as it is (render settings error, copied audio with several sections): a state.
    SaveBlocked = "save_blocked", "The job cannot be saved", Saving, Error, Banner, STATE, desktop true,
        critical false, ("The job cannot be saved", "Copied audio needs one section.");
    /// Save was pressed while the job cannot be saved (no sections, no folder, an unusable name).
    SaveRefused = "save_refused", "Save not possible yet", Saving, Warning, Banner, LOUD, desktop false,
        critical false, ("The job was not saved", "Set a section start and end first.");
    SaveFailed = "save_failed", "Save failed", Saving, Error, Banner, FIXED_BANNER, desktop true, critical true,
        ("The job was not saved", "The target folder could not be checked: permission denied");
    SaveCanceled = "save_canceled", "Save canceled", Saving, Info, Status, QUIET, desktop false, critical false,
        ("Not saved", "The conflict dialog was canceled.");
    Ready = "ready", "File opened", Source, Info, Status, QUIET, desktop false, critical false, ("Ready", "");
    OpenFailed = "open_failed", "File cannot be opened", Source, Error, Banner, FIXED_BANNER, desktop true,
        critical true, ("Cannot open clip.mp4", "Invalid data found when processing input");
    PlaybackFailed = "playback_failed", "File does not play", Source, Error, Banner, FIXED_BANNER, desktop true,
        critical true, ("The file cannot be played", "The video could not be decoded.");
    KnownSource = "known_source", "Source cut before", Source, Warning, Banner, LOUD, desktop false, critical false,
        ("This video was cut before", "First cut on 2026-10-12. 3 files in the library were made from it.");
    SectionAdded = "section_added", "Section added", Sections, Info, Status, QUIET, desktop false, critical false,
        ("Section 2 added", "");
    SectionRefused = "section_refused", "Section not possible", Sections, Warning, Toast, EVENT, desktop false,
        critical false, ("Set a section start and end first", "");
    /// A filter action that needs an open video (M6.5e disables them instead).
    NoVideo = "no_video", "Needs an open video", Filters, Warning, Toast, EVENT, desktop false, critical false,
        ("The crop can be edited once a video is open", "");
    FiltersReset = "filters_reset", "Filters reset", Filters, Info, Status, QUIET, desktop false, critical false,
        ("Filters reset", "");
    AutoCrop = "auto_crop", "Black borders cropped", Filters, Success, Toast, EVENT, desktop false, critical false,
        ("Black borders cropped", "140 pixels at the top and bottom");
    AutoCropFailed = "auto_crop_failed", "Black border detection failed", Filters, Error, Toast, EVENT,
        desktop true, critical false, ("Black borders not detected", "ffmpeg ended with exit code 1");
    /// The probe render's progress, then its result (Q-N10: toast, banner or nothing; the card always shows it).
    ProbeRender = "probe_render", "Probe render", Probe, Info, Toast, LOUD, desktop false, critical false,
        ("Probe render", "10 s around 0:06 · 42 %");
    ProbeFailed = "probe_failed", "Probe render failed", Probe, Error, Toast, EVENT, desktop true, critical false,
        ("The probe render failed", "The encoder refused the settings.");
    /// The player shows the probe in a loop: the way back (a state).
    ProbePlaying = "probe_playing", "Probe render playing", Probe, Info, Banner, STATE, desktop false,
        critical false, ("Probe render: 10 s around 0:06", "AV1 balanced · the timeline waits; Esc returns");
    ProbeLocked = "probe_locked", "Editing waits for the probe", Probe, Info, Status, QUIET, desktop false,
        critical false, ("The probe render is playing", "Esc returns to the source.");
    ProfileChanged = "profile_changed", "Render profile saved", Codecs, Success, Toast, EVENT, desktop false,
        critical false, ("Profile \"My HEVC\" saved", "");
    ProfileRefused = "profile_refused", "Render profile not changed", Codecs, Warning, Toast, EVENT, desktop false,
        critical false, ("A profile with this name exists", "");
    SettingsReadOnly = "settings_read_only", "Settings not saved", Settings, Warning, Toast, EVENT, desktop false,
        critical false, ("Not saved", "config.toml is from a newer RustCut: the choice is kept for this run only.");
    SettingsWindowFailed = "settings_window_failed", "Settings window failed", Settings, Error, Toast, EVENT,
        desktop true, critical false, ("The settings window could not be opened", "No OpenGL context");
    JobFinished = "job_finished", "Job finished", Queue, Success, Nothing, &[Toast, Nothing], desktop false,
        critical false, ("Job finished", "basic_h264_720p - 03.mkv in /videos/out");
    JobFailed = "job_failed", "Job failed", Queue, Error, Toast, LOUD, desktop true, critical false,
        ("Job failed", "basic_h264_720p - 03.mkv: the encoder refused the settings");
    QueueDone = "queue_done", "Queue done", Queue, Success, Toast, &[Toast, Nothing], desktop true, critical false,
        ("The queue is done", "3 jobs finished");
    /// "Power off in 5 s" after the queue (also suspend and quit; Q-N8: default the desktop only).
    PowerCountdown = "power_countdown", "Countdown after the queue", Queue, Warning, Nothing, LOUD, desktop true,
        critical false, ("Power off in 5 s", "The queue is done. Esc aborts.");
    PowerRefused = "power_refused", "Power off refused", Queue, Error, Toast, LOUD, desktop true, critical false,
        ("The system refused", "Not authorized");
    PowerChoice = "power_choice", "\"When done\" chosen", Queue, Info, Status, QUIET, desktop false, critical false,
        ("RustCut quits when the queue is done", "");
    JobLoadFailed = "job_load_failed", "Job cannot be loaded", Editing, Error, Toast, EVENT, desktop true,
        critical false, ("The job cannot be loaded into the editor", "Its spec is unreadable.");
    /// Loading, editing and leaving a job (Q-N8: no message by default).
    Editing = "editing", "Job loaded or released", Editing, Info, Nothing, QUIET, desktop false, critical false,
        ("Job 3 is in the editor", "Save updates it.");
    TagsUpdated = "tags_updated", "Tags and rating changed", Tags, Success, Toast, EVENT, desktop false,
        critical false, ("Tags and rating of \"Clip\" updated", "");
    TagsFailed = "tags_failed", "Tags not changed", Tags, Error, Toast, EVENT, desktop true, critical false,
        ("The job's tags were not changed", "RustView unavailable: not installed");
    LogCopied = "log_copied", "Log copied", Logs, Success, Toast, EVENT, desktop false, critical false,
        ("143 log lines copied", "");
    ClipboardFailed = "clipboard_failed", "Clipboard unusable", Logs, Warning, Toast, EVENT, desktop false,
        critical false, ("The clipboard cannot be used", "");
    /// The queue database cannot be used (M6.5d turns it into a dialog with ways out).
    DatabaseUnusable = "database_unusable", "Queue database unusable", App, Error, Banner, FIXED_BANNER,
        desktop true, critical true, ("The queue cannot be saved", "database disk image is malformed");
    ToolsMissing = "tools_missing", "ffmpeg or ffprobe missing", App, Error, Banner, FIXED_BANNER, desktop true,
        critical true, ("ffmpeg cannot be started", "No such file or directory: nothing can be rendered.");
}

impl Kind {
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.info().id == id)
    }

    pub const fn id(self) -> &'static str {
        self.info().id
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_catalog_is_consistent() {
        let mut ids = HashSet::new();
        for kind in Kind::ALL {
            let info = kind.info();
            assert!(ids.insert(info.id), "{} twice", info.id);
            assert_eq!(Kind::from_id(info.id), Some(*kind));
            assert!(
                info.places.contains(&info.place),
                "{}: its default place must be allowed",
                info.id
            );
            assert!(!info.title.is_empty() && !info.sample.0.is_empty(), "{}", info.id);
            assert!(
                info.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{}: config ids are snake_case",
                info.id
            );
            // Q-N8: errors also go to the desktop while RustCut is not in front.
            assert_eq!(
                info.desktop,
                info.level == Level::Error || matches!(kind, Kind::QueueDone | Kind::PowerCountdown),
                "{}",
                info.id
            );
            if info.critical {
                assert_eq!(info.places.len(), 1, "{}: a critical kind has its one place", info.id);
                assert_eq!(info.level, Level::Error, "{}", info.id);
            }
        }
        // The user's critical kinds (Q-N10); RustView and failed jobs are not critical.
        let critical: Vec<&str> = Kind::ALL.iter().filter(|k| k.info().critical).map(|k| k.id()).collect();
        assert_eq!(
            critical,
            [
                "save_failed",
                "open_failed",
                "playback_failed",
                "database_unusable",
                "tools_missing"
            ]
        );
        assert!(!Kind::JobFailed.info().critical);
    }
}
