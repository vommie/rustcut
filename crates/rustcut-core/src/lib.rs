//! RustCut core: everything that needs no user interface.
//!
//! This crate never depends on the app crate or on generated Slint code. Its modules are the domain model (time, sections,
//! filters), render planning and the ffmpeg runner, the queue engine, the SQLite store, settings, the RustView client and the
//! hash service (see `HANDOFF.md`, section 5).

pub mod actor;
pub mod app_identity;
pub mod diagnostics;
pub mod edit;
pub mod external;
pub mod folders;
pub mod hashing;
pub mod job;
pub mod media;
pub mod naming;
pub mod process;
pub mod queue;
pub mod redact;
pub mod render;
pub mod rustview;
pub mod settings;
pub mod store;
pub mod time;
