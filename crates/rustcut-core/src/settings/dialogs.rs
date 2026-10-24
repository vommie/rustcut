//! `[dialogs]` in `config.toml`: how RustCut's dialogs behave (the Settings window's Dialogs page). Added in 0.0.20
//! with the conflict dialog's tabs (M5.4, user request); a file without the section reads the defaults.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogSettings {
    /// "The file exists" / "Queued jobs write the same file" when a job is saved.
    pub conflict: ConflictDialogSettings,
}

/// The conflict dialog of saving a job.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConflictDialogSettings {
    /// The way out chosen when the dialog opens (when it can be taken).
    #[serde_as(as = "DefaultOnError")]
    pub preselect: ConflictPreselect,
    /// List every file of the same numbered series ("Name - 01.mkv", "Name - 02.mp4", ...).
    pub show_series: bool,
    /// List the other files whose names start with the same name.
    pub show_similar: bool,
    /// Which files get a picture.
    #[serde_as(as = "DefaultOnError")]
    pub pictures: PictureScope,
    /// Where in a video the picture is taken, in percent of its length.
    pub picture_at_percent: u8,
    /// Pictures made at the same time.
    pub parallel_pictures: u8,
    /// The size of the pictures in the list of similar files.
    #[serde_as(as = "DefaultOnError")]
    pub tile_size: TileSize,
}

impl ConflictDialogSettings {
    pub const PICTURE_AT_PERCENT: std::ops::RangeInclusive<u8> = 0..=90;
    pub const PARALLEL_PICTURES: std::ops::RangeInclusive<u8> = 1..=8;

    /// Values out of range brought back into it; the notes say what changed.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let at = self
            .picture_at_percent
            .clamp(*Self::PICTURE_AT_PERCENT.start(), *Self::PICTURE_AT_PERCENT.end());
        if at != self.picture_at_percent {
            notes.push("dialogs.conflict.picture_at_percent brought into range".to_owned());
            self.picture_at_percent = at;
        }
        let parallel = self
            .parallel_pictures
            .clamp(*Self::PARALLEL_PICTURES.start(), *Self::PARALLEL_PICTURES.end());
        if parallel != self.parallel_pictures {
            notes.push("dialogs.conflict.parallel_pictures brought into range".to_owned());
            self.parallel_pictures = parallel;
        }
        notes
    }
}

impl Default for ConflictDialogSettings {
    fn default() -> Self {
        Self {
            preselect: ConflictPreselect::default(),
            show_series: true,
            show_similar: true,
            pictures: PictureScope::default(),
            picture_at_percent: 10,
            parallel_pictures: 3,
            tile_size: TileSize::default(),
        }
    }
}

/// The way out the conflict dialog chooses when it opens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPreselect {
    /// The next free number of the counter (`"(n)"` when the name has no counter).
    #[default]
    NextNumber,
    /// `Name (n)`.
    Variant,
    /// Replace the existing file (only when it exists).
    Overwrite,
}

/// Which files of the conflict dialog get a picture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureScope {
    /// The existing file and every listed file.
    #[default]
    All,
    /// Only the existing file.
    Existing,
    /// None (the dialog shows names and facts only).
    Off,
}

/// The size of a picture in the list of similar files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TileSize {
    /// The width of a picture in the list in logical pixels (16:9).
    pub const fn width(self) -> u32 {
        match self {
            Self::Small => 80,
            Self::Medium => 112,
            Self::Large => 160,
        }
    }
}
