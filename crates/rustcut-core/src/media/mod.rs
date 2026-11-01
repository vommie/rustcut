//! Media probing (ADR-3): ffprobe's JSON as a typed, tolerant [`MediaInfo`] (container, streams, rotation, frame rate incl.
//! a variable-frame-rate check from packet timestamps), the supervised ffprobe runner, the readable summary the Media
//! Info panel shows, previews of files (description and one picture, cached), and the picture checks of the filters (black borders with `cropdetect`, interlacing with
//! `idet`).

mod crop_search;
mod detect;
mod ffprobe;
mod formats;
mod model;
mod pixel;
mod preview;
pub(crate) mod probe;
mod summary;
mod timing;

pub use crop_search::{CROP_SAMPLES, CropSearch, CropSides, crop_sample_times};
pub use detect::{
    CropBox, CropDetection, CropProgress, DetectOptions, IdetCounts, Interlacing, combine_crop_samples,
    cropdetect_args, detect_crop, detect_crop_retrying, detect_crop_watched, detect_interlace, idet_args, idet_start,
    parse_cropdetect, parse_idet,
};
pub use ffprobe::{DecodeError, decode, decode_timing};
pub use formats::{VIDEO_FORMATS, is_video_file, video_extensions};
pub use model::{
    AudioStream, ColorInfo, Container, FieldOrder, FrameRateMode, FrameTiming, MediaInfo, Ratio, Stream, StreamKind,
    VideoStream,
};
pub use pixel::{pick_args, pick_color};
pub use preview::{
    DEFAULT_PARALLEL, Preview, PreviewBatch, PreviewError, PreviewEvent, PreviewOptions, PreviewPrograms,
    PreviewRequest, PreviewService, PreviewSlot, THUMB_HEIGHT, THUMB_WIDTH, Thumbnail, picture_time, thumbnail_args,
};
pub(crate) use probe::run as run_tool;
pub use probe::{
    PROBE_TIMEOUT, ProbeError, ProbeOptions, TIMING_SAMPLE_SECS, TIMING_TIMEOUT, probe, probe_args, probe_frame_timing,
    probe_with_timing, timing_args,
};
pub use summary::{
    InfoRow, InfoSection, format_bit_rate, format_fps, format_frame_rate, format_sample_rate, format_size, summarize,
    to_text,
};
