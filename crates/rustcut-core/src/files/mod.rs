//! Files RustCut moves on the user's request (P12 block 7): a target file that follows its job's new target.

mod place;
mod relocate;
mod sidecar;

pub use place::{Place, place};
pub use relocate::{Interrupted, Method, RelocateError, needs_copy, relocate, settle_interrupted, temp_path};
pub use sidecar::carry_sidecar;
