//! Shows the conflict dialog's state in the `Export` global: the header, the tabs with their counts, the current tab's
//! choices (each with the key that picks it), the queued jobs and the footer. The previews have their own models
//! (`previews.rs`), filled in place as the files are read; closing the dialog stops them.

use slint::{ComponentHandle as _, Model as _, ModelRc, VecModel};

use super::ExportBridge;
use super::dialog::{Choice, ConflictDialog, Tab};
use super::save::Pending;
use crate::icons::Icon;
use crate::ui::queue::title as state_title;
use crate::util::model::sync;
use crate::{ConflictJobView, ConflictOptionView, Export, GlobalTooltip, TabView};

/// The tabs with what they hold: title, the count for its badge ("" = none) and symbol (P9.15, Q-P9.15 counts A).
pub(super) fn tab_labels(dialog: &ConflictDialog) -> Vec<(String, String, Icon)> {
    dialog
        .tabs()
        .into_iter()
        .map(|tab| match tab {
            Tab::Similar => (
                tab.title().to_owned(),
                (dialog.series.len() + dialog.others.len()).to_string(),
                Icon::TabSimilar,
            ),
            Tab::Queue => (
                tab.title().to_owned(),
                dialog.conflicts.queued.len().to_string(),
                Icon::PanelQueue,
            ),
            Tab::Known => (
                tab.title().to_owned(),
                dialog.known.as_ref().map_or(0, |k| k.files.len()).to_string(),
                Icon::TabKnown,
            ),
            Tab::Decision if dialog.browse => (
                "Target".to_owned(),
                dialog.target_count().to_string(),
                Icon::ExistingFile,
            ),
            Tab::Decision => (tab.title().to_owned(), String::new(), Icon::TabDecision),
        })
        .collect()
}

/// The number Slint uses for a tab (`Export.dialog-tab-kind`).
const fn tab_kind(tab: Tab) -> i32 {
    match tab {
        Tab::Decision => 0,
        Tab::Similar => 2,
        Tab::Queue => 3,
        Tab::Known => 4,
    }
}

impl ExportBridge {
    /// Shows the dialog's state (or closes it and stops its previews).
    pub(super) fn publish_dialog(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let e = window.global::<Export<'_>>();
        let pending = self.pending.borrow();
        let Some(Pending { dialog, .. }) = pending.as_ref() else {
            drop(pending);
            self.stop_previews();
            // Closed by a button under a details tooltip: the tooltip's area is gone and cannot hide it.
            window.global::<GlobalTooltip<'_>>().invoke_hide_cold();
            e.set_dialog_open(false);
            return;
        };
        let c = &dialog.conflicts;
        // Review 50 #7 (P9.3).
        let title = match (c.exists.is_some(), c.queued.is_empty()) {
            (true, false) => "Target file already exists and is queued",
            (true, true) => "Target file already exists",
            (false, false) => "Target filename already used by the queue",
            (false, true) => "Files in the target folder",
        };
        e.set_dialog_known_mode(dialog.known.is_some());
        match &dialog.known {
            Some(known) => {
                e.set_dialog_title(known.title.as_str().into());
                e.set_dialog_text(known.text.as_str().into());
            }
            None => e.set_dialog_title(title.into()),
        }
        e.set_dialog_file(dialog.file.as_str().into());
        let folder = dialog.folder.display().to_string();
        let separator = if folder.ends_with(std::path::MAIN_SEPARATOR) {
            ""
        } else {
            std::path::MAIN_SEPARATOR_STR
        };
        e.set_dialog_path(dialog.folder.join(&dialog.file).display().to_string().into());
        e.set_dialog_file_tail(format!("{separator}{}", dialog.file).into());
        e.set_dialog_folder(folder.into());
        e.set_dialog_has_existing(c.exists.is_some());
        let tabs: Vec<TabView> = tab_labels(dialog)
            .into_iter()
            .map(|(title, count, icon)| TabView {
                title: title.into(),
                icon: icon.to_slint(),
                count: count.into(),
                enabled: true,
            })
            .collect();
        e.set_dialog_tabs(ModelRc::new(VecModel::from(tabs)));
        let tabs = dialog.tabs();
        e.set_dialog_tab(
            tabs.iter()
                .position(|t| *t == dialog.tab)
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(0),
        );
        e.set_dialog_tab_kind(tab_kind(dialog.tab));
        e.set_dialog_jobs_title(
            match c.queued.len() {
                1 => "1 job in the queue writes this file".to_owned(),
                n => format!("{n} jobs in the queue write this file"),
            }
            .into(),
        );
        let jobs: Vec<ConflictJobView> = c
            .queued
            .iter()
            .map(|q| ConflictJobView {
                id: q.id.to_string().into(),
                name: q
                    .target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into(),
                state: state_title(q.state).into(),
                running: q.state.is_active(),
            })
            .collect();
        e.set_dialog_jobs(ModelRc::new(VecModel::from(jobs)));
        let iterating = self.state.borrow().auto_increment;
        let options: Vec<ConflictOptionView> = dialog
            .options_of(dialog.tab)
            .into_iter()
            .map(|(index, o)| ConflictOptionView {
                key: i32::try_from(index).unwrap_or(i32::MAX),
                title: o.title.as_str().into(),
                file: o.file.as_str().into(),
                detail_name: o.detail_name.as_str().into(),
                detail: o.detail.as_str().into(),
                enabled: o.enabled,
                recommended: false,
                typed: o.choice == Choice::Rename,
                key_tint: key_tint(o.choice, o.destructive, iterating),
            })
            .collect();
        // In place: a new model made every row again, and the "Own name" field lost the focus the moment a click put
        // it there (picking the row republishes; user review 2026-10-27: one could not type into it).
        let current = e.get_dialog_tab_options();
        if let Some(model) = current.as_any().downcast_ref::<VecModel<ConflictOptionView>>() {
            sync(model, options);
        } else {
            e.set_dialog_tab_options(ModelRc::new(VecModel::from(options)));
        }
        e.set_dialog_others_title(format!("Other names starting with \"{}\"", self.state.borrow().name).into());
        e.set_dialog_tile_width(i32::try_from(self.dialog_settings.borrow().tile_size.width()).unwrap_or(112));
        e.set_dialog_selected(i32::try_from(dialog.selected).unwrap_or_default());
        e.set_dialog_typed(dialog.typed.as_str().into());
        e.set_dialog_browse(dialog.browse);
        e.set_dialog_can_confirm(dialog.browse || dialog.outcome().is_some());
        e.set_dialog_confirm_text(dialog.confirm_text().into());
        e.set_dialog_open(true);
    }
}

/// The color of a choice's key: green for the next free number while the counter auto-iterates (what saving does
/// anyway), red for a choice that replaces or removes something, else neutral.
pub(super) fn key_tint(choice: Choice, destructive: bool, iterating: bool) -> i32 {
    match (destructive, choice == Choice::NextNumber && iterating) {
        (true, _) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}
