//! RustCut: the desktop app (Slint UI, player view, panels, desktop integration).

/// The code Slint generates from `ui/*.slint`. It is not ours to lint: the generator decides how it is written.
mod generated {
    #![allow(
        unsafe_code,
        trivial_casts,
        trivial_numeric_casts,
        unused_qualifications,
        rust_2018_idioms,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        clippy::nursery
    )]
    slint::include_modules!();
}
pub use generated::*;

pub mod app;
pub mod ui;
