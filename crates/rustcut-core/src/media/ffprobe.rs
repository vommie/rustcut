//! Decoding ffprobe's JSON (`-print_format json -show_format -show_streams -show_error` plus a packet sample) into a
//! [`MediaInfo`]. Tolerant on purpose: numbers may come as strings (`"bit_rate": "1173907"`), values may be `N/A` or
//! missing, and one field that does not fit becomes `None` instead of failing the whole probe.

use std::collections::BTreeMap;

use az::CheckedAs as _;
use serde::Deserialize;
use serde_with::{DefaultOnError, DisplayFromStr, PickFirst, serde_as};

use super::model::{
    AudioStream, ColorInfo, Container, FieldOrder, FrameTiming, MediaInfo, Ratio, Stream, StreamKind, VideoStream,
};
use super::timing;
use crate::time::{FrameRate, Timestamp};

/// Why ffprobe's output is not a media description.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// ffprobe reported an error for the file (its `error` object): broken, not a media file, no access.
    #[error("{0}")]
    Reported(String),
    #[error("ffprobe's output is not readable: {0}")]
    Malformed(String),
}

type Tags = BTreeMap<String, serde_json::Value>;

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawProbe {
    #[serde_as(as = "DefaultOnError")]
    streams: Vec<RawStream>,
    #[serde_as(as = "DefaultOnError")]
    format: Option<RawFormat>,
    #[serde_as(as = "DefaultOnError")]
    packets: Vec<RawPacket>,
    #[serde_as(as = "DefaultOnError")]
    error: Option<RawError>,
}

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawFormat {
    #[serde_as(as = "DefaultOnError")]
    format_name: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    format_long_name: Option<String>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    duration: Option<f64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    start_time: Option<f64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    bit_rate: Option<u64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    size: Option<u64>,
    #[serde_as(as = "DefaultOnError")]
    tags: Tags,
}

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawStream {
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    index: Option<u32>,
    #[serde_as(as = "DefaultOnError")]
    codec_type: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    codec_name: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    codec_long_name: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    profile: Option<String>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    bit_rate: Option<u64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    duration: Option<f64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    start_time: Option<f64>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    width: Option<u32>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    height: Option<u32>,
    #[serde_as(as = "DefaultOnError")]
    sample_aspect_ratio: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    display_aspect_ratio: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    pix_fmt: Option<String>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    bits_per_raw_sample: Option<u8>,
    #[serde_as(as = "DefaultOnError")]
    r_frame_rate: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    avg_frame_rate: Option<String>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    nb_frames: Option<u64>,
    #[serde_as(as = "DefaultOnError")]
    field_order: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    color_range: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    color_space: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    color_transfer: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    color_primaries: Option<String>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    sample_rate: Option<u32>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    channels: Option<u16>,
    #[serde_as(as = "DefaultOnError")]
    channel_layout: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    sample_fmt: Option<String>,
    #[serde_as(as = "DefaultOnError")]
    disposition: Tags,
    #[serde_as(as = "DefaultOnError")]
    tags: Tags,
    #[serde_as(as = "DefaultOnError")]
    side_data_list: Vec<RawSideData>,
}

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawSideData {
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    rotation: Option<f64>,
}

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawPacket {
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    stream_index: Option<u32>,
    #[serde_as(as = "DefaultOnError<Option<PickFirst<(_, DisplayFromStr)>>>")]
    pts_time: Option<f64>,
}

#[serde_as]
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawError {
    #[serde_as(as = "DefaultOnError")]
    string: Option<String>,
}

/// Decodes ffprobe's JSON output. An `error` object from ffprobe becomes [`DecodeError::Reported`].
pub fn decode(json: &[u8]) -> Result<MediaInfo, DecodeError> {
    let raw: RawProbe = serde_json::from_slice(json).map_err(|e| DecodeError::Malformed(e.to_string()))?;
    if let Some(error) = raw.error {
        return Err(DecodeError::Reported(
            error
                .string
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "ffprobe could not read the file".to_owned()),
        ));
    }
    let format = raw.format.unwrap_or_default();
    let container = Container {
        format: non_empty(format.format_name),
        format_long: non_empty(format.format_long_name),
        duration: format.duration.and_then(seconds).filter(|d| !d.is_negative()),
        start_time: format.start_time.and_then(seconds),
        bit_rate: format.bit_rate.filter(|b| *b > 0),
        size: format.size,
        title: tag(&format.tags, "title"),
    };
    let streams: Vec<Stream> = raw
        .streams
        .into_iter()
        .enumerate()
        .map(|(position, s)| stream(position, s))
        .collect();
    let mut info = MediaInfo {
        container,
        streams,
        frame_timing: None,
    };
    if let Some((primary, _)) = info.primary_video() {
        let index = primary.index;
        let pts: Vec<f64> = raw
            .packets
            .iter()
            .filter(|p| p.stream_index == Some(index))
            .filter_map(|p| p.pts_time)
            .collect();
        info.frame_timing = timing::analyze(&pts);
    }
    Ok(info)
}

/// The frame spacing in the output of the timing sample (`-select_streams N -show_entries packet=pts_time`); `None` for
/// output that is not JSON or holds too few timestamps.
pub fn decode_timing(json: &[u8]) -> Option<FrameTiming> {
    let raw: RawProbe = serde_json::from_slice(json).ok()?;
    let pts: Vec<f64> = raw.packets.iter().filter_map(|p| p.pts_time).collect();
    timing::analyze(&pts)
}

fn stream(position: usize, raw: RawStream) -> Stream {
    let kind = match raw.codec_type.as_deref() {
        Some("video") => StreamKind::Video(video(&raw)),
        Some("audio") => StreamKind::Audio(AudioStream {
            sample_rate: raw.sample_rate.filter(|r| *r > 0),
            channels: raw.channels.filter(|c| *c > 0),
            channel_layout: non_empty(raw.channel_layout.clone()),
            sample_format: non_empty(raw.sample_fmt.clone()),
        }),
        Some("subtitle") => StreamKind::Subtitle,
        Some("data") => StreamKind::Data,
        Some("attachment") => StreamKind::Attachment,
        Some(other) => StreamKind::Other(other.to_owned()),
        None => StreamKind::Other("unknown".to_owned()),
    };
    let duration = raw
        .duration
        .and_then(seconds)
        .or_else(|| tag(&raw.tags, "duration").and_then(|d| Timestamp::parse(&d).ok()))
        .filter(|d| !d.is_negative());
    Stream {
        index: raw.index.unwrap_or_else(|| u32::try_from(position).unwrap_or(u32::MAX)),
        kind,
        codec: non_empty(raw.codec_name),
        codec_long: non_empty(raw.codec_long_name),
        profile: non_empty(raw.profile),
        bit_rate: raw
            .bit_rate
            .or_else(|| tag(&raw.tags, "bps").and_then(|b| b.parse().ok()))
            .filter(|b| *b > 0),
        duration,
        start_time: raw.start_time.and_then(seconds),
        language: tag(&raw.tags, "language").filter(|l| !l.eq_ignore_ascii_case("und")),
        title: tag(&raw.tags, "title"),
        default: flag(&raw.disposition, "default"),
    }
}

fn video(raw: &RawStream) -> VideoStream {
    let pixel_format = non_empty(raw.pix_fmt.clone());
    VideoStream {
        width: raw.width.filter(|w| *w > 0),
        height: raw.height.filter(|h| *h > 0),
        rotation: rotation(raw),
        sample_aspect: raw.sample_aspect_ratio.as_deref().and_then(ratio),
        display_aspect: raw.display_aspect_ratio.as_deref().and_then(ratio),
        bit_depth: raw
            .bits_per_raw_sample
            .filter(|b| *b > 0)
            .or_else(|| pixel_format.as_deref().and_then(bit_depth_of)),
        pixel_format,
        frame_rate: raw.r_frame_rate.as_deref().and_then(|r| r.parse::<FrameRate>().ok()),
        avg_frame_rate: raw.avg_frame_rate.as_deref().and_then(|r| r.parse::<FrameRate>().ok()),
        frame_count: raw.nb_frames.filter(|n| *n > 0),
        field_order: raw.field_order.as_deref().and_then(FieldOrder::from_ffprobe),
        color: ColorInfo {
            range: known(raw.color_range.clone()),
            space: known(raw.color_space.clone()),
            transfer: known(raw.color_transfer.clone()),
            primaries: known(raw.color_primaries.clone()),
        },
        attached_picture: flag(&raw.disposition, "attached_pic"),
    }
}

/// Display rotation clockwise in whole degrees (0..360), as mpv computes it: ffprobe's display-matrix `rotation` is
/// counter-clockwise; the old `rotate` tag (before ffmpeg moved it into side data) is clockwise.
fn rotation(raw: &RawStream) -> u16 {
    raw.side_data_list
        .iter()
        .find_map(|d| d.rotation)
        .map(|ccw| -ccw)
        .or_else(|| tag(&raw.tags, "rotate").and_then(|r| r.parse::<f64>().ok()))
        .and_then(|degrees| degrees.round().checked_as::<i64>())
        .and_then(|degrees| u16::try_from(degrees.rem_euclid(360)).ok())
        .unwrap_or(0)
}

/// `16:9` -> 16/9; `0:1`, `N/A` and garbage -> `None`.
fn ratio(text: &str) -> Option<Ratio> {
    let (num, den) = text.split_once(':')?;
    Ratio::new(num.trim().parse().ok()?, den.trim().parse().ok()?)
}

/// Pixel formats with eight bits per component whose names carry no depth.
const EIGHT_BIT_FORMATS: &[&str] = &[
    "yuv410p", "yuv411p", "yuv420p", "yuv422p", "yuv440p", "yuv444p", "yuvj420p", "yuvj422p", "yuvj444p", "yuva420p",
    "yuva444p", "nv12", "nv16", "nv21", "nv24", "gray", "gbrp", "gbrap", "rgb24", "bgr24", "rgba", "bgra", "argb",
    "abgr", "yuyv422", "uyvy422",
];

/// Bits per component read from a pixel format name: `yuv420p10le` -> 10, `p010le` -> 10, `gray12le` -> 12,
/// `yuv420p` / `nv12` -> 8. `None` when the name does not say (packed, float or exotic formats).
fn bit_depth_of(pix_fmt: &str) -> Option<u8> {
    let name = pix_fmt
        .strip_suffix("le")
        .or_else(|| pix_fmt.strip_suffix("be"))
        .unwrap_or(pix_fmt);
    if EIGHT_BIT_FORMATS.contains(&name) {
        return Some(8);
    }
    // Semi-planar high-depth formats: p010, p012, p016.
    if let Some(depth) = name.strip_prefix('p').and_then(|d| d.parse::<u8>().ok()) {
        return Some(depth);
    }
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let depth = name.get(stem.len()..).and_then(|d| d.parse::<u8>().ok())?;
    let planar = stem.ends_with('p') && ["yuv", "gbr"].iter().any(|family| stem.starts_with(family));
    (planar || stem == "gray").then_some(depth)
}

/// Seconds to a timestamp; `None` for values that do not fit.
fn seconds(secs: f64) -> Option<Timestamp> {
    Timestamp::from_secs_f64(secs)
}

/// A tag by case-insensitive name; Matroska's language-suffixed duplicates (`DURATION-eng`) count when the plain one is
/// missing.
fn tag(tags: &Tags, name: &str) -> Option<String> {
    let value = tags
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .or_else(|| {
            tags.iter().find(|(key, _)| {
                key.split_once('-')
                    .is_some_and(|(base, _)| base.eq_ignore_ascii_case(name))
            })
        })
        .map(|(_, value)| value)?;
    match value {
        serde_json::Value::String(s) => non_empty(Some(s.clone())),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A disposition flag (`1` = set).
fn flag(disposition: &Tags, name: &str) -> bool {
    disposition
        .get(name)
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|v| v != 0)
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty() && t != "N/A")
}

/// A color property; ffprobe writes `unknown` when it is not set.
fn known(text: Option<String>) -> Option<String> {
    non_empty(text).filter(|t| t != "unknown" && t != "unspecified")
}

#[cfg(test)]
#[path = "ffprobe_tests.rs"]
mod tests;
