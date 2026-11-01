//! The `[player]` table of `config.toml`: playback, the preview's audio and what shows over the video.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

/// The limits of `PlayerSettings::hide_pointer_ms`.
pub const MIN_HIDE_POINTER_MS: u32 = 500;
pub const MAX_HIDE_POINTER_MS: u32 = 60_000;

/// How the preview evens out its loudness (P12 5.3, user decision Q-P12d); the render is never affected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    #[default]
    None,
    /// EBU R128 (`loudnorm`, the switch "Even out the loudness" before settings version 10).
    Loudness,
    /// `dynaudnorm`: slow and gentle.
    Dynamic,
    /// `speechnorm`: lifts every syllable.
    Speech,
    /// `acompressor`: only pushes down the loud parts.
    Compressor,
}

impl Normalization {
    /// The kinds that do something, in the menu's order.
    pub const ACTIVE: [Self; 4] = [Self::Loudness, Self::Dynamic, Self::Speech, Self::Compressor];

    /// The name in the menu, the tooltip and the symbol over the video.
    pub const fn title(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Loudness => "Loudness",
            Self::Dynamic => "Dynamic",
            Self::Speech => "Speech",
            Self::Compressor => "Compressor",
        }
    }

    /// What the kind does, for its tooltip.
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::None => "The sound as it is in the file.",
            Self::Loudness => "EBU R128: everything about equally loud. The strongest kind.",
            Self::Dynamic => "Evens out slowly and gently, stays natural. Good for films and mixed sound.",
            Self::Speech => "Made for speech: lifts every syllable. Restless on music.",
            Self::Compressor => "Night mode: only pushes down the loud parts, at once.",
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// Start playing when a video opens.
    pub auto_play: bool,
    /// mpv `hwdec` (`auto-copy`, `no`, `vaapi`, ...).
    pub hwdec: String,
    /// Background around the video (`#RRGGBB`).
    pub background: String,
    /// Volume change per mouse wheel step on the video, in percent.
    pub wheel_volume_step: u8,
    /// Mute the last second of a video while previewing (against crackling at the end).
    pub mute_last_second: bool,
    /// How the preview evens out its loudness (settings version 10; before: the switch `loudnorm_preview`). A value this
    /// version does not know reads as the default.
    #[serde_as(as = "DefaultOnError")]
    pub normalization: Normalization,
    /// The kind used before the current one; a right click brings it back from "None" ([`PlayerSettings::return_to`]).
    #[serde_as(as = "DefaultOnError")]
    pub last_normalization: Normalization,
    /// A key that changes the normalization shows its name in the middle of the video.
    pub flash_normalization: bool,
    /// A play or pause symbol flashes over the video when a key plays or pauses it (P9.9, user decision Q-P9.9).
    pub flash_play_pause: bool,
    /// The pause symbol stays (faint) over the video while it is paused (P9.9).
    pub keep_pause_symbol: bool,
    /// The mute key shows a speaker in the middle of the video (0.0.72; older files have none: on).
    pub flash_mute: bool,
    /// Ctrl+0..5 show the rating's digit in the middle of the video (0.0.72; on).
    pub flash_rating: bool,
    /// The mouse wheel shows the volume bar near the bottom of the video (0.0.72; on).
    pub volume_bar: bool,
    /// The pointer hides when it rests over the video (P12 5.2).
    pub hide_pointer: bool,
    /// How long it has to rest, in milliseconds.
    pub hide_pointer_ms: u32,
    /// The program of "Open in mpv"; `None` = `mpv` from `PATH`.
    pub mpv: Option<PathBuf>,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            auto_play: true,
            hwdec: "auto-copy".into(),
            background: "#444444".into(),
            wheel_volume_step: 5,
            mute_last_second: false,
            normalization: Normalization::None,
            last_normalization: Normalization::None,
            flash_normalization: true,
            flash_play_pause: true,
            keep_pause_symbol: true,
            flash_mute: true,
            flash_rating: true,
            volume_bar: true,
            hide_pointer: true,
            hide_pointer_ms: 3_000,
            mpv: None,
        }
    }
}

impl PlayerSettings {
    /// The kind a right click on the button brings back: the one used last, else the first of the menu.
    pub fn return_to(&self) -> Normalization {
        if self.last_normalization == Normalization::None {
            Normalization::Loudness
        } else {
            self.last_normalization
        }
    }

    /// Chooses `kind` and remembers the one it replaces, so a right click finds its way back.
    pub fn choose_normalization(&mut self, kind: Normalization) {
        if self.normalization != Normalization::None {
            self.last_normalization = self.normalization;
        }
        self.normalization = kind;
    }

    /// The right click: "None", or back to the kind used last.
    pub fn toggle_normalization(&mut self) {
        let next = if self.normalization == Normalization::None {
            self.return_to()
        } else {
            Normalization::None
        };
        self.choose_normalization(next);
    }

    /// The key that goes through every kind: None, then the menu's order, then None again.
    pub fn next_normalization(&mut self) {
        let position = Normalization::ACTIVE.iter().position(|k| *k == self.normalization);
        let next = match position {
            Some(i) => Normalization::ACTIVE.get(i + 1).copied().unwrap_or_default(),
            None => Normalization::ACTIVE.first().copied().unwrap_or_default(),
        };
        self.choose_normalization(next);
    }
}

/// Settings version 9 -> 10 (P12 5.3, user decision Q-P12d): the switch `player.loudnorm_preview` became the choice
/// `player.normalization`; on means "loudness", the kind that switch always was.
pub(super) fn migrate_normalization(table: &mut toml::Table) {
    let Some(player) = table.get_mut("player").and_then(toml::Value::as_table_mut) else {
        return;
    };
    if let Some(old) = player.remove("loudnorm_preview")
        && old.as_bool() == Some(true)
    {
        player.insert("normalization".into(), toml::Value::String("loudness".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::{VersionedFile as _, load};
    use super::super::schema::Settings;
    use super::*;

    fn migrated(old: &str) -> (Settings, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(Settings::FILE_NAME), old).unwrap();
        let backups = dir.path().join("b");
        let settings = load::<Settings>(dir.path(), &backups).unwrap().value().clone();
        (settings, dir)
    }

    #[test]
    fn version_9_switch_on_becomes_loudness_with_a_backup() {
        let old = "version = 9\n\n[player]\nloudnorm_preview = true\nmute_last_second = true\n";
        let (settings, dir) = migrated(old);
        assert_eq!(settings.player.normalization, Normalization::Loudness);
        assert!(settings.player.mute_last_second, "the other values stay");
        assert!(
            std::fs::read_dir(dir.path().join("b"))
                .unwrap()
                .flatten()
                .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t == old)),
            "the old file is backed up first"
        );
    }

    #[test]
    fn version_9_switch_off_or_missing_becomes_none() {
        for old in [
            "version = 9\n\n[player]\nloudnorm_preview = false\n",
            "version = 9\n\n[player]\nauto_play = false\n",
            "version = 9\n",
        ] {
            assert_eq!(migrated(old).0.player.normalization, Normalization::None, "{old}");
        }
    }

    #[test]
    fn a_newer_kind_reads_as_none() {
        let old = format!(
            "version = {}\n\n[player]\nnormalization = \"future\"\n",
            Settings::VERSION
        );
        assert_eq!(migrated(&old).0.player.normalization, Normalization::None);
    }

    #[test]
    fn the_new_options_have_defaults_in_old_files() {
        let (settings, _dir) = migrated("version = 9\n\n[player]\nauto_play = false\n");
        assert!(settings.player.hide_pointer);
        assert_eq!(settings.player.hide_pointer_ms, 3_000);
        assert!(settings.player.flash_normalization);
        assert_eq!(settings.player.mpv, None);
    }

    #[test]
    fn a_right_click_turns_off_and_comes_back() {
        let mut p = PlayerSettings::default();
        p.choose_normalization(Normalization::Dynamic);
        p.toggle_normalization();
        assert_eq!(p.normalization, Normalization::None);
        p.toggle_normalization();
        assert_eq!(p.normalization, Normalization::Dynamic);
    }

    #[test]
    fn a_right_click_from_a_fresh_none_takes_the_first_kind() {
        let mut p = PlayerSettings::default();
        p.toggle_normalization();
        assert_eq!(p.normalization, Normalization::Loudness);
    }

    #[test]
    fn the_key_goes_through_every_kind_and_back_to_none() {
        let mut p = PlayerSettings::default();
        let mut seen = Vec::new();
        for _ in 0..5 {
            p.next_normalization();
            seen.push(p.normalization);
        }
        assert_eq!(
            seen,
            [
                Normalization::Loudness,
                Normalization::Dynamic,
                Normalization::Speech,
                Normalization::Compressor,
                Normalization::None
            ]
        );
    }

    #[test]
    fn the_rest_time_is_kept_in_range() {
        let mut s = Settings::default();
        s.player.hide_pointer_ms = 10;
        assert!(!s.sanitize().is_empty());
        assert_eq!(s.player.hide_pointer_ms, MIN_HIDE_POINTER_MS);
        s.player.hide_pointer_ms = 90_000;
        assert!(!s.sanitize().is_empty());
        assert_eq!(s.player.hide_pointer_ms, MAX_HIDE_POINTER_MS);
    }
}
