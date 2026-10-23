//! What RustCut knows about a media file: its container and streams as ffprobe reports them. Every field is optional,
//! because files and ffprobe versions leave different things out; a missing value is shown as unknown, never guessed.

use crate::time::{FrameRate, Timestamp};

/// A probed media file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub container: Container,
    /// In file order (`index` = ffmpeg's stream index).
    pub streams: Vec<Stream>,
    /// Frame spacing of the primary video stream in the first seconds of the file (from packet timestamps).
    pub frame_timing: Option<FrameTiming>,
}

/// The container (ffprobe's `format` section).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Container {
    /// Short format names, comma-separated (`mov,mp4,m4a,3gp,3g2,mj2`).
    pub format: Option<String>,
    /// Readable format name (`QuickTime / MOV`).
    pub format_long: Option<String>,
    pub duration: Option<Timestamp>,
    pub start_time: Option<Timestamp>,
    /// Overall bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// File size in bytes.
    pub size: Option<u64>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// ffmpeg's stream index (`0:<index>`).
    pub index: u32,
    pub kind: StreamKind,
    /// Short codec name (`h264`, `aac`).
    pub codec: Option<String>,
    /// Readable codec name (`H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10`).
    pub codec_long: Option<String>,
    pub profile: Option<String>,
    /// Bits per second.
    pub bit_rate: Option<u64>,
    pub duration: Option<Timestamp>,
    pub start_time: Option<Timestamp>,
    /// ISO 639 code as stored (`eng`, `deu`); `und` is treated as unknown.
    pub language: Option<String>,
    pub title: Option<String>,
    /// The container marks this stream as the default of its kind.
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamKind {
    Video(VideoStream),
    Audio(AudioStream),
    Subtitle,
    Data,
    /// Fonts and other files inside the container (Matroska).
    Attachment,
    /// A type ffprobe reports that RustCut does not know, or none at all.
    Other(String),
}

impl StreamKind {
    /// Lowercase name of the kind (`video`, `audio`, ...).
    pub fn name(&self) -> &str {
        match self {
            Self::Video(_) => "video",
            Self::Audio(_) => "audio",
            Self::Subtitle => "subtitle",
            Self::Data => "data",
            Self::Attachment => "attachment",
            Self::Other(name) => name,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoStream {
    /// Stored size in pixels (before rotation).
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Display rotation in degrees clockwise (0, 90, 180, 270; mpv's `video-params/rotate`), from the display matrix.
    pub rotation: u16,
    /// Pixel aspect ratio (`None` when unknown or `0:1`).
    pub sample_aspect: Option<Ratio>,
    /// Picture aspect ratio as stored.
    pub display_aspect: Option<Ratio>,
    /// ffmpeg's pixel format name (`yuv420p`, `yuv420p10le`).
    pub pixel_format: Option<String>,
    /// Bits per color component (stated by the stream, else read from the pixel format).
    pub bit_depth: Option<u8>,
    /// The base rate all timestamps fit in (ffprobe's `r_frame_rate`).
    pub frame_rate: Option<FrameRate>,
    /// Frames divided by duration (ffprobe's `avg_frame_rate`).
    pub avg_frame_rate: Option<FrameRate>,
    /// Frame count stored in the container (not counted).
    pub frame_count: Option<u64>,
    pub field_order: Option<FieldOrder>,
    pub color: ColorInfo,
    /// Cover art (an attached picture), not a video track.
    pub attached_picture: bool,
}

impl VideoStream {
    /// The size as shown: width and height swapped for a rotation of 90 or 270 degrees.
    pub fn oriented_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.width?, self.height?);
        Some(if self.rotation % 180 == 90 { (h, w) } else { (w, h) })
    }

    /// The pixels are not square (anamorphic video).
    pub fn is_anamorphic(&self) -> bool {
        self.sample_aspect.is_some_and(|r| r.num != r.den)
    }

    /// The stream says it is interlaced (a container flag; the content may differ, M2.3 checks it with `idet`).
    pub fn is_interlaced(&self) -> bool {
        self.field_order.is_some_and(FieldOrder::is_interlaced)
    }
}

/// Whole numbers `num:den`, both positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ratio {
    pub num: u32,
    pub den: u32,
}

impl Ratio {
    pub const fn new(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            None
        } else {
            Some(Self { num, den })
        }
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

/// Scan type as the stream declares it (ffprobe's `field_order`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldOrder {
    Progressive,
    /// `tt`: top field coded and shown first.
    TopFirst,
    /// `bb`: bottom field coded and shown first.
    BottomFirst,
    /// `tb`: top coded first, bottom shown first.
    TopCodedBottomFirst,
    /// `bt`: bottom coded first, top shown first.
    BottomCodedTopFirst,
}

impl FieldOrder {
    /// ffprobe's names; `unknown` and anything else are `None`.
    pub fn from_ffprobe(name: &str) -> Option<Self> {
        match name {
            "progressive" => Some(Self::Progressive),
            "tt" => Some(Self::TopFirst),
            "bb" => Some(Self::BottomFirst),
            "tb" => Some(Self::TopCodedBottomFirst),
            "bt" => Some(Self::BottomCodedTopFirst),
            _ => None,
        }
    }

    pub fn is_interlaced(self) -> bool {
        self != Self::Progressive
    }
}

/// Color description as stored (ffmpeg's names: `bt709`, `smpte2084`, `tv`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorInfo {
    pub range: Option<String>,
    pub space: Option<String>,
    pub transfer: Option<String>,
    pub primaries: Option<String>,
}

impl ColorInfo {
    pub fn is_empty(&self) -> bool {
        self.range.is_none() && self.space.is_none() && self.transfer.is_none() && self.primaries.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioStream {
    /// Samples per second.
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    /// ffmpeg's layout name (`mono`, `stereo`, `5.1(side)`).
    pub channel_layout: Option<String>,
    /// ffmpeg's sample format name (`fltp`, `s16`).
    pub sample_format: Option<String>,
}

/// How evenly the frames of the primary video stream are spaced in a sample of packet timestamps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTiming {
    /// Frames in the sample.
    pub frames: usize,
    /// Seconds the sample covers.
    pub span_secs: f64,
    /// Frames per second over the longest and the shortest gap between two frames.
    pub min_fps: f64,
    pub max_fps: f64,
    /// More than a stray gap differs clearly from the usual spacing.
    pub variable: bool,
}

/// Whether the frame rate is constant, as far as the probe can tell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameRateMode {
    Constant,
    /// Variable frame rate; the range comes from the sampled timestamps when they showed it.
    Variable {
        min_fps: Option<f64>,
        max_fps: Option<f64>,
    },
    Unknown,
}

/// Relative difference between the base and the average frame rate above which the average says "variable" even when the
/// sampled start of the file looked even (one frame more or less in an eight-second clip stays below it).
const AVERAGE_RATE_TOLERANCE: f64 = 0.02;

impl MediaInfo {
    /// The video stream a player shows: the default one, else the first, never cover art.
    pub fn primary_video(&self) -> Option<(&Stream, &VideoStream)> {
        let mut videos = self.streams.iter().filter_map(|s| match &s.kind {
            StreamKind::Video(v) if !v.attached_picture => Some((s, v)),
            _ => None,
        });
        let first = videos.next()?;
        Some(
            std::iter::once(first)
                .chain(videos)
                .find(|(s, _)| s.default)
                .unwrap_or(first),
        )
    }

    pub fn audio_streams(&self) -> impl Iterator<Item = (&Stream, &AudioStream)> {
        self.streams.iter().filter_map(|s| match &s.kind {
            StreamKind::Audio(a) => Some((s, a)),
            _ => None,
        })
    }

    pub fn has_audio(&self) -> bool {
        self.audio_streams().next().is_some()
    }

    /// The container's duration, else the longest stream.
    pub fn duration(&self) -> Option<Timestamp> {
        self.container
            .duration
            .or_else(|| self.streams.iter().filter_map(|s| s.duration).max())
    }

    /// Constant or variable frame rate of the primary video stream: variable when the sampled timestamps are uneven or
    /// the average rate differs clearly from the base rate (Matroska stores no frame count, so there only the timestamps
    /// tell).
    pub fn frame_rate_mode(&self) -> FrameRateMode {
        let Some((_, video)) = self.primary_video() else {
            return FrameRateMode::Unknown;
        };
        let rates_differ = match (video.frame_rate, video.avg_frame_rate) {
            (Some(base), Some(avg)) => {
                Some((base.as_f64() - avg.as_f64()).abs() / base.as_f64() > AVERAGE_RATE_TOLERANCE)
            }
            _ => None,
        };
        match (self.frame_timing, rates_differ) {
            (Some(t), _) if t.variable => FrameRateMode::Variable {
                min_fps: Some(t.min_fps),
                max_fps: Some(t.max_fps),
            },
            (_, Some(true)) => FrameRateMode::Variable {
                min_fps: None,
                max_fps: None,
            },
            (Some(_), _) | (None, Some(false)) => FrameRateMode::Constant,
            (None, None) => FrameRateMode::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(rotation: u16) -> VideoStream {
        VideoStream {
            width: Some(640),
            height: Some(360),
            rotation,
            ..VideoStream::default()
        }
    }

    fn stream(index: u32, kind: StreamKind, default: bool) -> Stream {
        Stream {
            index,
            kind,
            codec: None,
            codec_long: None,
            profile: None,
            bit_rate: None,
            duration: None,
            start_time: None,
            language: None,
            title: None,
            default,
        }
    }

    #[test]
    fn rotation_swaps_the_shown_size() {
        assert_eq!(video(0).oriented_size(), Some((640, 360)));
        assert_eq!(video(90).oriented_size(), Some((360, 640)));
        assert_eq!(video(180).oriented_size(), Some((640, 360)));
        assert_eq!(video(270).oriented_size(), Some((360, 640)));
        assert_eq!(VideoStream::default().oriented_size(), None);
    }

    #[test]
    fn the_primary_video_skips_cover_art_and_prefers_the_default() {
        let cover = VideoStream {
            attached_picture: true,
            ..video(0)
        };
        let info = MediaInfo {
            streams: vec![
                stream(0, StreamKind::Video(cover), true),
                stream(1, StreamKind::Video(video(0)), false),
                stream(2, StreamKind::Video(video(90)), true),
            ],
            ..MediaInfo::default()
        };
        assert_eq!(info.primary_video().map(|(s, _)| s.index), Some(2));
        let info = MediaInfo {
            streams: vec![
                stream(0, StreamKind::Audio(AudioStream::default()), true),
                stream(1, StreamKind::Video(video(0)), false),
                stream(2, StreamKind::Video(video(0)), false),
            ],
            ..MediaInfo::default()
        };
        assert_eq!(info.primary_video().map(|(s, _)| s.index), Some(1));
        assert!(info.has_audio());
    }

    #[test]
    fn frame_rate_mode_combines_timestamps_and_rates() {
        let rate = |n, d| FrameRate::new(n, d);
        let with = |base, avg, timing| MediaInfo {
            streams: vec![stream(
                0,
                StreamKind::Video(VideoStream {
                    frame_rate: base,
                    avg_frame_rate: avg,
                    ..video(0)
                }),
                false,
            )],
            frame_timing: timing,
            ..MediaInfo::default()
        };
        let even = FrameTiming {
            frames: 250,
            span_secs: 10.0,
            min_fps: 25.0,
            max_fps: 25.0,
            variable: false,
        };
        let uneven = FrameTiming {
            min_fps: 10.0,
            max_fps: 30.0,
            variable: true,
            ..even
        };
        assert_eq!(
            with(rate(25, 1), rate(25, 1), Some(even)).frame_rate_mode(),
            FrameRateMode::Constant
        );
        assert_eq!(
            with(rate(25, 1), rate(25, 1), None).frame_rate_mode(),
            FrameRateMode::Constant
        );
        // Matroska: the average equals the base rate, only the timestamps show it.
        assert_eq!(
            with(rate(30, 1), rate(30, 1), Some(uneven)).frame_rate_mode(),
            FrameRateMode::Variable {
                min_fps: Some(10.0),
                max_fps: Some(30.0)
            }
        );
        // MP4: the average gives it away even when the sampled start was even.
        assert_eq!(
            with(rate(30, 1), rate(400, 19), Some(even)).frame_rate_mode(),
            FrameRateMode::Variable {
                min_fps: None,
                max_fps: None
            }
        );
        // One frame more or less in a short clip is not variable.
        assert_eq!(
            with(rate(25, 1), rate(499, 20), None).frame_rate_mode(),
            FrameRateMode::Constant
        );
        assert_eq!(with(None, None, None).frame_rate_mode(), FrameRateMode::Unknown);
        assert_eq!(MediaInfo::default().frame_rate_mode(), FrameRateMode::Unknown);
    }
}
