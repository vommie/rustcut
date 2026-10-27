//! The texts of the conflict dialog's tiles without Slint: the state of a file's preview, its facts, date, the reason
//! for a missing picture and the details tooltip's rows.

use std::sync::Arc;

use rustcut_core::media::{MediaInfo, Preview, format_bit_rate, format_frame_rate, format_size};
use rustcut_core::naming::ExistingFile;
use rustcut_core::time::Timestamp;
use slint::Image;

/// What a tile shows (the numbers are `PreviewView.state` in Slint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::export) enum TileState {
    Loading = 0,
    Picture = 1,
    /// Described, but without a picture (pictures off, audio only, ffmpeg failed): the note says why.
    NoPicture = 2,
    /// Gone or unreadable.
    Failed = 3,
}

/// What the dialog knows about one file.
#[derive(Clone, Default)]
pub(in crate::ui::export) enum Slot {
    /// Asked, no answer yet.
    #[default]
    Waiting,
    Ready(Arc<Preview>, Option<Image>),
    Gone(String),
}

/// The texts of a tile or of the existing file's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::export) struct TileText {
    pub state: TileState,
    /// "812 MB · 4:12 · 1920×1080".
    pub facts: String,
    /// "812 MB · 4:12".
    pub short: String,
    /// "2026-10-20 14:32".
    pub date: String,
    /// Why there is no picture, in two words for the picture (empty with a picture or while loading); the details
    /// tooltip has the whole reason as its "Problem" row.
    pub note: String,
    /// The details tooltip: label and value per row.
    pub rows: Vec<(String, String)>,
}

/// The texts for `slot`; `known` is what the folder check found before any preview (size and date of the target).
pub(in crate::ui::export) fn tile_text(slot: &Slot, known: Option<ExistingFile>, pictures: bool) -> TileText {
    let mut text = TileText {
        state: TileState::Loading,
        facts: String::new(),
        short: String::new(),
        date: String::new(),
        note: String::new(),
        rows: Vec::new(),
    };
    if let Some(known) = known {
        text.short = format_size(known.bytes);
        text.facts.clone_from(&text.short);
        text.date = known.modified.map(date_text).unwrap_or_default();
    }
    match slot {
        Slot::Waiting => {}
        Slot::Gone(reason) => {
            text.state = TileState::Failed;
            "Not readable".clone_into(&mut text.note);
            text.rows.push(("Problem".to_owned(), reason.clone()));
        }
        Slot::Ready(preview, image) => {
            let info = preview.info.as_ref();
            let length = info.and_then(MediaInfo::duration).map(length_text);
            let size = info
                .and_then(|i| i.primary_video())
                .and_then(|(_, v)| v.oriented_size())
                .map(|(w, h)| format!("{w}×{h}"));
            let bytes = format_size(preview.bytes);
            text.short = [Some(bytes.clone()), length.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            text.facts = [Some(bytes.clone()), length.clone(), size]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            text.date = preview.modified.map(date_text).unwrap_or_default();
            text.rows = rows(preview, &bytes, &text.date, length);
            text.state = if image.is_some() {
                TileState::Picture
            } else {
                TileState::NoPicture
            };
            if image.is_none() {
                let (note, why) = no_picture_note(preview, pictures);
                note.clone_into(&mut text.note);
                if let Some(why) = why {
                    text.rows.push(("Problem".to_owned(), why));
                }
            }
        }
    }
    text
}

/// Why a described file has no picture: two words for the picture and, when there is more to say, the whole reason.
fn no_picture_note(preview: &Preview, pictures: bool) -> (&'static str, Option<String>) {
    let Some(info) = preview.info.as_ref() else {
        let why = preview.problem.as_deref().unwrap_or("ffprobe could not read it");
        return ("Not a video", Some(why.to_owned()));
    };
    if info.primary_video().is_none() {
        return (if info.has_audio() { "Audio only" } else { "No video" }, None);
    }
    if !pictures {
        return (
            "Pictures off",
            Some("Pictures are off for this file (Settings > Dialogs)".to_owned()),
        );
    }
    let why = preview.problem.as_deref().unwrap_or("ffmpeg wrote none");
    ("No picture", Some(why.to_owned()))
}

/// The details tooltip: modified, size, length, video, audio, bit rate.
fn rows(preview: &Preview, bytes: &str, date: &str, length: Option<String>) -> Vec<(String, String)> {
    let mut rows = vec![
        ("Modified".to_owned(), date.to_owned()),
        ("Size".to_owned(), bytes.to_owned()),
    ];
    rows.retain(|(_, value)| !value.is_empty());
    let Some(info) = preview.info.as_ref() else {
        return rows;
    };
    if let Some(length) = length {
        rows.push(("Length".to_owned(), length));
    }
    if let Some((stream, video)) = info.primary_video() {
        let parts: Vec<String> = [
            video.oriented_size().map(|(w, h)| format!("{w}×{h}")),
            stream.codec.as_deref().map(codec_label),
            video.avg_frame_rate.or(video.frame_rate).map(format_frame_rate),
        ]
        .into_iter()
        .flatten()
        .collect();
        rows.push(("Video".to_owned(), parts.join(" · ")));
    }
    let audio: Vec<_> = info.audio_streams().collect();
    let audio_text = match audio.as_slice() {
        [] => "None".to_owned(),
        [(stream, track)] => [
            stream.codec.as_deref().map(codec_label),
            track
                .channel_layout
                .clone()
                .or_else(|| track.channels.map(|n| format!("{n} channels"))),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
        [(first, _), ..] => format!(
            "{} tracks · {}",
            audio.len(),
            first.codec.as_deref().map_or_else(String::new, codec_label)
        ),
    };
    rows.push(("Audio".to_owned(), audio_text));
    if let Some(rate) = info.container.bit_rate {
        rows.push(("Bit rate".to_owned(), format_bit_rate(rate)));
    }
    rows
}

/// ffmpeg's short codec names as people know them (`h264` -> `H.264`).
pub(in crate::ui::export) fn codec_label(codec: &str) -> String {
    match codec {
        "h264" => "H.264".to_owned(),
        "hevc" => "HEVC".to_owned(),
        "av1" => "AV1".to_owned(),
        "vp8" => "VP8".to_owned(),
        "vp9" => "VP9".to_owned(),
        "mpeg4" => "MPEG-4".to_owned(),
        "mpeg2video" => "MPEG-2".to_owned(),
        "mpeg1video" => "MPEG-1".to_owned(),
        "prores" => "ProRes".to_owned(),
        "aac" => "AAC".to_owned(),
        "opus" => "Opus".to_owned(),
        "mp3" => "MP3".to_owned(),
        "flac" => "FLAC".to_owned(),
        "vorbis" => "Vorbis".to_owned(),
        "ac3" => "AC-3".to_owned(),
        "eac3" => "E-AC-3".to_owned(),
        c if c.starts_with("pcm_") => "PCM".to_owned(),
        other => other.to_uppercase(),
    }
}

/// `4:12`, `1:02:03` (whole seconds).
pub(in crate::ui::export) fn length_text(length: Timestamp) -> String {
    let secs = length.as_micros().max(0) / Timestamp::MICROS_PER_SECOND;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "2026-10-27 19:00 (today)": the date with how long ago it was (user review 2026-10-27).
fn date_text(time: std::time::SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = time.into();
    let date = local.format("%Y-%m-%d %H:%M").to_string();
    match crate::util::ago::ago(local.date_naive(), chrono::Local::now().date_naive()) {
        Some(ago) => format!("{date} ({ago})"),
        None => date,
    }
}
