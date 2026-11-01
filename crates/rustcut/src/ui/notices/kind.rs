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

use NoticePlace::{Activity, Banner, Bubble, Dialog, Nothing, Status, Toast};

const EVENT: &[NoticePlace] = &[Toast, Banner, Status, Nothing];
const LOUD: &[NoticePlace] = &[Toast, Banner, Nothing];
const QUIET: &[NoticePlace] = &[Status, Toast, Nothing];
/// The result of something the status bar showed running.
const ACTIVITY: &[NoticePlace] = &[Activity, Toast, Banner, Status, Nothing];
const STATE: &[NoticePlace] = &[Banner, Nothing];
const FIXED_BANNER: &[NoticePlace] = &[Banner];
/// At the input a check is about (M6.5c); hints and warnings can be switched off.
const BUBBLE: &[NoticePlace] = &[Bubble];
/// A dialog that needs an answer (M6.5d): its place never changes.
const FIXED_DIALOG: &[NoticePlace] = &[Dialog];
const BUBBLE_OR_NOT: &[NoticePlace] = &[Bubble, Nothing];
const TOAST_OR_NOT: &[NoticePlace] = &[Toast, Nothing];
/// A question before a job is saved: asked, or the safe way taken without asking (a `[warnings]` switch, 0.0.67).
const DIALOG_OR_NOT: &[NoticePlace] = &[Dialog, Nothing];

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
    /// The job cannot be saved as it is (render settings error, copied audio with several sections): a state.
    SaveBlocked = "save_blocked", "The job cannot be saved", Saving, Error, Banner, STATE, desktop true,
        critical false, ("The job cannot be saved", "Copied audio needs one section.");
    /// Save was pressed while the job cannot be saved (no sections, no folder, an unusable name).
    SaveRefused = "save_refused", "Save not possible yet", Saving, Warning, Banner, LOUD, desktop false,
        critical false, ("The job was not saved", "Set a section start and end first.");
    SaveFailed = "save_failed", "Save failed", Saving, Error, Banner, FIXED_BANNER, desktop true, critical true,
        ("The job was not saved", "The target folder could not be checked: permission denied");
    /// The export card's name check (M6.5c): files in the folder with similar names.
    TargetSimilar = "target_similar", "Similar names in the target folder", Saving, Info, Bubble, BUBBLE_OR_NOT,
        desktop false, critical false,
        ("2 similar filenames in target folder", "");
    /// The target file exists or a queued job writes it: Save asks what to do.
    TargetConflict = "target_conflict", "Target file exists or is queued", Saving, Warning, Bubble, BUBBLE_OR_NOT,
        desktop false, critical false, ("Target filename already exists",
        "If you do not fix it here, Save opens a dialog that helps you resolve it.");
    /// Save asks when the target file exists (`warnings.target_exists`); "Nothing" takes the next free name.
    AskTargetExists = "ask_target_exists", "Question: the target file exists", Saving, Warning, Dialog,
        DIALOG_OR_NOT, desktop false, critical false, ("The target file exists",
        "Save asks what to do. Nothing: the job gets the next free name; a file is never replaced unasked.");
    /// Save asks when a queued job writes the same file (`warnings.target_queued`); "Nothing" saves anyway.
    AskTargetQueued = "ask_target_queued", "Question: a queued job writes the same file", Saving, Warning, Dialog,
        DIALOG_OR_NOT, desktop false, critical false, ("A queued job writes the same file",
        "Save asks what to do. Nothing: saved anyway; the queue never renders both at the same time.");
    /// Save asks before a job without tags or rating is queued (`warnings.missing_metadata`, the Tag & Rate panel's
    /// "Warn" switch).
    AskMissingMetadata = "ask_missing_metadata", "Question: no tags or rating", Saving, Warning, Dialog,
        DIALOG_OR_NOT, desktop false, critical false, ("The job has no tags or rating",
        "Save asks first (tags only when RustView offers some). The same as the Tag & Rate panel's \"Warn\".");
    /// The name or the folder cannot be used: the job cannot be saved like this.
    TargetProblem = "target_problem", "Target cannot be used", Saving, Error, Bubble, BUBBLE, desktop false,
        critical false, ("The folder does not exist or cannot be read", "/videos/out");
    SaveCanceled = "save_canceled", "Save canceled", Saving, Info, Status, QUIET, desktop false, critical false,
        ("Not saved", "The conflict dialog was canceled.");
    Ready = "ready", "File opened", Source, Info, Status, QUIET, desktop false, critical false, ("Ready", "");
    OpenFailed = "open_failed", "File cannot be opened", Source, Error, Banner, FIXED_BANNER, desktop true,
        critical true, ("Cannot open clip.mp4", "Invalid data found when processing input");
    PlaybackFailed = "playback_failed", "File does not play", Source, Error, Banner, FIXED_BANNER, desktop true,
        critical true, ("The file cannot be played", "The video could not be decoded.");
    KnownSource = "known_source", "Source known to RustView", Source, Warning, Banner, LOUD, desktop false,
        critical false, ("RustView knows this video", "Known since 2026-10-12. 3 files in the library were made from it.");
    SectionAdded = "section_added", "Section added", Sections, Info, Status, QUIET, desktop false, critical false,
        ("Section 2 added", "");
    /// A typed time that is no time, or puts the end before the start (M6.5c).
    TimeInvalid = "time_invalid", "Typed time refused", Sections, Error, Bubble, BUBBLE, desktop false,
        critical false, ("\"1:2x\" is not a time", "Type H:MM:SS.mmm, for example 0:01:23.500.");
    /// Two sections share a part (rendered twice).
    SectionsOverlap = "sections_overlap", "Sections overlap", Sections, Warning, Bubble, BUBBLE_OR_NOT,
        desktop false, critical false, ("Overlaps section 1", "The shared part is rendered twice.");
    SectionRefused = "section_refused", "Section not possible", Sections, Warning, Toast, EVENT, desktop false,
        critical false, ("Set a section start and end first", "");
    /// The interlace check found combing (Q-N10: the card's note is quiet; a toast or banner only on request).
    Interlaced = "interlaced", "Interlaced video found", Filters, Info, Nothing, LOUD, desktop false,
        critical false, ("The video is interlaced", "Switch Deinterlace on in the Filters card.");
    FiltersReset = "filters_reset", "Filters reset", Filters, Info, Status, QUIET, desktop false, critical false,
        ("Filters reset", "");
    /// A filter menu's "save as default" (P9.13): Settings > Filters took its options.
    FilterDefaultsSaved = "filter_defaults_saved", "Filter defaults saved", Filters, Success, Status, QUIET,
        desktop false, critical false, ("Deshake options saved as the defaults", "");
    AutoCrop = "auto_crop", "Black borders cropped", Filters, Success, Activity, ACTIVITY, desktop false,
        critical false, ("Cropped", "140 top, 140 bottom → 1920×800");
    AutoCropFailed = "auto_crop_failed", "Black border detection failed", Filters, Error, Activity, ACTIVITY,
        desktop true, critical false, ("Auto-crop failed", "ffmpeg ended with exit code 1");
    /// A crop set from the history without opening it (its button's right click).
    CropFromHistory = "crop_from_history", "Crop from the history", Filters, Success, Status, QUIET,
        desktop false, critical false, ("Crop 1920×800 set from the history", "");
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
    /// An error or warning of the render settings, at its option (M6.5c).
    ProfileIssue = "profile_issue", "Render settings problem", Codecs, Error, Bubble, BUBBLE, desktop false,
        critical false, ("WebM cannot hold AAC audio", "Choose Opus or Vorbis, or another container.");
    ProfileChanged = "profile_changed", "Render profile saved", Codecs, Success, Toast, EVENT, desktop false,
        critical false, ("Profile \"My HEVC\" saved", "");
    ProfileRefused = "profile_refused", "Render profile not changed", Codecs, Warning, Toast, EVENT, desktop false,
        critical false, ("A profile with this name exists", "");
    /// The Target Folders page could not do what was asked: a picture refused, no free place on the wheel, a folder
    /// already in the tree (P11.3).
    FolderEdit = "folder_edit", "Target folder not changed", Settings, Warning, Toast, EVENT, desktop false,
        critical false, ("The picture was not taken", "The file is neither an image nor an SVG RustCut can read.");
    SettingsReadOnly = "settings_read_only", "Settings not saved", Settings, Warning, Toast, EVENT, desktop false,
        critical false, ("Not saved", "config.toml is from a newer RustCut: the choice is kept for this run only.");
    /// A typed value the option refuses (the Settings window, the Codecs panel; M6.5c).
    ValueInvalid = "value_invalid", "Typed value refused", Settings, Error, Bubble, BUBBLE, desktop false,
        critical false, ("Not a value here", "Type a whole number from 1 to 10.");
    /// `config.toml` or `ui-state.toml` from a newer RustCut (M6.5d: a dialog with the ways out).
    SettingsNewer = "settings_newer", "Settings from a newer RustCut", Settings, Error, Dialog, FIXED_DIALOG,
        desktop true, critical true, ("config.toml is from a newer RustCut", "Format 3; this version knows 2.");
    /// The newer file was backed up and saved with this version, or reset to its defaults.
    SettingsReplaced = "settings_replaced", "Newer settings file replaced", Settings, Success, Toast, EVENT,
        desktop false, critical false, ("config.toml saved with this version", "The copy is in the backup folder.");
    SettingsWindowFailed = "settings_window_failed", "Settings window failed", Settings, Error, Toast, EVENT,
        desktop true, critical false, ("The settings window could not be opened", "No OpenGL context");
    /// The queue starts paused while jobs wait (user decision Q-P8b): a banner with "Resume" by default; "Nothing" with
    /// the desktop switch on makes it a desktop notification.
    QueuePaused = "queue_paused", "Queue paused at the start", Queue, Info, Banner, LOUD, desktop false,
        critical false, ("The queue is paused", "It was paused when RustCut ended. 3 jobs wait.");
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
    /// A job was loaded into the editor (a copy, Q-P8b; D-P9m: replaces the strip in the Job Finalization card, no
    /// message by default: the job is highlighted in the queue).
    JobLoaded = "job_loaded", "Job loaded into the editor", Editing, Info, Nothing, TOAST_OR_NOT, desktop false,
        critical false, ("Job #3 loaded: Beach - 03.mkv", "Saving makes a new job; job #3 stays as it is.");
    /// A loaded job writes into a folder the target folder tree does not have (P11.1, Q-FP2: offered, never added
    /// silently; a banner while the folder is the card's "Job target", the user's answer 2026-10-28).
    FolderUnknown = "folder_unknown", "Loaded job's folder not in the list", Editing, Info, Banner, EVENT,
        desktop false, critical false, ("The job's folder is not in the target folders", "/media/New");
    /// The other notes about loading jobs: not loaded, the link ended, tags left as they were (Q-N8: no message by
    /// default).
    Editing = "editing", "Other notes about loaded jobs", Editing, Info, Nothing, QUIET, desktop false,
        critical false, ("Job #3 is no longer linked to the editor", "");
    TagsUpdated = "tags_updated", "Tags and rating changed", Tags, Success, Toast, EVENT, desktop false,
        critical false, ("Tags and rating of \"Clip\" updated", "");
    TagsFailed = "tags_failed", "Tags not changed", Tags, Error, Toast, EVENT, desktop true, critical false,
        ("The job's tags were not changed", "RustView unavailable: not installed");
    /// RustView refused a job's tags and rating (0.0.67; unreachable calls are retried and say so in the Tag & Rate
    /// panel instead).
    SyncFailed = "sync_failed", "Job's tags not written to RustView", Tags, Error, Toast, EVENT, desktop true,
        critical false, ("Tags of job 3 not written to RustView", "RustView refused: no such media");
    /// At the start, once RustView answers for the first time: "Active" or "Warn" of the Tag & Rate panel is still
    /// off from an earlier run (user decision Q-P8b: a banner by default, or a toast), with a button that switches both on.
    TaggingOff = "tagging_off", "Tagging switched off at the start", Tags, Warning, Banner, LOUD, desktop false,
        critical false, ("Tagging is off", "Jobs are saved without RustView. Switched off in an earlier run.");
    LogCopied = "log_copied", "Log copied", Logs, Success, Toast, EVENT, desktop false, critical false,
        ("143 log lines copied", "");
    ClipboardFailed = "clipboard_failed", "Clipboard unusable", Logs, Warning, Toast, EVENT, desktop false,
        critical false, ("The clipboard cannot be used", "");
    TimeCopied = "time_copied", "Time copied", App, Success, Toast, EVENT, desktop false, critical false,
        ("0:01:23.456 copied", "");
    MpvFailed = "mpv_failed", "mpv did not start", App, Warning, Toast, EVENT, desktop false, critical false,
        ("mpv cannot be found", "Install it with: sudo pacman -S mpv");
    /// The queue database cannot be used (M6.5d turns it into a dialog with ways out).
    DatabaseUnusable = "database_unusable", "Queue database unusable", App, Error, Dialog, FIXED_DIALOG,
        desktop true, critical true, ("The queue cannot be opened", "database disk image is malformed");
    /// A way out of the database dialog worked.
    DatabaseRepaired = "database_repaired", "Queue database repaired", App, Success, Toast, EVENT, desktop false,
        critical false, ("2 jobs rescued", "The old file is kept in the backup folder.");
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
            // Q-N8: errors also go to the desktop while RustCut is not in front; a bubble belongs to an input the
            // user works with, never the desktop.
            assert_eq!(
                info.desktop,
                (info.level == Level::Error && info.place != Bubble)
                    || matches!(kind, Kind::QueueDone | Kind::PowerCountdown),
                "{}",
                info.id
            );
            assert_eq!(
                info.place == Bubble,
                info.places.contains(&Bubble),
                "{}: a bubble kind is a bubble or nothing",
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
                "settings_newer",
                "database_unusable",
                "tools_missing"
            ]
        );
        assert!(!Kind::JobFailed.info().critical);
    }
}
