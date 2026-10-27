//! Player, Editor & Sections and Export & Naming (the template; the target folders have their own editor).

use rustcut_core::naming::{NameContext, render_stem};
use rustcut_core::settings::{AutoSection, OnJobLoad, Settings};

use super::Card;
use crate::ui::settings::field::{Field, Kind, Value, always, choice, number, switch};
use crate::util::hex_color;

fn set_background(s: &mut Settings, v: Value) -> Result<(), String> {
    let rgb =
        hex_color::parse_rgb(v.as_text()?).ok_or("Choose a color, or type it as #RRGGBB (for example #444444).")?;
    s.player.background = hex_color::format_rgb(rgb);
    Ok(())
}

/// mpv's `hwdec` values for Linux (mpv 0.41's manual and `--hwdec=help`; other systems' values are left out).
const HWDEC: &[&str] = &[
    "auto-copy",
    "auto",
    "auto-unsafe",
    "no",
    "vaapi-copy",
    "vaapi",
    "nvdec-copy",
    "nvdec",
    "vulkan-copy",
    "vulkan",
    "drm-copy",
    "drm",
    "cuda-copy",
    "cuda",
    "vdpau-copy",
    "vdpau",
    "v4l2m2m-copy",
];

fn set_hwdec(s: &mut Settings, v: Value) -> Result<(), String> {
    let text = v.as_text()?;
    let text = if text.is_empty() { "auto-copy" } else { text };
    if !text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ',' | '_'))
    {
        return Err("Type one of mpv's hwdec values, for example auto-copy, no or vaapi.".into());
    }
    text.clone_into(&mut s.player.hwdec);
    Ok(())
}

pub static PLAYER: &[Card] = &[
    Card {
        title: "Playback",
        description: "",
        fields: &[
            switch!(
                "player.auto_play",
                "Play when a video opens",
                "Off: a video opens paused on its first frame.",
                player.auto_play
            ),
            number!(
                "player.wheel_volume_step",
                "Volume per wheel step",
                "How much one notch of the mouse wheel over the video changes the volume.",
                player.wheel_volume_step: u8,
                1..=25,
                1,
                "%"
            ),
            Field {
                key: "player.hwdec",
                label: "Hardware decoding",
                help: "mpv's hwdec: pick a value from the list or type one (several separated by commas try them in \
                       turn). auto-copy (the default) uses the GPU where it works safely, no always decodes in \
                       software; the -copy variants copy each frame back to the CPU. Without a usable GPU, mpv \
                       decodes in software.",
                kind: Kind::Text {
                    placeholder: "auto-copy",
                    suggestions: HWDEC,
                },
                live: true,
                get: |s| Value::Text(s.player.hwdec.clone()),
                set: set_hwdec,
                enabled: always,
                indent: false,
            },
            Field {
                key: "player.background",
                label: "Background",
                help: "The color around the video. Gray shows where a video's own black borders end.",
                kind: Kind::Color,
                live: true,
                get: |s| Value::Text(s.player.background.clone()),
                set: set_background,
                enabled: always,
                indent: false,
            },
        ],
    },
    Card {
        title: "Symbols over the video",
        description: "What keys and the mouse wheel show over the video, each on its own (the buttons show their \
                      state themselves).",
        fields: &[
            switch!(
                "player.flash_play_pause",
                "Flash a play or pause symbol",
                "When a key plays or pauses the video, the symbol shows briefly in its middle.",
                player.flash_play_pause
            ),
            switch!(
                "player.keep_pause_symbol",
                "Keep the pause symbol while paused",
                "A faint pause symbol stays in the middle of the video while it is paused.",
                player.keep_pause_symbol
            ),
            switch!(
                "player.flash_mute",
                "Flash the mute symbol",
                "When the mute key mutes or unmutes the video, a speaker shows briefly in its middle.",
                player.flash_mute
            ),
            switch!(
                "player.flash_rating",
                "Flash the rating",
                "When Ctrl+0 to Ctrl+5 give a rating, its digit shows briefly in the middle of the video.",
                player.flash_rating
            ),
            switch!(
                "player.volume_bar",
                "Show the volume bar",
                "When the mouse wheel changes the volume, a bar with the level shows near the bottom of the video.",
                player.volume_bar
            ),
        ],
    },
    Card {
        title: "Preview audio",
        description: "These change only what you hear in RustCut, never the rendered file.",
        fields: &[
            switch!(
                "player.mute_last_second",
                "Mute the last second",
                "Against the crackling some videos make at their end.",
                player.mute_last_second
            ),
            switch!(
                "player.loudnorm_preview",
                "Even out the loudness",
                "Normalizes the preview's loudness (ffmpeg's loudnorm); costs a little CPU while playing.",
                player.loudnorm_preview
            ),
        ],
    },
];

pub static EDITOR: &[Card] = &[
    Card {
        title: "Seeking",
        description: "The keys and the steps they jump (Hotkeys has the keys).",
        fields: &[
            number!(
                "editor.seek_step_secs",
                "Short step",
                "Left and Right arrow.",
                editor.seek_step_secs: u16,
                1..=3600,
                1,
                "s"
            ),
            number!(
                "editor.seek_step_long_secs",
                "Long step",
                "Shift with Left and Right arrow.",
                editor.seek_step_long_secs: u16,
                1..=3600,
                5,
                "s"
            ),
        ],
    },
    Card {
        title: "Saving a job",
        description: "",
        fields: &[choice!(
            "editor.auto_section",
            "Without sections",
            "What Save does when no section was added: take the range between the marks (the whole video without \
             marks, like PyCutRv), refuse, always take the whole video, or only the range between the marks.",
            editor.auto_section,
            [
                (AutoSection::RangeOrWhole, "Marked range, else whole video"),
                (AutoSection::Off, "Refuse to save"),
                (AutoSection::WholeVideo, "Whole video"),
                (AutoSection::CurrentRange, "Current range")
            ]
        )],
    },
    Card {
        title: "Editing a queued job",
        description: "",
        fields: &[choice!(
            "editor.on_job_load",
            "The current video",
            "What happens to the open video, its sections, filters and tags when a queued job is loaded into the \
             editor: replaced at once, or a question first while it has sections or marks.",
            editor.on_job_load,
            [(OnJobLoad::Replace, "Replace it"), (OnJobLoad::Ask, "Ask first")]
        )],
    },
];

/// A sample of every variable a template can use.
fn sample() -> NameContext {
    NameContext {
        name: "Holiday".into(),
        source: "Holiday".into(),
        counter: Some(1),
        date: "2026-10-24".into(),
        time: "14-05-09".into(),
        profile: "AV1 balanced".into(),
        codec: "AV1".into(),
        sections: 2,
        duration: "0-01-30".into(),
    }
}

fn set_template(s: &mut Settings, v: Value) -> Result<(), String> {
    let text = v.as_text()?;
    render_stem(text, &sample()).map_err(|e| format!("Not a usable template: {e}."))?;
    text.clone_into(&mut s.export.name_template);
    Ok(())
}

/// The file name the template makes from the sample (shown under the field).
pub fn template_example(template: &str) -> Option<String> {
    render_stem(template, &sample()).ok()
}

pub static EXPORT: &[Card] = &[Card {
    title: "File names",
    description: "The template makes the target's file name (without the extension). Variables: name (typed in the \
                  export card), source, counter (empty while the counter is off), date, time, profile, codec, \
                  sections, duration; counter | pad(2) writes 01.",
    fields: &[Field {
        key: "export.name_template",
        label: "Template",
        help: "Jinja syntax, for example {{ name }}{% if counter %} - {{ counter | pad(2) }}{% endif %}.",
        kind: Kind::Text {
            placeholder: rustcut_core::naming::DEFAULT_TEMPLATE,
            suggestions: &[],
        },
        live: true,
        get: |s| Value::Text(s.export.name_template.clone()),
        set: set_template,
        enabled: always,
        indent: false,
    }],
}];
