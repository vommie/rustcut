//! RustCut player: libmpv's render API drawing into an OpenGL texture on Slint's context (ADR-2 in `HANDOFF.md`).
//!
//! - [`Player`] owns the mpv core: options, the command thread (every libmpv call except rendering) and the event thread with
//!   the observed properties (safe code only).
//! - [`attach_surface`] installs Slint's rendering notifier on a window: mpv draws each new frame into a framebuffer texture
//!   while Slint renders, and the app shows that texture as a `slint::Image`. mpv's update callback requests the redraw.
//! - [`FrameStats`] measures how long the video frames take (logged while playing and when the surface goes away).
//!
//! This is the only crate of the workspace that may contain `unsafe` code (libmpv and OpenGL FFI). Every `unsafe` block carries
//! a `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` is denied), and only the modules that talk to FFI allow
//! `unsafe_code`, each with a reason.

mod engine;
mod stats;
mod surface;

pub use engine::{
    AudioOptions, EndReason, MAX_VOLUME, OpenOptions, Player, PlayerError, PlayerEvent, PlayerHandle, PlayerOptions,
    PreviewQuality, Property, VideoFilter,
};
pub use stats::{FrameReport, FrameSample, FrameStats, Spread};
pub use surface::{SurfaceError, SurfaceHooks, attach_surface};
