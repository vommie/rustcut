//! General, Warnings & Confirmations, Desktop Integration, Appearance and Advanced & Logs.

use rustcut_core::settings::{GeneralSettings, Settings};

use super::Card;
use crate::ui::settings::field::{Field, Kind, Value, WRONG_VALUE, always, number, path_field, switch};

pub static GENERAL: &[Card] = &[
    Card {
        title: "Startup",
        description: "",
        fields: &[switch!(
            "general.single_instance",
            "One window",
            "A second start hands its files to the running RustCut instead of opening another window.",
            general.single_instance
        )],
    },
    Card {
        title: "Deleting and discarding",
        description: "RustCut never asks \"Are you sure?\": buttons and menu entries that delete or discard something \
                      have to be held until the ring is full. Their keys act at once.",
        fields: &[Field {
            key: "general.hold_to_confirm_ms",
            label: "Hold for",
            help: "How long such a button or menu entry has to be held. 0 = a click is enough.",
            kind: Kind::Number {
                min: 0,
                max: 10_000,
                step: 100,
                unit: "ms",
            },
            live: true,
            get: |s| Value::Number(i64::from(s.general.hold_to_confirm_ms)),
            set: set_hold,
            enabled: always,
            indent: false,
        }],
    },
    Card {
        title: "Quitting",
        description: "",
        fields: &[switch!(
            "general.confirm_quit_while_rendering",
            "Warn when closing while a job renders",
            "Closing the window while the queue renders asks first. A render that is stopped waits again at the next \
             start.",
            general.confirm_quit_while_rendering
        )],
    },
];

fn set_hold(s: &mut Settings, v: Value) -> Result<(), String> {
    let ms = u16::try_from(v.as_number()?).map_err(|_out_of_range| WRONG_VALUE.to_owned())?;
    let range = GeneralSettings::HOLD_TO_CONFIRM_MS;
    if ms != 0 && !range.contains(&ms) {
        return Err(format!(
            "Type 0 (a click is enough) or {} to {} ms.",
            range.start(),
            range.end()
        ));
    }
    s.general.hold_to_confirm_ms = ms;
    Ok(())
}

pub static WARNINGS: &[Card] = &[
    Card {
        title: "When a job is saved",
        description: "Each of these asks or hints before a job is queued. Switched off, RustCut takes the safe way \
                      without asking.",
        fields: &[
            switch!(
                "warnings.target_exists",
                "Ask when the target file exists",
                "Off: the job gets the next free name (the next counter number, else \"Name (n)\"); an existing file \
                 is never replaced unasked.",
                warnings.target_exists
            ),
            switch!(
                "warnings.target_queued",
                "Ask when a queued job writes the same file",
                "Off: the job is saved anyway; the queue never renders two jobs with the same target at once.",
                warnings.target_queued
            ),
            switch!(
                "warnings.base_name_in_folder",
                "Show similar names in the target folder",
                "A hint in the export card with the files whose names start with the typed name (never a question).",
                warnings.base_name_in_folder
            ),
            switch!(
                "warnings.missing_metadata",
                "Warn about missing tags or rating",
                "The Tag & Rate panel's \"Warn\" switch: asks before a job without tags or rating is saved (tags only \
                 when RustView offers some), while the RustView link and the panel's \"Active\" switch are on. The \
                 question holds the rating bar: one click saves with it.",
                warnings.missing_metadata
            ),
        ],
    },
    Card {
        title: "When a video opens",
        description: "",
        fields: &[Field::button(
            "page.messages",
            "Warning about a source cut before",
            "Moved to Messages: the kind \"Source cut before\" (its place \"Nothing\" = the video's MD5 is not read).",
            "Open Messages",
        )],
    },
];

fn tray_on(s: &Settings) -> bool {
    s.desktop.tray.enabled
}

pub static DESKTOP: &[Card] = &[
    Card {
        title: "Notifications",
        description: "",
        fields: &[Field::button(
            "page.messages.desktop",
            "Desktop notifications",
            "Moved to Messages: the master switch and the time on its first tab, which kinds go to the desktop on \
             the second.",
            "Open Messages",
        )],
    },
    Card {
        title: "Tray",
        description: "",
        fields: &[
            switch!(
                "desktop.tray.enabled",
                "Show a tray icon",
                "Queue progress, pause and resume in the system tray; a click shows or hides the window.",
                desktop.tray.enabled
            ),
            switch!(
                "desktop.tray.minimize_to_tray",
                "Minimize to the tray while rendering",
                "Minimizing the window while the queue renders hides it in the tray.",
                desktop.tray.minimize_to_tray
            )
            .under(tray_on),
        ],
    },
    Card {
        title: "Taskbar and power",
        description: "",
        fields: &[
            switch!(
                "desktop.taskbar_progress",
                "Queue progress on the taskbar entry",
                "Shows the overall progress of the queue on RustCut's taskbar entry (KDE Plasma).",
                desktop.taskbar_progress
            ),
            switch!(
                "desktop.inhibit_sleep",
                "Keep the computer awake while rendering",
                "Blocks sleep and the screen locker's suspend while a job renders; released when the queue stops.",
                desktop.inhibit_sleep
            ),
        ],
    },
];

pub static APPEARANCE: &[Card] = &[Card {
    title: "Tooltips",
    description: "Every button explains itself in a tooltip, with its key.",
    fields: &[
        switch!(
            "appearance.tooltips",
            "Show tooltips",
            "Off: no tooltips anywhere (the descriptions in this window stay).",
            appearance.tooltips
        ),
        number!(
            "appearance.tooltip_delay_ms",
            "Delay",
            "How long the pointer rests on a button before its tooltip shows.",
            appearance.tooltip_delay_ms: u16,
            0..=5000,
            100,
            "ms"
        )
        .under(|s| s.appearance.tooltips),
    ],
}];

fn set_log_level(s: &mut Settings, v: Value) -> Result<(), String> {
    let text = v.as_text()?;
    let text = if text.is_empty() { "info" } else { text };
    tracing_subscriber::EnvFilter::builder()
        .with_regex(false)
        .parse(text)
        .map_err(|e| format!("Not a log filter: {e}."))?;
    text.clone_into(&mut s.advanced.log_level);
    Ok(())
}

pub static ADVANCED: &[Card] = &[
    Card {
        title: "Programs",
        description: "Empty = the program found in PATH.",
        fields: &[
            path_field!(
                "advanced.ffmpeg",
                "ffmpeg",
                "Renders the jobs, checks the encoders and finds crops and interlacing.",
                advanced.ffmpeg,
                "ffmpeg (from PATH)",
                folder: false
            ),
            path_field!(
                "advanced.ffprobe",
                "ffprobe",
                "Reads the streams of a video (the Info panel) and checks every finished render.",
                advanced.ffprobe,
                "ffprobe (from PATH)",
                folder: false
            ),
        ],
    },
    Card {
        title: "Logs",
        description: "",
        fields: &[
            Field {
                key: "advanced.log_level",
                label: "Log level",
                help: "error, warn, info, debug or trace, or a filter such as rustcut=debug,info. The RUST_LOG \
                       variable wins when it is set.",
                kind: Kind::Text { placeholder: "info" },
                live: true,
                get: |s| Value::Text(s.advanced.log_level.clone()),
                set: set_log_level,
                enabled: always,
                indent: false,
            },
            number!(
                "advanced.log_keep_days",
                "Keep log files for",
                "One file per day; older files are deleted at start.",
                advanced.log_keep_days: u16,
                1..=365,
                1,
                "days"
            ),
            Field::button("open.logs", "Log folder", "The daily log files of the app.", "Open"),
        ],
    },
    Card {
        title: "Shareable log",
        description: "A copy of the log that is safe to send: what is switched on below is replaced by placeholders \
                      that stay the same within a run (<file-3>.mkv, <dir-2>, <tag-4>, <user>, <md5-1>, <meta-1>). The \
                      Logs panel shows it with its spy button, and copies it from there.",
        fields: &[
            switch!(
                "advanced.shareable_log",
                "Write the shareable file",
                "rustcut-shareable.DATE.log beside the log, kept as long as the log (recommended: on).",
                advanced.shareable_log
            ),
            switch!(
                "advanced.redact.paths",
                "File names, folders and paths",
                "Videos, targets and every other path (the extension stays).",
                advanced.redact.paths
            ),
            switch!(
                "advanced.redact.tags",
                "Tag names",
                "RustView's tags (whole words).",
                advanced.redact.tags
            ),
            switch!(
                "advanced.redact.user",
                "User and host names",
                "Your login name and this computer's name, also inside URLs.",
                advanced.redact.user
            ),
            switch!(
                "advanced.redact.md5",
                "MD5 sums",
                "The checksums RustCut and RustView keep of the videos.",
                advanced.redact.md5
            ),
            switch!(
                "advanced.redact.metadata",
                "Titles and metadata",
                "The titles stored in the videos and their streams.",
                advanced.redact.metadata
            ),
        ],
    },
    Card {
        title: "Folders",
        description: "Where RustCut keeps its files (the --home option moves all of them).",
        fields: &[
            Field::button(
                "open.config",
                "Settings",
                "config.toml, ui-state.toml and their backups.",
                "Open",
            ),
            Field::button(
                "open.data",
                "Data",
                "The queue database and the deshake analysis files.",
                "Open",
            ),
            Field::button(
                "open.cache",
                "Cache",
                "Probe renders and encoder checks; safe to delete.",
                "Open",
            ),
        ],
    },
];
