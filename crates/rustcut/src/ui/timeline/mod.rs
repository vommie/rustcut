//! The timeline under the video (ADR-12 layout, user decision 2026-10-23: its own strip, the transport bar keeps no seek
//! bar): a time ruler, every section of the list, the current section's marks, the playhead, the time under the pointer,
//! zoom and an overview strip while zoomed. Like the dock, the geometry is pure Rust ([`Viewport`], [`ticks`], [`hit`],
//! [`view`]) and Slint only draws what it computes; pointer hits use the same numbers.

pub mod hit;
pub mod ticks;
pub mod view;
pub mod viewport;

pub use hit::{Area, Hit};
pub use view::TimelineView;
pub use viewport::Viewport;
