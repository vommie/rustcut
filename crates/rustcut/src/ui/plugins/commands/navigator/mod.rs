//! The folder navigator (Q-NF3): the bubble with the open video's folder, its search and pages. `pattern` and `state`
//! are pure; the bridge lives in `bridge.rs`.

mod bridge;
pub mod pattern;
pub mod state;

pub(super) use bridge::NavigatorUi;
