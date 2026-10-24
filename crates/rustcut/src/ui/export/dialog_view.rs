//! Shows the conflict dialog's state in the `Export` global: the header, the tabs with their counts, the current tab's
//! choices (each with the key that picks it), the queued jobs and the footer. The previews have their own models
//! (`previews.rs`), filled in place as the files are read; closing the dialog stops them.

use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use super::ExportBridge;
use super::dialog::{Choice, ConflictDialog, Tab};
use super::save::Pending;
use crate::ui::queue::title as state_title;
use crate::{ConflictJobView, ConflictOptionView, Export, GlobalTooltip};

/// The titles of the tabs with what they hold ("Similar files (12)").
pub(super) fn tab_titles(dialog: &ConflictDialog) -> Vec<String> {
    dialog
        .tabs()
        .into_iter()
        .map(|tab| match tab {
            Tab::Similar => format!("{} ({})", tab.title(), dialog.series.len() + dialog.others.len()),
            Tab::Queue => format!("{} ({})", tab.title(), dialog.conflicts.queued.len()),
            Tab::Decision if dialog.browse => "Target".to_owned(),
            Tab::Decision => tab.title().to_owned(),
        })
        .collect()
}

/// The number Slint uses for a tab (`Export.dialog-tab-kind`).
const fn tab_kind(tab: Tab) -> i32 {
    match tab {
        Tab::Decision => 0,
        Tab::Similar => 2,
        Tab::Queue => 3,
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
        let title = match (c.exists.is_some(), c.queued.is_empty()) {
            (true, false) => "The file exists, and queued jobs write it too",
            (true, true) => "The file exists already",
            (false, false) => "Queued jobs write the same file",
            (false, true) => "Files in the target folder",
        };
        e.set_dialog_title(title.into());
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
        let titles: Vec<SharedString> = tab_titles(dialog).into_iter().map(Into::into).collect();
        e.set_dialog_tabs(ModelRc::new(VecModel::from(titles)));
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
        let options: Vec<ConflictOptionView> = dialog
            .options_of(dialog.tab)
            .into_iter()
            .map(|(index, o)| ConflictOptionView {
                key: i32::try_from(index).unwrap_or(i32::MAX),
                title: o.title.as_str().into(),
                file: o.file.as_str().into(),
                detail: o.detail.as_str().into(),
                enabled: o.enabled,
                destructive: o.destructive,
                typed: o.choice == Choice::Rename,
            })
            .collect();
        e.set_dialog_tab_options(ModelRc::new(VecModel::from(options)));
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
