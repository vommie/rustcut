//! Editing model: the sections to keep, the filter chain, and the lavfi text that both the mpv preview and the ffmpeg render
//! use (ADR-7, ADR-8).

mod chain;
mod crop;
mod crop_history;
mod filter;
mod filter_complex;
mod lavfi;
mod orientation;
mod preview;
mod ratio;
mod section;
mod section_list;

pub use chain::{ChainError, FINAL_MULTIPLES, FilterChain, FilterSlot, Size, even_multiple, final_multiple};
pub use crop::{Crop, CropAspect};
pub use crop_history::{CropHistory, CropMemory, DEFAULT_HISTORY_LENGTH, Fit, HISTORY_LENGTHS, fit};
pub use filter::{
    DeinterlaceRate, Deinterlacer, DeshakeCrop, DeshakeInterpolation, DeshakeParams, FieldParity, Filter, FilterKind,
    Mirror, ResizeAlgorithm, Rotation,
};
pub use filter_complex::{ConcatGraph, ConcatInput, concat_graph};
pub use lavfi::{DeshakePass, LavfiTarget, escape_value};
pub use orientation::{Orientation, OrientationParams};
pub use preview::{PreviewFilter, source_rotation_lavfi};
pub use ratio::{
    AspectRatio, DEFAULT_RATIOS, FREE_LABEL, MAX_RATIOS, ORIGINAL_LABEL, RatioChoice, RatioError, RatioPreset,
    RatioSet, RatioShape,
};
pub use section::{Section, SectionError};
pub use section_list::{SectionId, SectionList};
