//! Media probing (ADR-3): ffprobe's JSON as a typed, tolerant [`MediaInfo`] (container, streams, rotation, frame rate incl.
//! a variable-frame-rate check from packet timestamps), the supervised ffprobe runner, and the readable summary the
//! Media Info panel shows. Interlace detection (`idet`) and crop detection follow with the filters (M2.3).

mod ffprobe;
mod model;
mod probe;
mod summary;
mod timing;

pub use ffprobe::{DecodeError, decode, decode_timing};
pub use model::{
    AudioStream, ColorInfo, Container, FieldOrder, FrameRateMode, FrameTiming, MediaInfo, Ratio, Stream, StreamKind,
    VideoStream,
};
pub use probe::{
    PROBE_TIMEOUT, ProbeError, ProbeOptions, TIMING_SAMPLE_SECS, TIMING_TIMEOUT, probe, probe_args, probe_frame_timing,
    probe_with_timing, timing_args,
};
pub use summary::{
    InfoRow, InfoSection, format_bit_rate, format_fps, format_frame_rate, format_sample_rate, format_size, summarize,
    to_text,
};
