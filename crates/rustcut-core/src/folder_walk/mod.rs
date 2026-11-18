//! "Next in Folder": the videos of a source's folder in the order a file manager shows them (Dolphin's order when it
//! is asked to, else names A to Z), and what Dolphin says about a folder over D-Bus or in its stored view properties.

pub mod cache;
pub mod dolphin;
pub mod listing;
pub mod natural;
pub mod order;
pub mod type_name;

pub use cache::FolderCache;
pub use listing::{FileManagerMode, HiddenFiles, Listing, NamesBecause, OrderSource, Request, list};
