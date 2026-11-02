//! Loads and runs RustCut plugins: WebAssembly components of the world in `rustcut-plugin-api/wit`.
//!
//! - [`manifest`]: `plugin.toml` (who a plugin is, its settings as cards of fields).
//! - [`values`]: setting values, their defaults and sanitizing, TOML <-> JSON.
//! - [`package`]: plugin files (`.rcplugin`, tar.zst): read, stage, commit, pack.
//! - [`runtime`]: the sandbox (engine, compile cache, instances with time and memory limits, a thread per plugin).
//! - [`types`]: the values exchanged with a plugin, on RustCut's side.
//!
//! Nothing here touches RustCut's settings or UI; `rustcut-core::plugins` runs the plugins as a service.

pub mod manifest;
pub mod package;
pub mod runtime;
pub mod types;
pub mod values;
