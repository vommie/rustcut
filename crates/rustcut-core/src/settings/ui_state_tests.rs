//! Tests of `ui-state.toml` (round trips, tolerant reading of old and broken files, sanitizing).

use super::ui_state::*;
use crate::edit::{CropAspect, FilterChain};
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
            history_collapsed: true,
            history_height: 120,
            hidden: vec![9],
            locked: vec![3],
            last_tags: vec![3, 5],
            last_rating: Some(4),
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
    bad.tags.locked = vec![4, 4, 2];
    bad.tags.history_height = 9;
    bad.tags.last_rating = Some(9);
    assert_eq!(bad.sanitize().len(), 3);
    assert_eq!(bad.tags.history.len(), TagPanelState::HISTORY_MAX);
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
    use crate::edit::{Crop, Filter, FilterKind, Rotation};
    let dir = tempfile::tempdir().unwrap();
    let mut state = UiState::default();
    let mut chain = FilterChain::default();
    chain.set(Filter::Crop(Crop::new(2, 4, 6, 8)));
    chain.set_enabled(FilterKind::Crop, true);
    chain.set(Filter::Rotate {
        rotation: Rotation::CounterClockwise,
    });
    chain.move_up(FilterKind::Rotate);
    state.export.keep_filters = true;
    state.export.filters = Some(chain);
    state.export.crop_aspect = CropAspect::Wide;
    save(dir.path(), &state).unwrap();
    let text = std::fs::read_to_string(dir.path().join("ui-state.toml")).unwrap();
    assert!(text.contains("crop_aspect = \"16:9\""), "{text}");
    assert_eq!(
        load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value(),
        state
    );

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
    state.queue.selected = vec![JobId(7), JobId(3)];
    state.queue.current = Some(JobId(3));
    state.job.collapsed = vec!["filters".into()];
    state.logs = LogPanelState {
        search: "error".into(),
        errors_only: true,
        follow: false,
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
    assert_eq!(read.timeline.zoom, 2.0);
    assert_eq!(read.logs, LogPanelState::default());
    assert!(read.logs.follow, "following the newest line is the default");
    assert_eq!(read.job, JobPanelState::default());

    let mut bad = UiState::default();
    bad.queue.selected = vec![JobId(1), JobId(1), JobId(2)];
    bad.queue.state_filter = vec![JobStateKind::Held, JobStateKind::Held];
    bad.queue.search = "a".repeat(QueuePanelState::SEARCH_MAX + 5);
    assert_eq!(bad.sanitize().len(), 1, "only the search is worth a note");
    assert_eq!(bad.queue.selected, vec![JobId(1), JobId(2)]);
    assert_eq!(bad.queue.state_filter, vec![JobStateKind::Held]);
    assert_eq!(bad.queue.search.len(), QueuePanelState::SEARCH_MAX);
}

#[test]
fn the_settings_window_round_trips_and_old_files_load() {
    let dir = tempfile::tempdir().unwrap();
    // 0.0.14 wrote no `[settings]`.
    std::fs::write(dir.path().join("ui-state.toml"), "version = 1\n[window]\nwidth = 1000\n").unwrap();
    let loaded = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(loaded.settings, SettingsWindowState::default());

    let mut state = UiState::default();
    state.settings = SettingsWindowState {
        width: 1000,
        height: 700,
        page: "hotkeys".into(),
        profile: Some("my-hevc".into()),
        hotkey_search: "crop".into(),
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
    std::fs::write(dir.path().join("ui-state.toml"), "version = 1\nsettings = 3\n").unwrap();
    let read = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap().into_value();
    assert_eq!(read.settings, SettingsWindowState::default());
}
