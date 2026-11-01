//! Tests of the conflict dialog's model (`dialog.rs`): choices, tabs, preselection, the series and similar names.

use std::path::{Path, PathBuf};

use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::naming::{Conflicts, ExistingFile, FolderScan, QueuedTarget};
use rustcut_core::render::builtin_profiles;
use rustcut_core::settings::{ConflictDialogSettings, ConflictPreselect, ExportPanelState, ExportSettings};
use rustcut_core::time::Timestamp;

use super::dialog::{Choice, ConflictDialog, DialogInput, Tab, silent_outcome};
use super::state::{ExportState, JobFacts};

fn export(counter_enabled: bool) -> ExportState {
    let mut st = ExportState::new(
        &ExportSettings::default(),
        &ExportPanelState {
            counter_enabled,
            counter: 1,
            // An empty field is a counter while auto-increment is on: "no counter" needs both off.
            auto_increment: counter_enabled,
            ..ExportPanelState::default()
        },
    );
    st.file_opened(Path::new("/out/Clip.mp4"));
    st
}

fn facts() -> JobFacts {
    JobFacts::new(&builtin_profiles().remove(0), 1, Timestamp::from_secs(5))
}

const FOLDER: [&str; 8] = [
    "Clip - 01.mkv",
    "Clip - 02.mkv",
    "Clip - 07.mp4",
    "Clip - 08.ogg",
    "Clip.mkv",
    "Clip (1).mkv",
    "clip notes.txt",
    "clip notes.MOV",
];

fn dialog_with(
    st: &ExportState,
    conflicts: Conflicts,
    queue: Vec<QueuedTarget>,
    settings: &ConflictDialogSettings,
) -> ConflictDialog {
    let stem = st.file(&facts()).unwrap().0;
    ConflictDialog::new(DialogInput {
        folder: Path::new("/out"),
        stem: &stem,
        conflicts,
        scan: FolderScan {
            readable: true,
            missing: false,
            names: FOLDER.into_iter().map(str::to_owned).collect(),
        },
        queue,
        export: st,
        facts: &facts(),
        settings,
    })
}

fn dialog(st: &ExportState, conflicts: Conflicts, queue: Vec<QueuedTarget>) -> ConflictDialog {
    dialog_with(st, conflicts, queue, &ConflictDialogSettings::default())
}

fn exists() -> Conflicts {
    Conflicts {
        exists: Some(ExistingFile {
            bytes: 1,
            modified: None,
        }),
        ..Conflicts::default()
    }
}

fn choices(d: &ConflictDialog) -> Vec<Choice> {
    d.options.iter().map(|o| o.choice).collect()
}

#[test]
fn an_existing_file_offers_the_free_names_first_then_the_own_name_and_overwrite() {
    let st = export(true);
    let mut d = dialog(&st, exists(), Vec::new());
    assert_eq!(
        choices(&d),
        vec![Choice::NextNumber, Choice::Variant, Choice::Rename, Choice::Overwrite],
        "user review 2026-10-27: Overwrite last"
    );
    assert_eq!((d.selected, d.tab), (0, Tab::Decision));
    assert_eq!(
        d.options[0].file, "Clip - 03.mkv",
        "the series has 01, 02 and 07 (an MP4)"
    );
    assert_eq!(
        d.options[0].detail, "Takes the next number not used in the target folder.",
        "what the choice does, not the counter (review 50 #9)"
    );
    assert_eq!(d.outcome().unwrap().counter, Some(3));
    assert_eq!(
        d.options[1].file, "Clip (1) - 01.mkv",
        "the number belongs to the base name, before the counter (user request 2026-11-01)"
    );
    assert_eq!(d.confirm_text(), "Save", "the chosen row shows the file name");
    assert!(!d.options[2].enabled, "the typed name starts as the taken one");
    assert_eq!(
        (d.options[2].detail_name.as_str(), d.options[2].detail.as_str()),
        ("Clip - 01.mkv", "already exists."),
        "the name in italics before the words (review 50 #1)"
    );
    let decision: Vec<usize> = d.options_of(Tab::Decision).iter().map(|(i, _)| *i).collect();
    assert_eq!(
        decision,
        vec![0, 1, 2, 3],
        "every way out on the first tab, the others below the primary ones"
    );
    assert!(d.options_of(Tab::Similar).is_empty());
    d.type_name("Holiday");
    d.set_tab(Tab::Similar);
    d.select(2);
    assert_eq!(
        (d.outcome().unwrap().stem, d.tab),
        ("Holiday - 01".to_owned(), Tab::Decision),
        "a picked choice shows its tab"
    );
    d.step(1);
    assert_eq!(
        (d.current().unwrap().choice, d.confirm_text()),
        (Choice::Overwrite, "Overwrite")
    );
    assert!(d.outcome().unwrap().overwrite);
    assert_eq!(silent_outcome(&d, &st).unwrap().stem, "Clip - 03");
    d.step(1);
    assert_eq!(d.selected, 0, "wraps around");
}

#[test]
fn the_series_holds_every_number_and_extension_and_similar_names_the_rest() {
    let st = export(true);
    let d = dialog(&st, exists(), Vec::new());
    let series: Vec<(u32, &str)> = d.series.iter().map(|f| (f.counter, f.name.as_str())).collect();
    assert_eq!(
        series,
        vec![(1, "Clip - 01.mkv"), (2, "Clip - 02.mkv"), (7, "Clip - 07.mp4")],
        "the target (01) is part of its series"
    );
    assert_eq!(
        d.others,
        vec!["Clip (1).mkv", "Clip.mkv", "clip notes.MOV"],
        "video files only: no sound file, no text"
    );
    assert_eq!(d.tabs(), vec![Tab::Decision, Tab::Similar]);

    let off = ConflictDialogSettings {
        show_series: false,
        show_similar: false,
        ..ConflictDialogSettings::default()
    };
    let d = dialog_with(&st, exists(), Vec::new(), &off);
    assert!(d.series.is_empty() && d.others.is_empty());
    assert_eq!(d.tabs(), vec![Tab::Decision], "no tab without content");
    let only_similar = ConflictDialogSettings {
        show_series: false,
        ..ConflictDialogSettings::default()
    };
    let d = dialog_with(&st, exists(), Vec::new(), &only_similar);
    assert_eq!(d.others.len(), 5, "without the series its files count as similar names");
}

#[test]
fn the_preselected_way_out_follows_the_settings() {
    let st = export(true);
    let with = |preselect| ConflictDialogSettings {
        preselect,
        ..ConflictDialogSettings::default()
    };
    let d = dialog_with(&st, exists(), Vec::new(), &with(ConflictPreselect::Variant));
    assert_eq!(
        choices(&d),
        vec![Choice::Variant, Choice::NextNumber, Choice::Rename, Choice::Overwrite],
        "\"(n)\" takes the number's place on the first tab"
    );
    assert_eq!((d.current().unwrap().choice, d.tab), (Choice::Variant, Tab::Decision));
    let d = dialog_with(&st, exists(), Vec::new(), &with(ConflictPreselect::Overwrite));
    assert_eq!(d.current().unwrap().choice, Choice::Overwrite);
    // Overwrite cannot be preselected when only queued jobs write the file: the first free name instead.
    let queue = vec![QueuedTarget {
        id: JobId(4),
        target: PathBuf::from("/out/Clip - 01.mkv"),
        state: JobStateKind::Waiting,
    }];
    let queued = Conflicts {
        queued: queue.clone(),
        ..Conflicts::default()
    };
    let d = dialog_with(&st, queued, queue, &with(ConflictPreselect::Overwrite));
    assert_eq!(d.current().unwrap().choice, Choice::NextNumber);
}

#[test]
fn the_next_number_passes_over_the_series_in_other_formats() {
    let mut st = export(true);
    st.counter = 6;
    let d = dialog(&st, Conflicts::default(), Vec::new());
    assert_eq!(
        d.options[0].file, "Clip - 08.mkv",
        "07 exists as an MP4: a series never gets a number twice"
    );
    let hidden = ConflictDialogSettings {
        show_series: false,
        ..ConflictDialogSettings::default()
    };
    let d = dialog_with(&st, Conflicts::default(), Vec::new(), &hidden);
    assert_eq!(
        d.options[0].file, "Clip - 08.mkv",
        "also while the series is not listed"
    );
    assert!(d.series.is_empty());
}

#[test]
fn a_template_without_a_counter_offers_n_first() {
    let mut st = export(false);
    st.template = "{{ name }}".to_owned();
    let d = dialog(&st, exists(), Vec::new());
    assert_eq!(choices(&d), vec![Choice::Variant, Choice::Rename, Choice::Overwrite]);
    assert_eq!(d.options[0].file, "Clip (2).mkv");
    assert!(d.series.is_empty(), "no counter, no series");
    assert_eq!(d.current().unwrap().choice, Choice::Variant);
}

#[test]
fn queued_jobs_offer_save_anyway_and_removal_on_the_first_tab() {
    let mut st = export(false);
    st.counter_enabled = false;
    let queue = vec![QueuedTarget {
        id: JobId(4),
        target: PathBuf::from("/out/Clip.mkv"),
        state: JobStateKind::Waiting,
    }];
    let conflicts = Conflicts {
        queued: queue.clone(),
        ..Conflicts::default()
    };
    let d = dialog(&st, conflicts, queue.clone());
    assert_eq!(
        choices(&d),
        vec![
            Choice::NextNumber,
            Choice::Variant,
            Choice::Rename,
            Choice::KeepName,
            Choice::RemoveQueued
        ]
    );
    assert_eq!(d.options[0].title, "Add a number");
    assert_eq!(d.options[0].file, "Clip - 03.mkv", "01 and 02 are on the drive");
    assert_eq!(d.options[1].file, "Clip (2).mkv");
    assert_eq!(d.options[4].outcome.as_ref().unwrap().remove, vec![JobId(4)]);
    assert!(d.options[3].outcome.as_ref().unwrap().overwrite);
    assert_eq!(silent_outcome(&d, &st).unwrap().stem, "Clip (2)");
    assert!(!d.tabs().contains(&Tab::Queue), "the jobs are the decision itself");
    let running = vec![QueuedTarget {
        state: JobStateKind::Rendering,
        ..queue[0].clone()
    }];
    let d = dialog(
        &st,
        Conflicts {
            queued: running.clone(),
            ..Conflicts::default()
        },
        running,
    );
    assert!(!d.options[4].enabled && d.options[4].detail.contains("rendering"));
}

#[test]
fn tabs_switch_and_wrap_and_a_tab_without_content_is_refused() {
    let st = export(true);
    let queue = vec![QueuedTarget {
        id: JobId(9),
        target: PathBuf::from("/out/Clip - 01.mkv"),
        state: JobStateKind::Waiting,
    }];
    let both = Conflicts {
        queued: queue.clone(),
        ..exists()
    };
    let mut d = dialog(&st, both, queue);
    assert_eq!(d.tabs(), vec![Tab::Decision, Tab::Similar, Tab::Queue]);
    d.cycle_tab(-1);
    assert_eq!(d.tab, Tab::Queue, "backwards from the first wraps to the last");
    d.cycle_tab(1);
    assert_eq!(d.tab, Tab::Decision);
    d.cycle_tab(1);
    assert_eq!(d.tab, Tab::Similar);
    assert_eq!(d.selected, 0, "switching tabs keeps the choice");
    let mut d = dialog(&st, exists(), Vec::new());
    d.set_tab(Tab::Queue);
    assert_eq!(d.tab, Tab::Decision);
    d.set_tab(Tab::Similar);
    assert_eq!(d.tab, Tab::Similar);
}

#[test]
fn the_files_dialog_opens_on_the_similar_files_when_the_target_is_free() {
    // Review 48 (P9.3): nothing has the name yet, so the dialog shows the similar files and "Target" with 0 (a badge
    // since P9.15).
    let st = export(false);
    let free = dialog(&st, Conflicts::default(), Vec::new()).browsing();
    assert_eq!((free.tab, free.target_count()), (Tab::Similar, 0));
    assert_eq!(
        super::dialog_view::tab_labels(&free)
            .first()
            .map(|(title, count, _)| (title.as_str(), count.as_str())),
        Some(("Target", "0"))
    );
    let taken = dialog(&st, exists(), Vec::new()).browsing();
    assert_eq!((taken.tab, taken.target_count()), (Tab::Decision, 1));
}

#[test]
fn a_new_base_name_stays_for_the_session_but_a_number_or_overwrite_keep_the_old_one() {
    let saved = |choice: Choice, typed: &str| {
        let mut st = export(true);
        let mut d = dialog(&st, exists(), Vec::new());
        assert_eq!(d.typed, "Clip", "the field holds the base name, not the stem");
        d.type_name(typed);
        d.select(d.options.iter().position(|o| o.choice == choice).unwrap());
        let outcome = d.outcome().unwrap();
        let stem = outcome.stem.clone();
        if let Some(name) = outcome.name {
            st.name = name;
        }
        st.saved(outcome.counter);
        (stem, st.name.clone(), st.file(&facts()).unwrap().1)
    };
    let row = |stem: &str, name: &str, next: &str| (stem.to_owned(), name.to_owned(), next.to_owned());
    assert_eq!(
        saved(Choice::Variant, ""),
        row("Clip (1) - 01", "Clip (1)", "Clip (1) - 02.mkv")
    );
    assert_eq!(
        saved(Choice::Rename, "Beach"),
        row("Beach - 01", "Beach", "Beach - 02.mkv")
    );
    assert_eq!(saved(Choice::NextNumber, ""), row("Clip - 03", "Clip", "Clip - 04.mkv"));
    assert_eq!(saved(Choice::Overwrite, ""), row("Clip - 01", "Clip", "Clip - 02.mkv"));
}

#[test]
fn the_queued_ways_keep_the_base_name() {
    let st = export(true);
    let queued = QueuedTarget {
        id: JobId(4),
        target: PathBuf::from("/out/Clip - 01.mkv"),
        state: JobStateKind::Waiting,
    };
    let conflicts = Conflicts {
        queued: vec![queued.clone()],
        ..Conflicts::default()
    };
    let d = dialog(&st, conflicts, vec![queued]);
    for choice in [Choice::KeepName, Choice::RemoveQueued] {
        let option = d.options.iter().find(|o| o.choice == choice).unwrap();
        assert_eq!(option.outcome.as_ref().unwrap().name, None);
    }
}

#[test]
fn a_template_without_the_name_types_the_stem_and_keeps_the_base_name() {
    let mut st = export(true);
    st.template = "{{ source }} {{ counter }}".into();
    let mut d = dialog(&st, exists(), Vec::new());
    assert_eq!(d.typed, "Clip 1", "the field holds the stem");
    let variant = d.options.iter().find(|o| o.choice == Choice::Variant).unwrap();
    assert_eq!(variant.file, "Clip 1 (1).mkv");
    assert_eq!(variant.outcome.as_ref().unwrap().name, None);
    d.type_name("Beach");
    let rename = d.options.iter().find(|o| o.choice == Choice::Rename).unwrap();
    let outcome = rename.outcome.as_ref().unwrap();
    assert_eq!((outcome.stem.as_str(), outcome.name.as_ref()), ("Beach", None));
}

#[test]
fn a_verbatim_name_keeps_its_name_in_a_variant() {
    let mut st = export(false);
    st.name = "Clip".into();
    st.verbatim = true;
    let d = dialog(&st, exists(), Vec::new());
    let variant = d.options.iter().find(|o| o.choice == Choice::Variant).unwrap();
    assert_eq!(variant.file, "Clip (2).mkv");
    assert_eq!(variant.outcome.as_ref().unwrap().name.as_deref(), Some("Clip (2)"));
}

#[test]
fn an_empty_counter_with_auto_increment_offers_the_next_number() {
    let mut st = export(false);
    st.auto_increment = true;
    let d = dialog(&st, exists(), Vec::new());
    assert_eq!(
        d.options[0].choice,
        Choice::NextNumber,
        "an empty field is 01 while auto-increment is on"
    );
    assert_eq!(d.options[0].title, "Next free number");
    assert_eq!(
        silent_outcome(&d, &st).unwrap().counter,
        d.options[0].outcome.as_ref().unwrap().counter
    );
}

#[test]
fn keys_are_green_for_the_iterated_number_and_red_for_replacing() {
    use super::dialog_view::key_tint;
    assert_eq!(key_tint(Choice::NextNumber, false, true), 1);
    assert_eq!(key_tint(Choice::NextNumber, false, false), 0);
    assert_eq!(key_tint(Choice::Overwrite, true, true), 2);
    assert_eq!(key_tint(Choice::RemoveQueued, true, false), 2);
    assert_eq!(key_tint(Choice::Variant, false, true), 0);
}
