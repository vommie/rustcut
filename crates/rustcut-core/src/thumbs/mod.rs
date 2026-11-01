//! The timeline's thumbnails (P12 block 6, user decision Q-P12e): which pictures, in which order (`plan`), the pictures of
//! the open video in memory (`set`), the job-based cache `tl_thumbs.db` (`store`), who makes them (`generator`: the helper
//! `rustcut-thumbs`, or ffmpeg when it cannot start) and the actor around them (`service`).

mod generator;
mod plan;
mod service;
mod set;
mod still;
mod store;
mod view;

pub use generator::helper_program;
pub use plan::{Grid, MIN_SPACING_MS, automatic_workers, level_order, niceness, spacing_ms};
pub use service::{
    LONG_SIDE, ThumbCommand, ThumbConfig, ThumbEvent, ThumbHandle, ThumbService, ThumbShared, ThumbVideo, spawn_thumbs,
};
pub use set::{Thumb, ThumbSet};
pub use still::still;
pub use store::{ThumbStats, ThumbStore, ThumbStoreCommand, ThumbStoreError, ThumbStoreHandle, spawn_thumb_store};
pub use view::{ThumbView, render};

#[cfg(test)]
mod tests;
