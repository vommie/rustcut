//! The readable description of a [`MediaInfo`] that the Media Info panel shows: sections (container, each stream) of
//! label/value rows, in plain words and the units people expect ("48 kHz", "4.14 Mbit/s", "29.97 fps"). Pure and
//! snapshot-tested against the test media, so the panel and the tests see the same text.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::model::{
    AudioStream, ColorInfo, FieldOrder, FrameRateMode, MediaInfo, Ratio, Stream, StreamKind, VideoStream,
};
use crate::time::{FrameRate, Precision, Timestamp};

/// A stream's duration is shown when it differs at least this much from the container's (audio that ends early).
const DURATION_DIFFERENCE: Timestamp = Timestamp::from_millis(100);
/// A stream's start time is shown when it differs at least this much from the container's.
const START_DIFFERENCE: Timestamp = Timestamp::from_millis(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoSection {
    pub title: String,
    pub rows: Vec<InfoRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoRow {
    pub label: &'static str,
    pub value: String,
}

/// The sections of `info`: the container first, then the streams in file order (attachments in one section).
pub fn summarize(info: &MediaInfo) -> Vec<InfoSection> {
    let mut sections = vec![container(info)];
    let count = |kind: &str| {
        info.streams
            .iter()
            .filter(|s| s.kind.name() == kind && !is_cover(s))
            .count()
    };
    let mut numbers: HashMap<&str, usize> = HashMap::new();
    let mut attachments = 0usize;
    for stream in &info.streams {
        if matches!(stream.kind, StreamKind::Attachment) {
            attachments += 1;
            continue;
        }
        let kind = stream.kind.name();
        let title = if is_cover(stream) {
            "Cover art".to_owned()
        } else {
            let several = count(kind) > 1;
            let number = numbers.entry(kind).or_insert(0);
            *number += 1;
            let mut title = capitalized(kind);
            if several {
                let _ = write!(title, " {number}");
                if stream.default {
                    title.push_str(" (default)");
                }
            }
            title
        };
        sections.push(InfoSection {
            title,
            rows: stream_rows(info, stream),
        });
    }
    if attachments > 0 {
        sections.push(InfoSection {
            title: "Attachments".into(),
            rows: vec![row("Files", attachments.to_string())],
        });
    }
    sections
}

/// All sections as plain text (logs, snapshot tests).
pub fn to_text(sections: &[InfoSection]) -> String {
    let mut text = String::new();
    for section in sections {
        let _ = writeln!(text, "[{}]", section.title);
        for row in &section.rows {
            let _ = writeln!(text, "{}: {}", row.label, row.value);
        }
    }
    text
}

fn container(info: &MediaInfo) -> InfoSection {
    let c = &info.container;
    let mut rows = Vec::new();
    if let Some(format) = c.format_long.as_ref().or(c.format.as_ref()) {
        rows.push(row("Format", format.clone()));
    }
    if let Some(duration) = info.duration() {
        rows.push(row("Duration", duration.format(Precision::Millis)));
    }
    if let Some(start) = c.start_time.filter(|s| *s != Timestamp::ZERO) {
        rows.push(row("Start time", start.format(Precision::Millis)));
    }
    if let Some(size) = c.size {
        rows.push(row("Size", format_size(size)));
    }
    if let Some(rate) = c.bit_rate {
        rows.push(row("Bit rate", format_bit_rate(rate)));
    }
    if let Some(title) = &c.title {
        rows.push(row("Title", title.clone()));
    }
    if info.streams.is_empty() {
        rows.push(row("Streams", "none".into()));
    }
    InfoSection {
        title: "Container".into(),
        rows,
    }
}

fn stream_rows(info: &MediaInfo, stream: &Stream) -> Vec<InfoRow> {
    let mut rows = Vec::new();
    if let Some(codec) = codec_text(stream) {
        rows.push(row("Codec", codec));
    }
    if let Some(profile) = &stream.profile {
        rows.push(row("Profile", profile.clone()));
    }
    match &stream.kind {
        StreamKind::Video(video) => video_rows(info, video, &mut rows),
        StreamKind::Audio(audio) => audio_rows(audio, &mut rows),
        StreamKind::Subtitle | StreamKind::Data | StreamKind::Attachment | StreamKind::Other(_) => {}
    }
    if let Some(rate) = stream.bit_rate {
        rows.push(row("Bit rate", format_bit_rate(rate)));
    }
    let container_duration = info.container.duration;
    if let Some(duration) = stream.duration
        && container_duration.is_none_or(|c| c.abs_diff(duration) >= DURATION_DIFFERENCE)
        && !is_cover(stream)
    {
        rows.push(row("Duration", duration.format(Precision::Millis)));
    }
    let container_start = info.container.start_time.unwrap_or(Timestamp::ZERO);
    if let Some(start) = stream.start_time
        && start.abs_diff(container_start) >= START_DIFFERENCE
    {
        rows.push(row("Start time", start.format(Precision::Millis)));
    }
    if let Some(language) = &stream.language {
        rows.push(row("Language", language_text(language)));
    }
    if let Some(title) = &stream.title {
        rows.push(row("Title", title.clone()));
    }
    rows
}

fn video_rows(info: &MediaInfo, video: &VideoStream, rows: &mut Vec<InfoRow>) {
    if let (Some(w), Some(h)) = (video.width, video.height) {
        rows.push(row("Resolution", format!("{w} x {h}")));
    }
    if video.rotation != 0 {
        let mut text = format!("{}° clockwise", video.rotation);
        if let Some((w, h)) = video.oriented_size() {
            let _ = write!(text, " (shown as {w} x {h})");
        }
        rows.push(row("Rotation", text));
    }
    if let Some(dar) = video.display_aspect {
        let mut text = ratio_text(dar);
        if video.is_anamorphic()
            && let Some(sar) = video.sample_aspect
        {
            let _ = write!(text, " (pixels {})", ratio_text(sar));
        }
        rows.push(row("Aspect ratio", text));
    }
    if video.attached_picture {
        if let Some(format) = &video.pixel_format {
            rows.push(row("Pixel format", format.clone()));
        }
        return;
    }
    if let Some(text) = frame_rate_text(info, video) {
        rows.push(row("Frame rate", text));
    }
    if let Some(frames) = video.frame_count {
        rows.push(row("Frames", frames.to_string()));
    }
    if let Some(order) = video.field_order {
        rows.push(row("Scan", scan_text(order).into()));
    }
    if let Some(format) = &video.pixel_format {
        let text = match video.bit_depth {
            Some(depth) => format!("{format} ({depth}-bit)"),
            None => format.clone(),
        };
        rows.push(row("Pixel format", text));
    }
    if let Some(color) = color_text(&video.color) {
        rows.push(row("Color", color));
    }
}

fn audio_rows(audio: &AudioStream, rows: &mut Vec<InfoRow>) {
    if let Some(rate) = audio.sample_rate {
        rows.push(row("Sample rate", format_sample_rate(rate)));
    }
    match (audio.channels, &audio.channel_layout) {
        (Some(n), Some(layout)) => rows.push(row("Channels", format!("{n} ({layout})"))),
        (Some(n), None) => rows.push(row("Channels", n.to_string())),
        (None, Some(layout)) => rows.push(row("Channels", layout.clone())),
        (None, None) => {}
    }
    if let Some(format) = &audio.sample_format {
        rows.push(row("Sample format", format.clone()));
    }
}

/// The frame rate of the primary video in plain words; other video streams show their base rate only.
fn frame_rate_text(info: &MediaInfo, video: &VideoStream) -> Option<String> {
    let is_primary = info.primary_video().is_some_and(|(_, v)| std::ptr::eq(v, video));
    let base = video.frame_rate.map(format_frame_rate);
    if !is_primary {
        return base;
    }
    match info.frame_rate_mode() {
        FrameRateMode::Variable {
            min_fps: Some(min),
            max_fps: Some(max),
        } => {
            let mut text = format!("Variable, {} to {} fps", format_fps(min), format_fps(max));
            if let Some(base) = &base {
                let _ = write!(text, " (base {base})");
            }
            Some(text)
        }
        FrameRateMode::Variable { .. } => {
            let mut parts = Vec::new();
            if let Some(base) = &base {
                parts.push(format!("base {base}"));
            }
            if let Some(avg) = video.avg_frame_rate {
                parts.push(format!("average {}", format_frame_rate(avg)));
            }
            Some(if parts.is_empty() {
                "Variable".into()
            } else {
                format!("Variable ({})", parts.join(", "))
            })
        }
        FrameRateMode::Constant | FrameRateMode::Unknown => {
            base.or_else(|| video.avg_frame_rate.map(format_frame_rate))
        }
    }
}

fn scan_text(order: FieldOrder) -> &'static str {
    match order {
        FieldOrder::Progressive => "Progressive",
        FieldOrder::TopFirst => "Interlaced, top field first",
        FieldOrder::BottomFirst => "Interlaced, bottom field first",
        FieldOrder::TopCodedBottomFirst => "Interlaced, top field coded first, bottom shown first",
        FieldOrder::BottomCodedTopFirst => "Interlaced, bottom field coded first, top shown first",
    }
}

/// Primaries, transfer and matrix (one name when they agree) and the range.
fn color_text(color: &ColorInfo) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for name in [&color.primaries, &color.transfer, &color.space].into_iter().flatten() {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let range = color.range.as_deref().map(|r| match r {
        "tv" | "mpeg" | "limited" => "limited range".to_owned(),
        "pc" | "jpeg" | "full" => "full range".to_owned(),
        other => format!("{other} range"),
    });
    match (names.is_empty(), range) {
        (true, None) => None,
        (true, Some(range)) => Some(capitalized(&range)),
        (false, None) => Some(names.join(" / ")),
        (false, Some(range)) => Some(format!("{}, {range}", names.join(" / "))),
    }
}

/// The readable codec name with ffmpeg's short name after it, unless the readable one already says it (`AAC (Advanced
/// Audio Coding)`).
fn codec_text(stream: &Stream) -> Option<String> {
    match (&stream.codec_long, &stream.codec) {
        (Some(long), Some(short)) if !long.to_ascii_lowercase().contains(&short.to_ascii_lowercase()) => {
            Some(format!("{long} ({short})"))
        }
        (Some(name), _) | (None, Some(name)) => Some(name.clone()),
        (None, None) => None,
    }
}

/// `deu` -> `German (deu)`; unknown codes stay as they are.
fn language_text(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    isolang::Language::from_639_3(&lower)
        .or_else(|| isolang::Language::from_639_1(&lower))
        .map_or_else(
            || code.to_owned(),
            |language| format!("{} ({code})", language.to_name()),
        )
}

fn is_cover(stream: &Stream) -> bool {
    matches!(&stream.kind, StreamKind::Video(v) if v.attached_picture)
}

fn row(label: &'static str, value: String) -> InfoRow {
    InfoRow { label, value }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// `16:9` stays a ratio; awkward ones (`427:240`) become `1.78:1`.
fn ratio_text(ratio: Ratio) -> String {
    if ratio.num <= 64 && ratio.den <= 64 {
        format!("{}:{}", ratio.num, ratio.den)
    } else {
        format!("{:.2}:1", ratio.as_f64())
    }
}

/// `10.34 MB` (decimal units, like file managers on Linux).
pub fn format_size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::DECIMAL)
}

/// `4.14 Mbit/s`, `96 kbit/s`, `800 bit/s`.
pub fn format_bit_rate(bits_per_second: u64) -> String {
    let rate = bits_per_second as f64;
    if rate >= 1_000_000.0 {
        format!("{} Mbit/s", trimmed(rate / 1_000_000.0, 2))
    } else if rate >= 1_000.0 {
        format!("{} kbit/s", trimmed(rate / 1_000.0, 0))
    } else {
        format!("{bits_per_second} bit/s")
    }
}

/// `48 kHz`, `44.1 kHz`, `22.05 kHz`.
pub fn format_sample_rate(hz: u32) -> String {
    format!("{} kHz", trimmed(f64::from(hz) / 1000.0, 3))
}

/// `25 fps`, `29.97 fps`, `23.976 fps`.
pub fn format_frame_rate(rate: FrameRate) -> String {
    format!("{} fps", format_fps(rate.as_f64()))
}

/// Up to three decimals without trailing zeros: `25`, `29.97`, `23.976`.
pub fn format_fps(fps: f64) -> String {
    trimmed(fps, 3)
}

fn trimmed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::{Container, FrameTiming};

    #[test]
    fn numbers_in_plain_units() {
        assert_eq!(format_bit_rate(4_136_536), "4.14 Mbit/s");
        assert_eq!(format_bit_rate(96_224), "96 kbit/s");
        assert_eq!(format_bit_rate(800), "800 bit/s");
        assert_eq!(format_bit_rate(2_000_000), "2 Mbit/s");
        assert_eq!(format_sample_rate(48_000), "48 kHz");
        assert_eq!(format_sample_rate(44_100), "44.1 kHz");
        assert_eq!(format_sample_rate(22_050), "22.05 kHz");
        assert_eq!(format_frame_rate(FrameRate::new(25, 1).unwrap()), "25 fps");
        assert_eq!(format_frame_rate(FrameRate::new(30000, 1001).unwrap()), "29.97 fps");
        assert_eq!(format_frame_rate(FrameRate::new(24000, 1001).unwrap()), "23.976 fps");
        assert_eq!(format_size(10_341_341), "10.34 MB");
        assert_eq!(ratio_text(Ratio { num: 16, den: 9 }), "16:9");
        assert_eq!(ratio_text(Ratio { num: 427, den: 240 }), "1.78:1");
        assert_eq!(language_text("deu"), "German (deu)");
        assert_eq!(language_text("en"), "English (en)");
        assert_eq!(language_text("xyz-custom"), "xyz-custom");
    }

    #[test]
    fn colors_are_combined() {
        let bt709 = ColorInfo {
            range: Some("tv".into()),
            space: Some("bt709".into()),
            transfer: Some("bt709".into()),
            primaries: Some("bt709".into()),
        };
        assert_eq!(color_text(&bt709).as_deref(), Some("bt709, limited range"));
        let hdr = ColorInfo {
            range: Some("pc".into()),
            space: Some("bt2020nc".into()),
            transfer: Some("smpte2084".into()),
            primaries: Some("bt2020".into()),
        };
        assert_eq!(
            color_text(&hdr).as_deref(),
            Some("bt2020 / smpte2084 / bt2020nc, full range")
        );
        let range_only = ColorInfo {
            range: Some("tv".into()),
            ..ColorInfo::default()
        };
        assert_eq!(color_text(&range_only).as_deref(), Some("Limited range"));
        assert_eq!(color_text(&ColorInfo::default()), None);
    }

    #[test]
    fn several_streams_of_a_kind_are_numbered_and_attachments_counted() {
        let stream = |index, kind, default| Stream {
            index,
            kind,
            codec: Some("x".into()),
            codec_long: None,
            profile: None,
            bit_rate: None,
            duration: None,
            start_time: None,
            language: None,
            title: None,
            default,
        };
        let info = MediaInfo {
            container: Container::default(),
            streams: vec![
                stream(0, StreamKind::Video(VideoStream::default()), true),
                stream(1, StreamKind::Audio(AudioStream::default()), true),
                stream(2, StreamKind::Audio(AudioStream::default()), false),
                stream(3, StreamKind::Attachment, false),
                stream(4, StreamKind::Attachment, false),
                stream(
                    5,
                    StreamKind::Video(VideoStream {
                        attached_picture: true,
                        ..VideoStream::default()
                    }),
                    false,
                ),
            ],
            frame_timing: Some(FrameTiming {
                frames: 10,
                span_secs: 1.0,
                min_fps: 10.0,
                max_fps: 10.0,
                variable: false,
            }),
        };
        let titles: Vec<String> = summarize(&info).into_iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            [
                "Container",
                "Video",
                "Audio 1 (default)",
                "Audio 2",
                "Cover art",
                "Attachments"
            ]
        );
        let empty = summarize(&MediaInfo::default());
        assert_eq!(to_text(&empty), "[Container]\nStreams: none\n");
    }
}
