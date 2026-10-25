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
fn an_existing_file_offers_a_number_and_overwrite_first_then_other_names() {
    let st = export(true);
    let mut d = dialog(&st, exists(), Vec::new());
    assert_eq!(
        choices(&d),
        vec![Choice::NextNumber, Choice::Overwrite, Choice::Variant, Choice::Rename]
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
    assert_eq!(d.options[2].file, "Clip - 01 (1).mkv");
    assert_eq!(d.confirm_text(), "Save", "the chosen row shows the file name");
    assert!(!d.options[3].enabled, "the typed name starts as the taken one");
    assert_eq!(
        (d.options[3].detail_name.as_str(), d.options[3].detail.as_str()),
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
    d.type_name("Clip - 02");
    assert!(d.options[3].detail.contains("taken"));
    d.type_name("Holiday");
    d.set_tab(Tab::Similar);
    d.select(3);
    assert_eq!(
        (d.outcome().unwrap().stem, d.tab),
        ("Holiday".to_owned(), Tab::Decision),
        "a picked choice shows its tab"
    );
    d.step(1);
    assert_eq!(d.selected, 0, "wraps around");
    d.step(1);
    assert_eq!(
        (d.current().unwrap().choice, d.confirm_text()),
        (Choice::Overwrite, "Overwrite")
    );
    assert!(d.outcome().unwrap().overwrite);
    assert_eq!(silent_outcome(&d, &st).unwrap().stem, "Clip - 03");
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
        vec![Choice::Variant, Choice::Overwrite, Choice::NextNumber, Choice::Rename],
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
    assert_eq!(choices(&d), vec![Choice::Variant, Choice::Overwrite, Choice::Rename]);
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
            Choice::KeepName,
            Choice::RemoveQueued,
            Choice::Variant,
            Choice::Rename
        ]
    );
    assert_eq!(d.options[0].title, "Add a number");
    assert_eq!(d.options[0].file, "Clip - 03.mkv", "01 and 02 are on the drive");
    assert_eq!(d.options[3].file, "Clip (2).mkv");
    assert_eq!(d.options[2].outcome.as_ref().unwrap().remove, vec![JobId(4)]);
    assert!(d.options[1].outcome.as_ref().unwrap().overwrite);
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
    assert!(!d.options[2].enabled && d.options[2].detail.contains("rendering"));
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
    // Review 48 (P9.3): nothing has the name yet, so the dialog shows the similar files and "Target (0)".
    let st = export(false);
    let free = dialog(&st, Conflicts::default(), Vec::new()).browsing();
    assert_eq!((free.tab, free.target_count()), (Tab::Similar, 0));
    assert_eq!(
        super::dialog_view::tab_titles(&free).first().map(String::as_str),
        Some("Target (0)")
    );
    let taken = dialog(&st, exists(), Vec::new()).browsing();
    assert_eq!((taken.tab, taken.target_count()), (Tab::Decision, 1));
}
