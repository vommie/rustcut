//! Tests of `config.toml`'s schema (defaults, tolerant reading of old and broken files, sanitizing).

use super::schema::*;
use crate::render::DEFAULT_PROFILE_ID;
use crate::settings::{LoadOutcome, VersionedFile as _, load, save};

#[test]
fn defaults_survive_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), &Settings::default()).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert_eq!(
        outcome,
        LoadOutcome::Loaded {
            value: Settings::default(),
            notes: vec![]
        }
    );
}

#[test]
fn a_partial_file_keeps_the_other_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "version = 1\n[queue]\nworkers = 3\n").unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(value.queue.workers, 3);
    assert_eq!(value.player, PlayerSettings::default());
    assert_eq!(value.render.default_profile, DEFAULT_PROFILE_ID);
}

#[test]
fn a_file_of_0_0_5_without_a_keymap_still_loads() {
    // What 0.0.5 wrote (no `[keymap]`, the old seek steps): every value is kept, the keymap uses its defaults.
    let dir = tempfile::tempdir().unwrap();
    let old = "version = 1\n[editor]\nauto_section = \"off\"\nseek_step_secs = 5\nseek_step_long_secs = 60\n\
               [player]\nauto_play = false\n";
    std::fs::write(dir.path().join("config.toml"), old).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert!(
        matches!(outcome, LoadOutcome::Loaded { ref notes, .. } if notes.is_empty()),
        "{outcome:?}"
    );
    let value = outcome.into_value();
    assert!(value.keymap.is_empty());
    assert_eq!((value.editor.seek_step_secs, value.editor.seek_step_long_secs), (5, 60));
    assert!(!value.player.auto_play);
    // Nothing to back up: no migration happened.
    assert!(!dir.path().join("b").exists());
}

#[test]
fn the_keymap_table_is_tolerant() {
    let dir = tempfile::tempdir().unwrap();
    let text = "version = 1\n[queue]\nworkers = 2\n[keymap]\nset_start = [\"I\", \"Home\"]\nadd_section = \"Plus\"\n\
                toggle_mute = []\nreload = 5\nseek_back = [\"LeftArrow\", 3]\n";
    std::fs::write(dir.path().join("config.toml"), text).unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(
        value.queue.workers, 2,
        "a bad keymap value must not reset the other settings"
    );
    let get = |k: &str| value.keymap.get(k).cloned();
    assert_eq!(get("set_start"), Some(Some(vec!["I".to_owned(), "Home".to_owned()])));
    assert_eq!(get("add_section"), Some(Some(vec!["Plus".to_owned()])));
    assert_eq!(get("toggle_mute"), Some(Some(vec![])));
    assert_eq!(get("reload"), Some(None));
    assert_eq!(get("seek_back"), Some(None));

    // A keymap that is not a table at all reads as empty.
    std::fs::write(dir.path().join("config.toml"), "version = 1\nkeymap = 7\n").unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert!(value.keymap.is_empty());
}

#[test]
fn the_keymap_round_trips_as_lists() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.keymap.insert("add_section".into(), Some(vec!["Plus".into()]));
    settings.keymap.insert("toggle_mute".into(), Some(vec![]));
    save(dir.path(), &settings).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("add_section = [\"Plus\"]"), "{text}");
    assert_eq!(
        load::<Settings>(dir.path(), &dir.path().join("b"))
            .unwrap()
            .into_value(),
        settings
    );
    // Defaults write no keymap table.
    save(dir.path(), &Settings::default()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(!text.contains("keymap"), "{text}");
}

#[test]
fn user_profiles_round_trip_and_a_broken_one_is_skipped() {
    use crate::render::{RateControl, VideoEncoder, builtin_profiles};
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    let mut mine = builtin_profiles().remove(0);
    mine.id = "mine".into();
    mine.name = "Mine".into();
    mine.set_encoder(VideoEncoder::X265);
    mine.video.rate_control = RateControl::ConstantQp { qp: 20 };
    mine.video.scene_cuts = Some(false);
    settings.render.profiles = vec![mine.clone()];
    settings.render.default_profile = "mine".into();
    save(dir.path(), &settings).unwrap();
    let read = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(read, settings);

    // A profile of 0.0.8 (no expert fields) and one with an encoder this version does not know.
    let text = "version = 1\n[queue]\nworkers = 2\n[render]\ndefault_profile = \"old\"\n\
                [[render.profiles]]\nid = \"future\"\nname = \"Future\"\n[render.profiles.video]\n\
                encoder = \"vvc_next\"\nrate_control = { mode = \"quality\", value = 30 }\n\
                [[render.profiles]]\nid = \"old\"\nname = \"Old\"\ncontainer = \"mp4\"\n\
                [render.profiles.video]\nencoder = \"libx264_typo\"\nrate_control = { mode = \"quality\", value = 20 }\n\
                [[render.profiles]]\nid = \"kept\"\nname = \"Kept\"\n[render.profiles.video]\nencoder = \"x264\"\n\
                rate_control = { mode = \"quality\", value = 21 }\npreset = \"slow\"\n\
                codec_params = [{ key = \"ref\", value = \"4\" }]\n[render.profiles.audio]\ncodec = \"aac\"\n";
    std::fs::write(dir.path().join("config.toml"), text).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    let read = outcome.into_value();
    assert_eq!(read.queue.workers, 2, "the other settings stay");
    let ids: Vec<&str> = read.render.profiles.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["kept"]);
    let kept = &read.render.profiles[0];
    assert_eq!(kept.video.preset.as_deref(), Some("slow"));
    assert_eq!((kept.video.aq_mode.as_deref(), kept.video.scene_cuts), (None, None));
    assert_eq!(read.render.default_profile, DEFAULT_PROFILE_ID, "\"old\" is gone");
    assert!(!dir.path().join("b").exists(), "no migration, no backup");
}

#[test]
fn sanitizing_repairs_bad_values() {
    let mut s = Settings::default();
    s.queue.workers = 0;
    s.render.nice = 40;
    s.render.default_profile = "gone".into();
    s.export.name_template = " ".into();
    s.render.profiles = vec![crate::render::builtin_profiles().remove(0)];
    s.advanced.log_keep_days = 0;
    s.editor.seek_step_secs = 0;
    let notes = s.sanitize();
    assert_eq!(notes.len(), 7, "{notes:?}");
    assert_eq!(s.editor.seek_step_secs, 1);
    assert_eq!(s.advanced.log_keep_days, 1);
    assert_eq!((s.queue.workers, s.render.nice), (1, 19));
    assert_eq!(s.render.default_profile, DEFAULT_PROFILE_ID);
    assert!(s.render.profiles.is_empty());
    assert!(Settings::default().sanitize().is_empty());
}

#[test]
fn hold_time_and_draft_on_load_have_defaults_and_are_tolerant() {
    // A file of 0.0.12 has neither: the defaults fill in (2 s, replace), nothing is migrated.
    let dir = tempfile::tempdir().unwrap();
    let old = "version = 1\n[general]\nsingle_instance = false\n[editor]\nauto_section = \"whole_video\"\n";
    std::fs::write(dir.path().join("config.toml"), old).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert!(
        matches!(outcome, LoadOutcome::Loaded { ref notes, .. } if notes.is_empty()),
        "{outcome:?}"
    );
    let value = outcome.into_value();
    assert_eq!(value.general.hold_to_confirm_ms, 2_000);
    assert!(!value.general.single_instance);
    assert_eq!(value.editor.draft_on_load, DraftOnLoad::Replace);
    assert_eq!(value.editor.auto_section, AutoSection::WholeVideo);
    assert!(!dir.path().join("b").exists());

    // A value of a later version reads as the default; the rest of the section stays.
    let text = "version = 1\n[editor]\nseek_step_secs = 4\ndraft_on_load = \"teleport\"\n";
    std::fs::write(dir.path().join("config.toml"), text).unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(value.editor.draft_on_load, DraftOnLoad::Replace);
    assert_eq!(value.editor.seek_step_secs, 4);

    let mut settings = Settings::default();
    settings.editor.draft_on_load = DraftOnLoad::SetAside;
    settings.general.hold_to_confirm_ms = 0;
    save(dir.path(), &settings).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("draft_on_load = \"set_aside\""), "{text}");
    let read = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(read, settings, "0 (a click is enough) is kept");

    for (stored, kept) in [(50, 300), (65_000, 10_000), (1_500, 1_500)] {
        let mut s = Settings::default();
        s.general.hold_to_confirm_ms = stored;
        s.sanitize();
        assert_eq!(s.general.hold_to_confirm_ms, kept);
    }
}

#[test]
fn appearance_and_tag_history_have_defaults_and_are_sanitized() {
    // A file of 0.0.14 has neither `[appearance]` nor `rustview.tag_history_size`: the defaults fill in, no migration.
    let dir = tempfile::tempdir().unwrap();
    let old = "version = 1\n[rustview]\ntimeout_secs = 7\n";
    std::fs::write(dir.path().join("config.toml"), old).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert!(
        matches!(outcome, LoadOutcome::Loaded { ref notes, .. } if notes.is_empty()),
        "{outcome:?}"
    );
    let value = outcome.into_value();
    assert_eq!(value.appearance, AppearanceSettings::default());
    assert_eq!((value.rustview.timeout_secs, value.rustview.tag_history_size), (7, 20));
    assert!(!dir.path().join("b").exists());

    let mut s = Settings::default();
    s.appearance.tooltip_delay_ms = 60_000;
    s.rustview.tag_history_size = 0;
    assert_eq!(s.sanitize().len(), 2);
    assert_eq!((s.appearance.tooltip_delay_ms, s.rustview.tag_history_size), (5_000, 1));
}

#[test]
fn dialog_settings_have_defaults_bad_values_read_as_defaults_and_ranges_are_kept() {
    use crate::settings::{ConflictPreselect, DialogSettings, PictureScope, TileSize};
    // A file of 0.0.19 has no `[dialogs]`: the defaults fill in, no migration, no backup.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "version = 1\n[queue]\nworkers = 2\n").unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert!(
        matches!(outcome, LoadOutcome::Loaded { ref notes, .. } if notes.is_empty()),
        "{outcome:?}"
    );
    let value = outcome.into_value();
    assert_eq!(value.dialogs, DialogSettings::default());
    assert_eq!(value.queue.workers, 2);
    assert!(!dir.path().join("b").exists());
    let c = &value.dialogs.conflict;
    assert_eq!(
        (
            c.preselect,
            c.show_series,
            c.show_similar,
            c.pictures,
            c.picture_at_percent,
            c.parallel_pictures
        ),
        (ConflictPreselect::NextNumber, true, true, PictureScope::All, 10, 3)
    );

    // An unknown choice reads as its default and never resets the rest; numbers out of range are brought back.
    let text = "version = 1\n[dialogs.conflict]\npreselect = \"later\"\npictures = \"existing\"\n\
                tile_size = \"huge\"\nshow_series = false\npicture_at_percent = 99\nparallel_pictures = 0\n";
    let mut s: Settings = toml::from_str(text).unwrap();
    let c = &s.dialogs.conflict;
    assert_eq!(
        (c.preselect, c.pictures, c.tile_size, c.show_series),
        (
            ConflictPreselect::NextNumber,
            PictureScope::Existing,
            TileSize::Medium,
            false
        )
    );
    assert_eq!(s.sanitize().len(), 2);
    assert_eq!(
        (
            s.dialogs.conflict.picture_at_percent,
            s.dialogs.conflict.parallel_pictures
        ),
        (90, 1)
    );
    let written = toml::to_string(&s).unwrap();
    assert!(written.contains("[dialogs.conflict]"), "{written}");
    assert_eq!(toml::from_str::<Settings>(&written).unwrap(), s, "round trip");
}
