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

/// What the middle shows for the pointed entry (0.0.82, the user's wish).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelMiddle {
    /// Its picture (its first letter when it has none, like the lists).
    #[default]
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

/// How an own picture shows in a spoke (P12 3.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpokePictures {
    /// A small circle above the name.
    #[default]
    Circle,
    /// The picture fills the spoke, cut to its shape, the name stands on it.
    Background,
}

/// How the wheel appears (P12 3.9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelOpening {
    Off,
    Fade,
    /// One spoke after the other, clockwise.
    #[default]
    Spokes,
    Fan,
    Spin,
}

/// How one level gives way to the next (P12 3.9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelLevels {
    Off,
    /// Into the level, and out again going back.
    #[default]
    Zoom,
    /// The next level grows out of the chosen spoke.
    Spoke,
    Turn,
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
    /// What the middle shows for the pointed entry (0.0.82; older files: the picture).
    #[serde_as(as = "DefaultOnError")]
    pub middle: WheelMiddle,
    /// How opaque the wheel's fields are, in percent (0.0.82; older files: 92).
    pub opacity: u8,
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
    /// The gap between two spokes, in tenths of a percent of the wheel's diameter, like every length below (P12 3.7).
    pub gap: u16,
    /// The gap around the middle.
    pub middle_gap: u16,
    /// How round the spokes' corners are.
    pub corners: u16,
    /// The middle's radius in percent of the wheel's outer radius.
    pub middle_size: u8,
    #[serde_as(as = "DefaultOnError")]
    pub spoke_pictures: SpokePictures,
    /// How dark a picture without a color is behind its name, in percent.
    pub picture_dim: u8,
    /// How dark the video is behind the wheel, in percent (while `dim` is on).
    pub video_dim: u8,
    /// Every level starts with the pointer in the middle, wherever it really is (P12 3.8).
    pub pointer_center: bool,
    /// How far the pointer moves until it chooses a spoke (with `pointer_center`).
    pub pointer_distance: u16,
    #[serde_as(as = "DefaultOnError")]
    pub opening: WheelOpening,
    #[serde_as(as = "DefaultOnError")]
    pub levels: WheelLevels,
    /// How long the animations take, in percent of their normal time.
    pub animation_speed: u16,
    /// The current folder lights up once at the opening.
    pub pulse: bool,
    /// The middle's picture fades over.
    pub crossfade: bool,
}

impl FolderWheelSettings {
    pub const PLACES: std::ops::RangeInclusive<u8> = 4..=12;
    pub const HOLD_MS: std::ops::RangeInclusive<u16> = 50..=2_000;
    pub const DWELL_MS: std::ops::RangeInclusive<u16> = 100..=3_000;
    pub const OPACITY: std::ops::RangeInclusive<u8> = 20..=100;
    pub const GAP: std::ops::RangeInclusive<u16> = 0..=60;
    pub const MIDDLE_GAP: std::ops::RangeInclusive<u16> = 0..=50;
    pub const CORNERS: std::ops::RangeInclusive<u16> = 0..=50;
    pub const MIDDLE_SIZE: std::ops::RangeInclusive<u8> = 24..=46;
    pub const DIM: std::ops::RangeInclusive<u8> = 0..=80;
    pub const POINTER_DISTANCE: std::ops::RangeInclusive<u16> = 20..=150;
    pub const ANIMATION_SPEED: std::ops::RangeInclusive<u16> = 50..=200;
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
            middle: WheelMiddle::Picture,
            opacity: 92,
            breadcrumb: false,
            dim: true,
            animations: true,
            wheel_shape: PictureShape::Round,
            list_shape: PictureShape::Rounded,
            color_mode: ColorMode::BackgroundAndIcons,
            gap: 10,
            middle_gap: 8,
            corners: 0,
            middle_size: 34,
            spoke_pictures: SpokePictures::Circle,
            picture_dim: 45,
            video_dim: 35,
            pointer_center: true,
            pointer_distance: 60,
            opening: WheelOpening::Spokes,
            levels: WheelLevels::Zoom,
            animation_speed: 100,
            pulse: true,
            crossfade: true,
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
        if !Self::OPACITY.contains(&self.opacity) {
            notes.push(format!("folder_wheel.opacity {} brought into range", self.opacity));
            self.opacity = self.opacity.clamp(*Self::OPACITY.start(), *Self::OPACITY.end());
        }
        clamp_into(&mut self.gap, Self::GAP, "gap", notes);
        clamp_into(&mut self.middle_gap, Self::MIDDLE_GAP, "middle_gap", notes);
        clamp_into(&mut self.corners, Self::CORNERS, "corners", notes);
        clamp_into(&mut self.middle_size, Self::MIDDLE_SIZE, "middle_size", notes);
        clamp_into(&mut self.picture_dim, Self::DIM, "picture_dim", notes);
        clamp_into(&mut self.video_dim, Self::DIM, "video_dim", notes);
        clamp_into(
            &mut self.pointer_distance,
            Self::POINTER_DISTANCE,
            "pointer_distance",
            notes,
        );
        clamp_into(
            &mut self.animation_speed,
            Self::ANIMATION_SPEED,
            "animation_speed",
            notes,
        );
        if !Self::DWELL_MS.contains(&self.dwell_ms) {
            notes.push(format!("folder_wheel.dwell_ms {} reset", self.dwell_ms));
            self.dwell_ms = fresh.dwell_ms;
        }
    }
}

/// Brings a number into its range, with a note.
fn clamp_into<T>(value: &mut T, range: std::ops::RangeInclusive<T>, name: &str, notes: &mut Vec<String>)
where
    T: Copy + PartialOrd + std::fmt::Display + Ord,
{
    if !range.contains(value) {
        notes.push(format!("folder_wheel.{name} {value} brought into range"));
        *value = (*value).clamp(*range.start(), *range.end());
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
        // 0.0.81 wrote no `middle` and no `opacity`.
        let older: Settings = toml::from_str("version = 8\n[folder_wheel]\nplaces = 6\n").unwrap();
        assert_eq!(
            (
                older.folder_wheel.places,
                older.folder_wheel.middle,
                older.folder_wheel.opacity
            ),
            (6, WheelMiddle::Picture, 92)
        );
        let mut odd: Settings = toml::from_str(
            "version = 8\n[folder_wheel]\nplaces = 30\nhold_ms = 0\ndwell_ms = 700\nsize = \"huge\"\npush = false\nopacity = 5\n",
        )
        .unwrap();
        assert_eq!(odd.folder_wheel.size, WheelSize::Automatic, "unknown = the default");
        let mut notes = Vec::new();
        odd.folder_wheel.sanitize(&mut notes);
        assert_eq!(notes.len(), 3);
        let w = odd.folder_wheel;
        assert_eq!(
            (w.places, w.hold_ms, w.dwell_ms, w.push, w.opacity),
            (12, 250, 700, false, 20)
        );
    }
}
