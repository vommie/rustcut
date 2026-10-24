//! Player, Editor & Sections, Filters and Export & Naming (the template; the target folders have their own editor).

use rustcut_core::edit::{Deinterlacer, DeshakeCrop, DeshakeInterpolation, DeshakeParams};
use rustcut_core::naming::{NameContext, render_stem};
use rustcut_core::settings::{AutoSection, OnJobLoad, Settings};

use super::Card;
use crate::ui::settings::field::{Field, Kind, Value, WRONG_VALUE, always, choice, number, switch};
use crate::util::hex_color;

fn set_background(s: &mut Settings, v: Value) -> Result<(), String> {
    let rgb =
        hex_color::parse_rgb(v.as_text()?).ok_or("Choose a color, or type it as #RRGGBB (for example #444444).")?;
    s.player.background = hex_color::format_rgb(rgb);
    Ok(())
}

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
                help: "mpv's hwdec: auto-copy (default), auto, no, vaapi, nvdec, vulkan. Without a GPU, mpv decodes in \
                       software.",
                kind: Kind::Text {
                    placeholder: "auto-copy",
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

/// A deshake parameter: kept in vid.stab's ranges (accuracy is never below shakiness).
macro_rules! deshake {
    ($key:literal, $label:literal, $help:literal, $field:ident : $t:ty, $min:literal..=$max:literal, $step:literal, $unit:literal) => {
        Field {
            key: $key,
            label: $label,
            help: $help,
            kind: Kind::Number {
                min: $min,
                max: $max,
                step: $step,
                unit: $unit,
            },
            live: true,
            get: |s| Value::Number(i64::from(s.filters.deshake.$field)),
            set: |s, v| {
                let mut params = s.filters.deshake;
                params.$field = <$t>::try_from(v.as_number()?).map_err(|_out_of_range| WRONG_VALUE.to_owned())?;
                s.filters.deshake = DeshakeParams::sanitized(params);
                Ok(())
            },
            enabled: always,
            indent: false,
        }
    };
}

pub static FILTERS: &[Card] = &[
    Card {
        title: "Deinterlace",
        description: "",
        fields: &[choice!(
            "filters.deinterlacer",
            "Deinterlacer",
            "Chosen when the filter is switched on. bwdif: good quality at a moderate cost; yadif: faster, a little \
             softer; w3fdif: sharp, for little motion; kerndeint: fast.",
            filters.deinterlacer,
            [
                (Deinterlacer::Bwdif, "bwdif"),
                (Deinterlacer::Yadif, "yadif"),
                (Deinterlacer::W3fdif, "w3fdif"),
                (Deinterlacer::Kerndeint, "kerndeint")
            ]
        )],
    },
    Card {
        title: "Deshake",
        description: "What a new deshake filter starts with (vid.stab, two passes). The Filters card changes them per \
                      job.",
        fields: &[
            deshake!(
                "filters.deshake.shakiness",
                "Shakiness",
                "How shaky the video is, 1 (little) to 10 (very). Higher is slower.",
                shakiness: u8,
                1..=10,
                1,
                ""
            ),
            deshake!(
                "filters.deshake.accuracy",
                "Accuracy",
                "Detection accuracy, at least the shakiness. Higher is slower.",
                accuracy: u8,
                1..=15,
                1,
                ""
            ),
            deshake!(
                "filters.deshake.stepsize",
                "Search step",
                "In pixels. Smaller is more accurate and slower.",
                stepsize: u8,
                1..=32,
                1,
                "px"
            ),
            deshake!(
                "filters.deshake.smoothing",
                "Smoothing",
                "Frames used to smooth the camera path. Higher is steadier but follows pans later.",
                smoothing: u16,
                0..=1000,
                5,
                "frames"
            ),
            deshake!(
                "filters.deshake.zoom",
                "Zoom",
                "Extra zoom against the moving borders.",
                zoom: i8,
                -100..=100,
                1,
                "%"
            ),
            choice!(
                "filters.deshake.optzoom",
                "Automatic zoom",
                "None, one zoom for the whole video, or a zoom that follows the motion.",
                filters.deshake.optzoom,
                [(0, "None"), (1, "Static"), (2, "Adaptive")]
            ),
            choice!(
                "filters.deshake.crop",
                "Borders",
                "What shows where the stabilized picture left the frame.",
                filters.deshake.crop,
                [(DeshakeCrop::Black, "Black"), (DeshakeCrop::Keep, "Previous frame")]
            ),
            choice!(
                "filters.deshake.interpolation",
                "Interpolation",
                "How moved pixels are computed. Bicubic is the sharpest and the slowest.",
                filters.deshake.interpolation,
                [
                    (DeshakeInterpolation::No, "None"),
                    (DeshakeInterpolation::Linear, "Linear"),
                    (DeshakeInterpolation::Bilinear, "Bilinear"),
                    (DeshakeInterpolation::Bicubic, "Bicubic")
                ]
            ),
            switch!(
                "filters.deshake.tripod",
                "Virtual tripod",
                "Keeps the camera fixed on the first frame instead of smoothing its path.",
                filters.deshake.tripod
            ),
        ],
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
        },
        live: true,
        get: |s| Value::Text(s.export.name_template.clone()),
        set: set_template,
        enabled: always,
        indent: false,
    }],
}];
