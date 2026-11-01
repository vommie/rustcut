//! Settings of the timeline thumbnails (P12 block 6, user decision Q-P12e). Nothing here existed before, so older files read
//! as these defaults without a migration.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

/// Largest extra delay before a thumbnail shows.
pub const MAX_THUMB_DELAY_MS: u32 = 2000;
/// How far the portrait size can be adjusted, in percent.
pub const MIN_THUMB_PORTRAIT_PERCENT: i16 = -50;
pub const MAX_THUMB_PORTRAIT_PERCENT: i16 = 100;
pub const MAX_THUMB_WORKERS: u8 = 16;

/// Where the time goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbLook {
    #[default]
    TimeBelow,
    TimeInside,
    PictureOnly,
}

/// How a portrait video's picture compares to a wide one's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbPortrait {
    #[default]
    SameHeight,
    SameArea,
    SameWidth,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbPlace {
    #[default]
    FollowPointer,
    Center,
}

/// Which part of the timeline shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbZone {
    #[default]
    RulerAndTrack,
    Ruler,
    Track,
}

/// What shows while no picture is near the place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbMissing {
    #[default]
    NearestWithDot,
    Nearest,
    EmptyField,
    Nothing,
}

/// How the generation shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbProgress {
    #[default]
    Strip,
    StatusBar,
    Nothing,
}

/// The factor of the density rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbDensity {
    Coarse,
    #[default]
    Normal,
    Fine,
}

impl ThumbDensity {
    pub fn factor(self) -> u32 {
        match self {
            Self::Coarse => 10,
            Self::Normal => 15,
            Self::Fine => 25,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThumbnailSettings {
    /// Off: no picture and nothing is generated.
    pub enabled: bool,
    #[serde_as(as = "DefaultOnError")]
    pub look: ThumbLook,
    /// The picture's width in pixels; 0 = fits itself (240, at most 60 % of the column).
    pub width: u16,
    #[serde_as(as = "DefaultOnError")]
    pub portrait: ThumbPortrait,
    pub portrait_percent: i16,
    #[serde_as(as = "DefaultOnError")]
    pub place: ThumbPlace,
    #[serde_as(as = "DefaultOnError")]
    pub zone: ThumbZone,
    pub delay_ms: u32,
    pub while_dragging: bool,
    #[serde_as(as = "DefaultOnError")]
    pub missing: ThumbMissing,
    #[serde_as(as = "DefaultOnError")]
    pub progress: ThumbProgress,
    /// Crop and rotation show as in the big picture.
    pub show_filters: bool,
    /// 0 = automatic.
    pub workers: u8,
    /// Niceness while no render runs, 0 to 19.
    pub nice: u8,
    #[serde_as(as = "DefaultOnError")]
    pub density: ThumbDensity,
    /// WebP quality, 0 to 100.
    pub quality: u8,
}

impl Default for ThumbnailSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            look: ThumbLook::default(),
            width: 0,
            portrait: ThumbPortrait::default(),
            portrait_percent: 0,
            place: ThumbPlace::default(),
            zone: ThumbZone::default(),
            delay_ms: 100,
            while_dragging: false,
            missing: ThumbMissing::default(),
            progress: ThumbProgress::default(),
            show_filters: true,
            workers: 0,
            nice: 19,
            density: ThumbDensity::default(),
            quality: 50,
        }
    }
}

impl ThumbnailSettings {
    /// Brings hand-edited values into range; true when something changed.
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();
        self.delay_ms = self.delay_ms.min(MAX_THUMB_DELAY_MS);
        self.portrait_percent = self.portrait_percent.clamp(MIN_THUMB_PORTRAIT_PERCENT, MAX_THUMB_PORTRAIT_PERCENT);
        self.width = if self.width == 0 { 0 } else { self.width.clamp(80, 640) };
        self.workers = self.workers.min(MAX_THUMB_WORKERS);
        self.nice = self.nice.min(19);
        self.quality = self.quality.min(100);
        before != *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_the_section_reads_as_the_defaults() {
        let settings: ThumbnailSettings = toml::from_str("").unwrap();
        assert_eq!(settings, ThumbnailSettings::default());
        assert!(settings.enabled);
        assert_eq!((settings.quality, settings.nice, settings.delay_ms), (50, 19, 100));
    }

    #[test]
    fn an_unknown_choice_reads_as_the_default() {
        let settings: ThumbnailSettings = toml::from_str("look = \"sideways\"\nquality = 70").unwrap();
        assert_eq!(settings.look, ThumbLook::TimeBelow);
        assert_eq!(settings.quality, 70);
    }

    #[test]
    fn hand_edited_values_come_back_into_range() {
        let mut settings = ThumbnailSettings {
            nice: 40,
            quality: 200,
            delay_ms: 99_000,
            portrait_percent: 900,
            width: 5,
            ..ThumbnailSettings::default()
        };
        assert!(settings.sanitize());
        assert_eq!((settings.nice, settings.quality, settings.delay_ms), (19, 100, 2000));
        assert_eq!((settings.portrait_percent, settings.width), (100, 80));
        assert!(!settings.sanitize());
    }
}
