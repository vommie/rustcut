//! Dark or light (questionnaire 2026-11-06-rustview-light-theme, shared with RustView): Settings > Appearance > Theme
//! follows the system (KDE's color scheme through the desktop portal), or is always dark or always light. Without an
//! answer from the system the app stays dark. RustView has the same rules (`rustview_core::settings::theme`).

use serde::{Deserialize, Serialize};

/// `appearance.theme` in `config.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    /// Follow the desktop's color scheme.
    #[default]
    System,
    Dark,
    Light,
}

/// What the desktop prefers (the portal's `org.freedesktop.appearance` `color-scheme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemScheme {
    /// No answer yet, no portal, or "no preference".
    #[default]
    Unknown,
    Dark,
    Light,
}

/// Whether the windows are dark.
pub fn resolve_dark(mode: ThemeMode, system: SystemScheme) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system != SystemScheme::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_mode_ignores_the_system() {
        for system in [SystemScheme::Unknown, SystemScheme::Dark, SystemScheme::Light] {
            assert!(resolve_dark(ThemeMode::Dark, system));
            assert!(!resolve_dark(ThemeMode::Light, system));
        }
    }

    #[test]
    fn following_the_system_stays_dark_without_an_answer() {
        assert!(resolve_dark(ThemeMode::System, SystemScheme::Unknown));
        assert!(resolve_dark(ThemeMode::System, SystemScheme::Dark));
        assert!(!resolve_dark(ThemeMode::System, SystemScheme::Light));
    }

    #[test]
    fn the_stored_names_are_snake_case() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            theme: ThemeMode,
        }
        let text = toml::to_string(&Wrap {
            theme: ThemeMode::Light,
        })
        .expect("toml");
        assert_eq!(text.trim(), "theme = \"light\"");
        let back: Wrap = toml::from_str("theme = \"system\"").expect("toml");
        assert_eq!(back.theme, ThemeMode::System);
    }
}
