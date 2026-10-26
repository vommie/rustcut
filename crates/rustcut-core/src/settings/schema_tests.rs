//! Tests of `config.toml`'s schema (defaults, tolerant reading of old and broken files, sanitizing).

use super::schema::*;
use crate::render::DEFAULT_PROFILE_ID;
use crate::settings::{LoadOutcome, VersionedFile as _, load, save};

/// A version 1 file (every release before the notice system) loads through the migrations to the current version
/// without notes.
fn clean_v1(outcome: &LoadOutcome<Settings>) -> bool {
    matches!(outcome, LoadOutcome::Migrated { from: 1, notes, .. } if notes.is_empty())
}

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
    assert!(clean_v1(&outcome), "{outcome:?}");
    let value = outcome.into_value();
    assert!(value.keymap.is_empty());
    assert_eq!((value.editor.seek_step_secs, value.editor.seek_step_long_secs), (5, 60));
    assert!(!value.player.auto_play);
    // Version 1 files go through the notice migration of version 2 (backed up first).
    assert!(dir.path().join("b").exists());
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
    assert!(dir.path().join("b").exists(), "version 1 is migrated, backed up first");
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
fn hold_time_and_job_load_have_defaults_and_are_tolerant() {
    // A file of 0.0.12 has neither: the defaults fill in (2 s, replace), nothing is migrated.
    let dir = tempfile::tempdir().unwrap();
    let old = "version = 1\n[general]\nsingle_instance = false\n[editor]\nauto_section = \"whole_video\"\n";
    std::fs::write(dir.path().join("config.toml"), old).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    assert!(clean_v1(&outcome), "{outcome:?}");
    let value = outcome.into_value();
    assert_eq!(value.general.hold_to_confirm_ms, 2_000);
    assert!(!value.general.single_instance);
    assert_eq!(value.editor.on_job_load, OnJobLoad::Replace);
    assert_eq!(value.editor.auto_section, AutoSection::WholeVideo);

    // A value of a later version reads as the default; the rest of the section stays.
    let text = "version = 4\n[editor]\nseek_step_secs = 4\non_job_load = \"teleport\"\n";
    std::fs::write(dir.path().join("config.toml"), text).unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!(value.editor.on_job_load, OnJobLoad::Replace);
    assert_eq!(value.editor.seek_step_secs, 4);

    let mut settings = Settings::default();
    settings.editor.on_job_load = OnJobLoad::Ask;
    settings.general.hold_to_confirm_ms = 0;
    save(dir.path(), &settings).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.contains("on_job_load = \"ask\""), "{text}");
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
    assert!(clean_v1(&outcome), "{outcome:?}");
    let value = outcome.into_value();
    assert_eq!(value.appearance, AppearanceSettings::default());
    assert_eq!((value.rustview.timeout_secs, value.rustview.tag_history_size), (7, 20));

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
    assert!(clean_v1(&outcome), "{outcome:?}");
    let value = outcome.into_value();
    assert_eq!(value.dialogs, DialogSettings::default());
    assert_eq!(value.queue.workers, 2);
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

#[test]
fn the_play_pause_symbols_are_on_by_default_and_old_files_load() {
    // 0.0.46 wrote no symbol switches: both are on (P9.9).
    let s: Settings = toml::from_str("version = 4\n[player]\nauto_play = false\n").unwrap();
    assert!(!s.player.auto_play && s.player.flash_play_pause && s.player.keep_pause_symbol);
    let s: Settings = toml::from_str("version = 4\n[player]\nkeep_pause_symbol = false\n").unwrap();
    assert!(s.player.flash_play_pause && !s.player.keep_pause_symbol);
}

#[test]
fn sidecar_writing_is_on_by_default_and_old_files_load() {
    // 0.0.19 wrote no `[hashing]`.
    let s: Settings = toml::from_str("version = 1\n[queue]\nworkers = 2\n").unwrap();
    assert!(s.hashing.write_sidecar);
    let s: Settings = toml::from_str("version = 1\n[hashing]\nwrite_sidecar = false\n").unwrap();
    assert!(!s.hashing.write_sidecar);
}

#[test]
fn a_file_of_0_0_24_moves_its_notice_options_with_a_backup() {
    use crate::settings::{KindRoute, NoticePlace};
    let dir = tempfile::tempdir().unwrap();
    let old = "version = 1\n[warnings]\nno_tags = false\nknown_source = false\n[desktop]\ntaskbar_progress = false\n\
               [desktop.notifications]\nenabled = true\njob_finished = true\njob_failed = true\nqueue_done = true\n\
               timeout_secs = 12\n";
    std::fs::write(dir.path().join("config.toml"), old).unwrap();
    let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
    let LoadOutcome::Migrated {
        value,
        from: 1,
        backup,
        notes,
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        old,
        "the old file is backed up first"
    );
    assert!(
        value.warnings.missing_metadata && !value.desktop.taskbar_progress,
        "the other values stay (the rating question was still on, so the one warning stays on)"
    );
    assert_eq!(value.notices.desktop.timeout_secs, 12);
    assert_eq!(
        value.notices.kinds.get("job_finished"),
        Some(&KindRoute {
            place: None,
            desktop: Some(true)
        })
    );
    assert_eq!(
        value.notices.kinds.get("known_source"),
        Some(&KindRoute {
            place: Some(NoticePlace::Nothing),
            desktop: None
        })
    );
    assert!(
        !value.notices.kinds.contains_key("job_failed"),
        "a default stays a default"
    );
    save(dir.path(), &value).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(text.starts_with("version = 4"), "{text}");
    assert!(
        !text.contains("[desktop.notifications]") && !text.contains("known_source = false"),
        "{text}"
    );
}

#[test]
fn a_file_of_0_0_28_loads_its_job_without_a_draft() {
    // Settings version 2: "set aside" reads as "ask", the key is renamed, the old file is backed up first.
    let dir = tempfile::tempdir().unwrap();
    for (stored, read) in [
        ("set_aside", OnJobLoad::Ask),
        ("ask", OnJobLoad::Ask),
        ("replace", OnJobLoad::Replace),
        ("teleport", OnJobLoad::Replace),
    ] {
        let old = format!("version = 2\n[editor]\nseek_step_secs = 5\ndraft_on_load = \"{stored}\"\n");
        std::fs::write(dir.path().join("config.toml"), &old).unwrap();
        let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
        let LoadOutcome::Migrated {
            value,
            from: 2,
            backup,
            notes,
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            old,
            "the old file is backed up first"
        );
        assert_eq!(
            (value.editor.on_job_load, value.editor.seek_step_secs),
            (read, 5),
            "{stored}"
        );
        save(dir.path(), &value).unwrap();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.starts_with("version = 4") && !text.contains("draft"), "{text}");
    }
    // Without the key nothing changes but the version.
    std::fs::write(dir.path().join("config.toml"), "version = 2\n[queue]\nworkers = 2\n").unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert_eq!((value.editor.on_job_load, value.queue.workers), (OnJobLoad::Replace, 2));
}

#[test]
fn a_file_of_0_0_32_gets_one_metadata_warning() {
    // Settings version 3: the two questions become one switch, on when either was on; the old file is backed up first.
    let dir = tempfile::tempdir().unwrap();
    for (warnings, wanted) in [
        ("no_tags = false\nno_rating = false\n", false),
        ("no_tags = false\nno_rating = true\n", true),
        ("no_tags = true\nno_rating = false\n", true),
        ("no_tags = false\n", true),
        ("no_rating = false\n", true),
    ] {
        let old = format!("version = 3\n[warnings]\ntarget_exists = false\n{warnings}");
        std::fs::write(dir.path().join("config.toml"), &old).unwrap();
        let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
        let LoadOutcome::Migrated {
            value,
            from: 3,
            backup,
            notes,
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            old,
            "the old file is backed up first"
        );
        assert_eq!(
            (value.warnings.missing_metadata, value.warnings.target_exists),
            (wanted, false),
            "{warnings}"
        );
        save(dir.path(), &value).unwrap();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(
            text.starts_with("version = 4") && !text.contains("no_tags") && !text.contains("no_rating"),
            "{text}"
        );
    }
    // Without the keys nothing changes but the version.
    std::fs::write(dir.path().join("config.toml"), "version = 3\n[queue]\nworkers = 2\n").unwrap();
    let value = load::<Settings>(dir.path(), &dir.path().join("b"))
        .unwrap()
        .into_value();
    assert!(value.warnings.missing_metadata);
    assert_eq!(value.queue.workers, 2);
}

#[test]
fn a_file_of_0_0_54_writes_the_shareable_log_and_replaces_every_kind() {
    let s: Settings = toml::from_str("version = 4\n[advanced]\nlog_level = \"debug\"\n").unwrap();
    assert_eq!(s.advanced.log_level, "debug");
    assert!(s.advanced.shareable_log, "on by default (user decision Q-P9)");
    assert_eq!(s.advanced.redact, crate::redact::RedactKinds::default());
    let partial: Settings = toml::from_str("version = 4\n[advanced.redact]\nmd5 = false\n").unwrap();
    assert!(!partial.advanced.redact.md5 && partial.advanced.redact.paths);
}
