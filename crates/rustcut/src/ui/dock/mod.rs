//! The dock (ADR-12): the pure layout model, its geometry, its place in `ui-state.toml`, the rows Slint shows and the
//! bridge that connects them.

mod bridge;
pub mod model;
pub mod persist;
pub mod plan;
mod view;

#[cfg(test)]
mod tests;

pub use bridge::DockBridge;
pub use model::{DockLayout, DropTarget, GroupRef, PanelId, Side};
