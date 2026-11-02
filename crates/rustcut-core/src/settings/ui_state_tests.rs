//! Tests of `ui-state.toml` (round trips, tolerant reading of old and broken files, sanitizing).

use super::ui_state::*;
use crate::edit::{Crop, CropAspect, CropMemory, FilterChain, Size};
use crate::settings::{VersionedFile as _, load, save};

#[test]
fn round_trip_with_an_opaque_dock_table() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    let mut dock = toml::Table::new();
    dock.insert("left_width".into(), toml::Value::Integer(320));
    state.dock = Some(dock);
    state.queue.state_filter = vec![crate::job::JobStateKind::Failed];
    state.queue.columns.insert("name".into(), 240);
    save(dir.path(), &state).unwrap();
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );
}

#[test]
fn the_tag_panel_round_trips_is_sanitized_and_old_files_load() {
    let dir = tempfile::tempdir().unwrap();
    let state = UiState {
        tags: TagPanelState {
            expanded: vec![1, 2],
            filter: "be.*".into(),
            regex: true,
            history: vec![5, 3],
            history_age: vec![0, 2],
            history_collapsed: true,
            history_height: 120,
            hidden: vec![9],
            shown: vec![11],
            locked: vec![3],
            last_tags: vec![3, 5],
            last_rating: Some(4),
            active: false,
            filter_collapsed: vec![1],
            filter_width: 400,
            filter_height: 600,
            show_hidden: true,
            search_shown: false,
        },
        ..UiState::default()
    };
    save(dir.path(), &state).unwrap();
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );

    let mut bad = UiState::default();
    bad.tags.history = (0..120).chain([1, 1]).collect();
    bad.tags.history_age = vec![0; 200];
    bad.tags.locked = vec![4, 4, 2];
    bad.tags.history_height = 9;
    bad.tags.last_rating = Some(9);
    bad.tags.filter_width = 10;
    assert_eq!(bad.sanitize().len(), 4);
    assert_eq!(bad.tags.filter_width, 260);
    assert_eq!(bad.tags.history.len(), TagPanelState::HISTORY_MAX);
    assert_eq!(
        bad.tags.history_age.len(),
        TagPanelState::HISTORY_MAX,
        "the ages stay beside their tags"
    );
    let mut twice = UiState::default();
    twice.tags.history = vec![4, 4, 6];
    twice.tags.history_age = vec![1, 2, 3];
    twice.sanitize();
    assert_eq!((twice.tags.history, twice.tags.history_age), (vec![4, 6], vec![1, 3]));
    assert_eq!(bad.tags.locked, vec![4, 2]);
    assert_eq!((bad.tags.history_height, bad.tags.last_rating), (40, None));

    // A file of 0.0.11 has no `[tags]`; a `[tags]` this version cannot read keeps the rest of the file.
    for tags in ["", "[tags]\nhistory = \"many\"\n"] {
        std::fs::write(
            dir.path().join("ui-state.toml"),
            format!("version = 1\n[timeline]\nzoom = 4.0\n{tags}"),
        )
        .unwrap();
        let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
        assert_eq!((read.timeline.zoom, read.tags.clone()), (4.0, TagPanelState::default()));
    }
    // A `[tags]` of 0.0.32 has no "Active" switch, no shown tags, no filter window and no eye switch: active, nothing
    // shown, defaults.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[tags]\nhidden = [3]\nlocked = [2]\n",
    )
    .unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert!(read.tags.active && read.tags.shown.is_empty());
    assert!(read.tags.history_age.is_empty(), "a file of 0.0.75 has no ages");
    assert!(
        !read.tags.show_hidden,
        "a file of 0.0.42 has no eye switch: the hidden tags stay hidden"
    );
    assert_eq!((read.tags.hidden.clone(), read.tags.locked.clone()), (vec![3], vec![2]));
    assert_eq!((read.tags.filter_width, read.tags.filter_height), (380, 560));
    assert!(
        read.tags.search_shown,
        "a file of 0.0.71 has no search toggle: the field shows"
    );
}

#[test]
fn absurd_values_are_reset() {
    let mut state = UiState::default();
    state.window.width = 5;
    state.window.maximized = true;
    state.export.volume = 255;
    state.queue.columns.insert("x".into(), 1);
    assert_eq!(state.sanitize().len(), 2);
    assert_eq!((state.window.width, state.window.maximized), (1280, true));
    assert!(state.queue.columns.is_empty());
    assert_eq!(state.export.volume, 100);
}

#[test]
fn an_amplified_volume_of_an_older_version_becomes_100() {
    // Up to 0.0.71 the volume went to 150 %; such a file loads and plays at 100 %.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[export]\nvolume = 150\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(value.export.volume, 100);
}

#[test]
fn the_timeline_zoom_is_kept_in_range_and_old_files_load() {
    let mut state = UiState::default();
    state.timeline.zoom = 0.2;
    assert_eq!(state.sanitize().len(), 1);
    assert_eq!(state.timeline.zoom, 1.0);
    state.timeline.zoom = 1e9;
    state.sanitize();
    assert_eq!(state.timeline.zoom, TimelineState::MAX_ZOOM);
    state.timeline.zoom = f64::NAN;
    state.sanitize();
    assert_eq!(state.timeline.zoom, 1.0);

    // A file of 0.0.5 has no `[timeline]`.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[window]\nwidth = 1000\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!((value.window.width, value.timeline.zoom), (1000, 1.0));
}

#[test]
fn kept_filters_round_trip_and_bad_values_read_as_missing() {
    use crate::edit::{Crop, Filter, FilterKind, Orientation};
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    let mut chain = FilterChain::default();
    chain.set(Filter::Crop(Crop::new(2, 4, 6, 8)));
    chain.set_enabled(FilterKind::Crop, true);
    chain.set(Filter::Orientation(Orientation::COUNTER_CLOCKWISE.into()));
    chain.move_up(FilterKind::Orientation);
    state.export.keep_filters = true;
    state.export.filters = Some(chain);
    state.export.crop_aspect = CropAspect::Wide;
    state.export.filter_menu_pinned = true;
    save(dir.path(), &state).unwrap();
    let text = std::fs::read_to_string(dir.path().join("ui-state.toml")).unwrap();
    assert!(text.contains("crop_aspect = \"16:9\""), "{text}");
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );

    // The kept filters of 0.0.47 (TOML): the Rotate and Mirror become one Orientation, nothing else changes.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[export]\nkeep_filters = true\n\
         [[export.filters]]\nenabled = true\n[export.filters.filter]\nkind = \"crop\"\ntop = 8\nright = 0\nbottom = 8\nleft = 0\n\
         [[export.filters]]\nenabled = true\n[export.filters.filter]\nkind = \"rotate\"\nrotation = \"clockwise\"\n\
         [[export.filters]]\nenabled = true\n[export.filters.filter]\nkind = \"mirror\"\nhorizontal = true\nvertical = false\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    let kept = value.export.filters.expect("the old chain loads");
    assert_eq!(
        kept.order().take(2).collect::<Vec<_>>(),
        vec![FilterKind::Crop, FilterKind::Orientation]
    );
    assert_eq!(
        kept.get(FilterKind::Orientation).map(|s| (s.enabled, s.filter)),
        Some((
            true,
            Filter::Orientation(Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL).into())
        ))
    );
    assert_eq!(value.export.crop_search, None);

    // A file of 0.0.6 has neither field; a broken chain or aspect does not reset the rest.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[export]\nvolume = 70\nkeep_filters = true\ncrop_aspect = \"7:3\"\nfilters = 5\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(value.export.volume, 70);
    assert!(value.export.keep_filters);
    assert_eq!(value.export.filters, None);
    assert_eq!(value.export.crop_aspect, CropAspect::Free);
    // The menu's pin is new in 0.0.49: an older file has none.
    assert!(!value.export.filter_menu_pinned);

    // 0.0.51-0.0.59 kept the crop's multiple with the filters; it is gone (the final size rule lives in config.toml,
    // Q-P9s): such a file still loads, the rest unchanged.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[export]\nvolume = 55\nkeep_filters = true\ncrop_multiple = 8\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(value.export.volume, 55);
    assert!(value.export.keep_filters);
}

#[test]
fn the_render_working_copy_and_probe_switches() {
    use crate::render::{VideoEncoder, builtin_profiles};
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    let mut working = builtin_profiles().remove(0);
    working.set_encoder(VideoEncoder::X264);
    state.export.profile = Some("av1-balanced".into());
    state.export.render = Some(working);
    state.export.probe = ProbeState {
        seconds: 20,
        exact: true,
    };
    save(dir.path(), &state).unwrap();
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );

    // 0.0.8 wrote neither; a broken copy or switch does not reset the rest.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[export]\nvolume = 60\nprofile = \"x265-quality\"\nprobe = 3\n\
         [export.render]\nid = 1\n",
    )
    .unwrap();
    let value = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(value.export.volume, 60);
    assert_eq!(value.export.profile.as_deref(), Some("x265-quality"));
    assert_eq!(value.export.render, None);
    assert_eq!(value.export.probe, ProbeState::default());

    let mut state = UiState::default();
    state.export.probe.seconds = 600;
    assert_eq!(state.sanitize().len(), 1);
    assert_eq!(state.export.probe.seconds, 60);
}

#[test]
fn positions_off_every_screen_or_half_known_are_dropped() {
    let mut state = UiState::default();
    (state.window.x, state.window.y) = (Some(-40), Some(1200));
    assert!(state.sanitize().is_empty());
    state.window.x = Some(900_000);
    assert_eq!(state.sanitize(), vec!["window position reset".to_owned()]);
    assert_eq!((state.window.x, state.window.y), (None, None));
    state.window.y = Some(10);
    assert_eq!(state.sanitize().len(), 1);
    assert_eq!(state.window.y, None);
}

#[test]
fn the_queue_job_and_logs_panels_round_trip_and_old_files_load() {
    use crate::job::{JobId, JobStateKind};
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    state.queue.state_filter = vec![JobStateKind::Waiting, JobStateKind::Failed];
    state.queue.search = "clip".into();
    state.queue.folder_filter = vec!["/out/clips".into()];
    state.queue.profile_filter = vec!["AV1 balanced".into(), "x265".into()];
    state.queue.filter_any = true;
    state.queue.selected = vec![JobId(7), JobId(3)];
    state.queue.current = Some(JobId(3));
    state.job.collapsed = vec!["filters".into()];
    state.logs = LogPanelState {
        search: "error".into(),
        errors_only: true,
        follow: false,
        source: LogSource::App,
        shareable: true,
    };
    save(dir.path(), &state).unwrap();
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );

    // 0.0.12 wrote none of the new fields; a state chip of a later version is left out, the other chips stay; broken
    // panel tables keep the rest of the file.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\njob = 4\n[queue]\npaused = true\nstate_filter = [\"waiting\", \"teleported\", \"failed\"]\n\
         search = \"x\"\n[timeline]\nzoom = 2.0\n[logs]\nerrors_only = \"yes\"\n",
    )
    .unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert!(read.queue.paused);
    assert_eq!(
        read.queue.state_filter,
        vec![JobStateKind::Waiting, JobStateKind::Failed]
    );
    assert_eq!((read.queue.selected.clone(), read.queue.current), (Vec::new(), None));
    // 0.0.40's filter menu: no folder or profile filter, and the filters combine with ALL.
    assert!(read.queue.folder_filter.is_empty() && read.queue.profile_filter.is_empty() && !read.queue.filter_any);
    assert_eq!(read.timeline.zoom, 2.0);
    assert_eq!(read.logs, LogPanelState::default());
    assert!(read.logs.follow, "following the newest line is the default");
    assert_eq!(read.job, JobPanelState::default());

    let mut bad = UiState::default();
    bad.queue.selected = vec![JobId(1), JobId(1), JobId(2)];
    bad.queue.state_filter = vec![JobStateKind::Held, JobStateKind::Held];
    bad.queue.search = "a".repeat(QueuePanelState::SEARCH_MAX + 5);
    bad.queue.folder_filter = vec!["/a".into(), "/a".into(), String::new()];
    assert_eq!(bad.sanitize().len(), 1, "only the search is worth a note");
    assert_eq!(bad.queue.folder_filter, vec!["/a".to_owned()]);
    assert_eq!(bad.queue.selected, vec![JobId(1), JobId(2)]);
    assert_eq!(bad.queue.state_filter, vec![JobStateKind::Held]);
    assert_eq!(bad.queue.search.len(), QueuePanelState::SEARCH_MAX);
}

#[test]
fn the_settings_window_round_trips_and_old_files_load() {
    let dir = tempfile::tempdir().unwrap();
    // 0.0.14 wrote no `[settings]`.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[window]\nwidth = 1000\n",
    )
    .unwrap();
    let loaded = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(loaded.settings, SettingsWindowState::default());

    let state = UiState {
        settings: SettingsWindowState {
            width: 1000,
            height: 700,
            x: Some(-20),
            y: Some(340),
            page: "hotkeys".into(),
            profile: Some("my-hevc".into()),
            hotkey_search: "crop".into(),
            messages_tab: 1,
            messages_search: "failed".into(),
            diagnostics_passed: [true, false],
            folder_selected: Some(crate::folders::FolderId(3)),
            folder_collapsed: vec![crate::folders::FolderId(1), crate::folders::FolderId(4)],
            folded_pages: vec!["plugins".into()],
        },
        ..UiState::default()
    };
    save(dir.path(), &state).unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.settings, state.settings);

    // A size off any screen is reset, the page stays; a value this version cannot read counts as missing.
    let mut state = UiState::default();
    state.settings.width = 10;
    state.settings.page = "queue".into();
    assert_eq!(state.sanitize().len(), 1);
    assert_eq!((state.settings.width, state.settings.page.as_str()), (900, "queue"));
    state.settings.messages_tab = 7;
    assert_eq!(state.sanitize().len(), 1);
    assert_eq!(state.settings.messages_tab, 0);
    // 0.0.25 wrote no messages tab: it starts on the first.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1
[settings]
page = \"messages\"
hotkey_search = \"x\"
",
    )
    .unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(
        (read.settings.messages_tab, read.settings.hotkey_search.as_str()),
        (0, "x")
    );
    assert_eq!(
        read.settings.diagnostics_passed, [false; 2],
        "0.0.54's field starts folded"
    );
    assert_eq!(
        (read.settings.x, read.settings.y),
        (None, None),
        "0.0.66 kept no position: the window manager places the window"
    );
    std::fs::write(dir.path().join("ui-state.toml"), "version = 1\nsettings = 3\n").unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.settings, SettingsWindowState::default());
}

#[test]
fn the_log_source_round_trips_and_unknown_sources_read_as_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    state.logs.source = LogSource::RustView;
    save(dir.path(), &state).unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.logs.source, LogSource::RustView);
    let text = std::fs::read_to_string(dir.path().join("ui-state.toml")).unwrap();
    assert!(text.contains("source = \"rustview\""), "{text}");
    state.logs.source = LogSource::Messages;
    save(dir.path(), &state).unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.logs.source, LogSource::Messages);
    // 0.0.15 wrote no source; a later version's source keeps the rest of the panel.
    for text in [
        "version = 1\n[logs]\nsearch = \"x\"\n",
        "version = 1\n[logs]\nsearch = \"x\"\nsource = \"gpu\"\n",
    ] {
        std::fs::write(dir.path().join("ui-state.toml"), text).unwrap();
        let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
        assert_eq!((read.logs.source, read.logs.search.as_str()), (LogSource::Job, "x"));
        assert!(!read.logs.shareable, "0.0.55's shareable view starts off");
    }
}

#[test]
fn dialog_sizes_round_trip_and_old_files_load() {
    use crate::settings::{DialogSize, DialogUiState};
    let dir = tempfile::tempdir().unwrap();
    // 0.0.19 wrote no `[dialogs]`.
    std::fs::write(
        dir.path().join("ui-state.toml"),
        "version = 1\n[window]\nwidth = 1000\n",
    )
    .unwrap();
    let loaded = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(loaded.dialogs, DialogUiState::default());

    let mut state = UiState::default();
    state.dialogs.sizes.insert(
        "conflict".into(),
        Some(DialogSize {
            width: 720,
            height: 560,
        }),
    );
    state.dialogs.conflict_others_folded = true;
    save(dir.path(), &state).unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.dialogs, state.dialogs);
    std::fs::write(dir.path().join("ui-state.toml"), "version = 1\ndialogs = 3\n").unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.dialogs, DialogUiState::default(), "unreadable = missing");
}

#[test]
fn a_file_without_a_crop_history_loads_and_a_stored_one_comes_back_clean() {
    let old: UiState = toml::from_str("version = 2\n[export]\nkeep_filters = true\n").unwrap();
    assert!(old.export.crop_history.is_empty());

    let hd = Size::new(1920, 1080);
    let entry = |top| CropMemory {
        frame: hd,
        crop: Crop::new(top, 0, top, 0),
        pinned: top == 140,
        used: 1_793_000_000,
        source: "clip.mkv".to_owned(),
    };
    let mut state = UiState::default();
    state.export.crop_history = vec![entry(140), entry(0), entry(140), entry(70)];
    let notes = state.sanitize();
    assert!(notes.iter().any(|n| n.contains("crop history")), "{notes:?}");
    assert_eq!(state.export.crop_history, [entry(140), entry(70)]);

    let text = toml::to_string(&state).unwrap();
    let back: UiState = toml::from_str(&text).unwrap();
    assert_eq!(back.export.crop_history, state.export.crop_history);

    // One entry this version cannot read costs only that entry; a broken list costs only the list.
    let text = "version = 2\n[[export.crop_history]]\nused = 1\n[[export.crop_history]]\nframe = { width = 1920, \
                height = 1080 }\ncrop = { top = 70, bottom = 70 }\n";
    let some: UiState = toml::from_str(text).unwrap();
    assert_eq!(some.export.crop_history.len(), 1);
    let broken: UiState = toml::from_str("version = 2\n[export]\ncrop_history = 3\nkeep_filters = true\n").unwrap();
    assert!(broken.export.crop_history.is_empty());
    assert!(broken.export.keep_filters);
}
