//! Every generic option of the Settings window, card by card: the pages' contents as data (the window draws them, the
//! tests walk them). The pages with their own editors add those below their cards (`Page::special`).

mod app;
mod dialogs;
mod editing;
mod work;

pub use self::editing::template_example;
use super::field::Field;
use super::page::Page;

/// A group of options with a title, like RustView's `SettingsCard`.
#[derive(Debug, Clone, Copy)]
pub struct Card {
    pub title: &'static str,
    pub description: &'static str,
    pub fields: &'static [Field],
}

/// The cards of `page`.
pub fn cards(page: Page) -> &'static [Card] {
    match page {
        Page::General => app::GENERAL,
        Page::Player => editing::PLAYER,
        Page::Editor => editing::EDITOR,
        Page::Filters => editing::FILTERS,
        Page::Export => editing::EXPORT,
        Page::Profiles | Page::Hotkeys | Page::About => &[],
        Page::Queue => work::QUEUE,
        Page::RustView => work::RUSTVIEW,
        Page::Warnings => app::WARNINGS,
        Page::Dialogs => dialogs::DIALOGS,
        Page::Desktop => app::DESKTOP,
        Page::Appearance => app::APPEARANCE,
        Page::Advanced => app::ADVANCED,
    }
}

/// The field with `key` on any page.
pub fn field(key: &str) -> Option<&'static Field> {
    Page::ALL
        .iter()
        .flat_map(|page| cards(*page))
        .flat_map(|card| card.fields)
        .find(|f| f.key == key)
}

#[cfg(test)]
mod tests {
    use rustcut_core::settings::{Settings, VersionedFile as _};

    use super::*;
    use crate::ui::settings::field::{Edit, Kind, Value};

    fn all() -> impl Iterator<Item = &'static Field> {
        Page::ALL.iter().flat_map(|p| cards(*p)).flat_map(|c| c.fields)
    }

    #[test]
    fn keys_are_unique_and_every_option_reads_its_default() {
        let mut keys = std::collections::HashSet::new();
        for f in all() {
            assert!(keys.insert(f.key), "{} twice", f.key);
            assert!(!f.label.is_empty() && !f.help.is_empty(), "{}", f.key);
            let value = (f.get)(&Settings::default());
            match f.kind {
                Kind::Switch => assert!(matches!(value, Value::Bool(_)), "{}", f.key),
                Kind::Choice(items) => {
                    assert!(matches!(value, Value::Index(i) if i < items.len()), "{}", f.key);
                }
                Kind::Number { min, max, .. } => {
                    assert!(
                        matches!(value, Value::Number(n) if (min..=max).contains(&n)),
                        "{}",
                        f.key
                    );
                }
                Kind::Text { .. } | Kind::Path { .. } => assert!(matches!(value, Value::Text(_)), "{}", f.key),
                Kind::Button(_) => assert_eq!(value, Value::None, "{}", f.key),
            }
        }
        assert!(keys.len() > 50, "{}", keys.len());
    }

    #[test]
    fn every_option_changes_its_setting_and_resets_to_the_default() {
        // The switches that sub-options wait for, on (most are on by default).
        let mut masters = Settings::default();
        masters.desktop.tray.enabled = true;
        for f in all() {
            let base = if (f.enabled)(&Settings::default()) {
                Settings::default()
            } else {
                masters.clone()
            };
            let mut s = base.clone();
            let edit = match f.kind {
                Kind::Switch => Edit::Toggle,
                Kind::Choice(items) => {
                    let Value::Index(i) = (f.get)(&s) else { panic!() };
                    Edit::Choose((i + 1) % items.len())
                }
                Kind::Number { min, max, .. } => {
                    let Value::Number(n) = (f.get)(&s) else { panic!() };
                    Edit::Type(if n == max { min } else { max }.to_string())
                }
                Kind::Path { .. } => Edit::Type("/usr/bin/true".into()),
                Kind::Text { .. } | Kind::Button(_) => continue,
            };
            let before = (f.get)(&s);
            assert_eq!(f.apply(&mut s, edit), Ok(true), "{}", f.key);
            assert_ne!((f.get)(&s), before, "{}", f.key);
            assert!(
                s.clone().sanitize().is_empty(),
                "{} leaves a value sanitizing repairs",
                f.key
            );
            assert!(f.reset(&mut s), "{}", f.key);
            assert_eq!(s, base, "{}", f.key);
        }
    }

    #[test]
    fn numbers_take_their_unit_and_steps_stay_in_range() {
        let workers = field("queue.workers").unwrap();
        let mut s = Settings::default();
        assert_eq!(workers.apply(&mut s, Edit::Type("3".into())), Ok(true));
        assert!(workers.apply(&mut s, Edit::Type("99".into())).is_err());
        assert!(workers.apply(&mut s, Edit::Type("two".into())).is_err());
        assert_eq!(workers.apply(&mut s, Edit::Step(100)), Ok(true));
        assert_eq!(s.queue.workers, 16);
        let hold = field("general.hold_to_confirm_ms").unwrap();
        assert_eq!(hold.apply(&mut s, Edit::Type("1500 ms".into())), Ok(true));
        assert_eq!(s.general.hold_to_confirm_ms, 1_500);
        assert!(
            hold.apply(&mut s, Edit::Type("100".into())).is_err(),
            "between 0 and the shortest hold"
        );
        assert_eq!(hold.apply(&mut s, Edit::Type("0".into())), Ok(true));
    }

    #[test]
    fn texts_are_checked_before_they_are_kept() {
        let mut s = Settings::default();
        let template = field("export.name_template").unwrap();
        assert!(template.apply(&mut s, Edit::Type("{{ nope }}".into())).is_err());
        assert!(template.apply(&mut s, Edit::Type("{{ name ".into())).is_err());
        assert_eq!(
            template.apply(&mut s, Edit::Type("{{ source }} {{ date }}".into())),
            Ok(true)
        );
        let color = field("player.background").unwrap();
        assert!(color.apply(&mut s, Edit::Type("black".into())).is_err());
        assert_eq!(color.apply(&mut s, Edit::Type("#202030".into())), Ok(true));
        let level = field("advanced.log_level").unwrap();
        assert!(level.apply(&mut s, Edit::Type("rustcut=loud".into())).is_err());
        assert_eq!(level.apply(&mut s, Edit::Type("rustcut=debug,info".into())), Ok(true));
        let ffmpeg = field("advanced.ffmpeg").unwrap();
        assert_eq!(ffmpeg.apply(&mut s, Edit::Type("/opt/ffmpeg".into())), Ok(true));
        assert_eq!(ffmpeg.apply(&mut s, Edit::Type("  ".into())), Ok(true));
        assert_eq!(s.advanced.ffmpeg, None, "empty = from PATH");
    }

    #[test]
    fn sub_options_wait_for_their_switch() {
        let mut s = Settings::default();
        s.desktop.notifications.enabled = false;
        let finished = field("desktop.notifications.job_finished").unwrap();
        assert!(finished.indent);
        assert_eq!(finished.apply(&mut s, Edit::Toggle), Ok(false));
        s.desktop.notifications.enabled = true;
        assert_eq!(finished.apply(&mut s, Edit::Toggle), Ok(true));
    }
}
