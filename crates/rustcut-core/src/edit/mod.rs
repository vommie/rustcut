//! Editing model: the sections to keep, the filter chain, and the lavfi text that both the mpv preview and the ffmpeg render
//! use (ADR-7, ADR-8).

mod chain;
mod crop;
mod filter;
mod filter_complex;
mod lavfi;
mod section;
mod section_list;

pub use chain::{ChainError, FilterChain, FilterSlot, Size};
pub use crop::Crop;
pub use filter::{
    DeinterlaceRate, Deinterlacer, DeshakeCrop, DeshakeInterpolation, DeshakeParams, Filter, FilterKind, Mirror,
    ResizeAlgorithm, Rotation,
};
pub use filter_complex::{ConcatGraph, ConcatInput, concat_graph};
pub use lavfi::{DeshakePass, LavfiTarget, escape_value};
pub use section::{Section, SectionError};
pub use section_list::{SectionId, SectionList};
