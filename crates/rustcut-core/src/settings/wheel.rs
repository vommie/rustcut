//! `[folder_wheel]` in `config.toml` (P11.3, user decision Q-FP2): how the folder wheel opens, moves and looks, and
//! how both pickers show pictures and colors. Every value has a default, so older files load unchanged.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

/// The mouse trigger: holding a button over the video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelButton {
    #[default]
    Middle,
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelSize {
    /// 40 % of the main window's shorter side, never below a comfortable size.
    #[default]
    Automatic,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelLabels {
    /// The picture and the name.
    #[default]
    PictureAndName,
    Picture,
    Name,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureShape {
    Round,
    #[default]
    Rounded,
    Square,
}

/// Where an entry's color shows (Q-FP2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// The wheel's fields are tinted (stronger when pointed), the list has a stripe, icons take the color; pictures stay.
    #[default]
    BackgroundAndIcons,
    /// Only the background; icons in the text color.
    Background,
    /// Only the pointed field.
    Pointed,
}

#[serde_as]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderWheelSettings {
    #[serde_as(as = "DefaultOnError")]
    pub button: WheelButton,
    /// How long the button is held before the wheel opens (ms).
    pub hold_ms: u16,
    /// How long the pointer rests on a group (and in the middle) before it opens (goes up), ms.
    pub dwell_ms: u16,
    /// Pushing past the outer edge opens a group at once.
    pub push: bool,
    /// Places per level (4-12).
    pub places: u8,
    /// As many places as a level has entries (up to `places`).
    pub divided: bool,
    #[serde_as(as = "DefaultOnError")]
    pub size: WheelSize,
    #[serde_as(as = "DefaultOnError")]
    pub labels: WheelLabels,
    /// The path of opened groups above the wheel.
    pub breadcrumb: bool,
    /// The video is dimmed behind the wheel.
    pub dim: bool,
    pub animations: bool,
    /// The pictures' shape in the wheel and in the list.
    #[serde_as(as = "DefaultOnError")]
    pub wheel_shape: PictureShape,
    #[serde_as(as = "DefaultOnError")]
    pub list_shape: PictureShape,
    #[serde_as(as = "DefaultOnError")]
    pub color_mode: ColorMode,
}

impl FolderWheelSettings {
    pub const PLACES: std::ops::RangeInclusive<u8> = 4..=12;
    pub const HOLD_MS: std::ops::RangeInclusive<u16> = 50..=2_000;
    pub const DWELL_MS: std::ops::RangeInclusive<u16> = 100..=3_000;
}

impl Default for FolderWheelSettings {
    fn default() -> Self {
        Self {
            button: WheelButton::Middle,
            hold_ms: 250,
            dwell_ms: 600,
            push: true,
            places: 8,
            divided: false,
            size: WheelSize::Automatic,
            labels: WheelLabels::PictureAndName,
            breadcrumb: false,
            dim: true,
            animations: true,
            wheel_shape: PictureShape::Round,
            list_shape: PictureShape::Rounded,
            color_mode: ColorMode::BackgroundAndIcons,
        }
    }
}

impl FolderWheelSettings {
    /// Brings the numbers into range; a note per change.
    pub(super) fn sanitize(&mut self, notes: &mut Vec<String>) {
        let fresh = Self::default();
        if !Self::PLACES.contains(&self.places) {
            notes.push(format!("folder_wheel.places {} brought into range", self.places));
            self.places = self.places.clamp(*Self::PLACES.start(), *Self::PLACES.end());
        }
        if !Self::HOLD_MS.contains(&self.hold_ms) {
            notes.push(format!("folder_wheel.hold_ms {} reset", self.hold_ms));
            self.hold_ms = fresh.hold_ms;
        }
        if !Self::DWELL_MS.contains(&self.dwell_ms) {
            notes.push(format!("folder_wheel.dwell_ms {} reset", self.dwell_ms));
            self.dwell_ms = fresh.dwell_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn a_file_without_the_wheel_has_its_defaults_and_odd_values_are_repaired() {
        // 0.0.77 wrote no `[folder_wheel]`.
        let old: Settings = toml::from_str("version = 8\n[queue]\nworkers = 2\n").unwrap();
        assert_eq!(old.folder_wheel, FolderWheelSettings::default());
        let mut odd: Settings = toml::from_str(
            "version = 8\n[folder_wheel]\nplaces = 30\nhold_ms = 0\ndwell_ms = 700\nsize = \"huge\"\npush = false\n",
        )
        .unwrap();
        assert_eq!(odd.folder_wheel.size, WheelSize::Automatic, "unknown = the default");
        let mut notes = Vec::new();
        odd.folder_wheel.sanitize(&mut notes);
        assert_eq!(notes.len(), 2);
        let w = odd.folder_wheel;
        assert_eq!((w.places, w.hold_ms, w.dwell_ms, w.push), (12, 250, 700, false));
    }
}
