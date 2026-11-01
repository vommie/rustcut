//! The pictures of the folder tree (P11.2, user decision Q-FP2): own images and SVGs, prepared once (validated, scaled
//! down, sanitized) and kept in `pictures.sqlite` by their content hash. Built-in icons are the app's (by name).

mod actor;
mod import;
mod store;
#[cfg(test)]
mod tests;

pub use actor::{PictureCommand, PictureHandle, spawn_picture_store};
pub use import::{
    MAX_INPUT, MAX_SIDE, PictureError, PictureKind, Prepared, Rgba, WEBP_QUALITY, decode_webp, prepare, render_svg,
    spoke_picture,
};
pub use store::{KEEP_NEW_SECS, PictureStore, SCHEMA_VERSION, StoredPicture};

/// The file inside the data folder.
pub const FILE_NAME: &str = "pictures.sqlite";
