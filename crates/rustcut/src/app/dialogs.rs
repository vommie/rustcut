//! The dialogs' sizes and the conflict dialog's folded groups (`DialogState` in `ui/dialog_frame.slint`): restored at
//! start, saved at quit as `[dialogs]` in `ui-state.toml` (user request with M5.4).

use rustcut_core::settings::{DialogSize, DialogUiState};
use slint::ComponentHandle as _;

use super::window::to_dimension;
use crate::{DialogState, MainWindow};

/// A dialog's id in `ui-state.toml` and its size properties.
struct Sizes {
    id: &'static str,
    get: fn(&DialogState<'_>) -> (f32, f32),
    set: fn(&DialogState<'_>, f32, f32),
}

const DIALOGS: [Sizes; 4] = [
    Sizes {
        id: "conflict",
        get: |d| (d.get_conflict_width(), d.get_conflict_height()),
        set: |d, w, h| {
            d.set_conflict_width(w);
            d.set_conflict_height(h);
        },
    },
    Sizes {
        id: "metadata",
        get: |d| (d.get_metadata_width(), d.get_metadata_height()),
        set: |d, w, h| {
            d.set_metadata_width(w);
            d.set_metadata_height(h);
        },
    },
    Sizes {
        id: "job_load",
        get: |d| (d.get_job_load_width(), d.get_job_load_height()),
        set: |d, w, h| {
            d.set_job_load_width(w);
            d.set_job_load_height(h);
        },
    },
    Sizes {
        id: "quit",
        get: |d| (d.get_quit_width(), d.get_quit_height()),
        set: |d, w, h| {
            d.set_quit_width(w);
            d.set_quit_height(h);
        },
    },
];

/// Logical pixels for Slint (a size from the file is at most 16384).
fn px(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

pub fn restore(main: &MainWindow, state: &DialogUiState) {
    let d = main.global::<DialogState<'_>>();
    for dialog in &DIALOGS {
        if let Some(size) = state.size(dialog.id) {
            (dialog.set)(&d, px(size.width), px(size.height));
        }
    }
    d.set_conflict_series_folded(state.conflict_series_folded);
    d.set_conflict_others_folded(state.conflict_others_folded);
}

pub fn capture(main: &MainWindow, state: &mut DialogUiState) {
    let d = main.global::<DialogState<'_>>();
    for dialog in &DIALOGS {
        let (w, h) = (dialog.get)(&d);
        // 0 = the dialog's default: nothing to keep.
        let size = (w > 0.0 && h > 0.0).then(|| DialogSize {
            width: to_dimension(w, 0),
            height: to_dimension(h, 0),
        });
        match size.filter(|s| s.width > 0 && s.height > 0) {
            Some(size) => {
                state.sizes.insert(dialog.id.to_owned(), Some(size));
            }
            None => {
                state.sizes.remove(dialog.id);
            }
        }
    }
    state.conflict_series_folded = d.get_conflict_series_folded();
    state.conflict_others_folded = d.get_conflict_others_folded();
}
