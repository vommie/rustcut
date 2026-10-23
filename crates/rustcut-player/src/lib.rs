//! RustCut player: libmpv's render API drawing into an OpenGL texture on Slint's context (ADR-2 in `HANDOFF.md`).
//!
//! This is the only crate of the workspace that may contain `unsafe` code (libmpv and OpenGL FFI). Every `unsafe` block carries
//! a `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` is denied), and only the modules that talk to FFI allow
//! `unsafe_code`, each with a reason. The engine arrives with milestone M1.1 (player spike).
