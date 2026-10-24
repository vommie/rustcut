//! Dialogs: how RustCut's dialogs behave (user request with M5.4); for now the conflict dialog of saving a job.

use rustcut_core::settings::{ConflictPreselect, PictureScope, TileSize};

use super::Card;
use crate::ui::settings::field::{choice, number, switch};

pub static DIALOGS: &[Card] = &[Card {
    title: "File already exists",
    description: "The dialog that opens when a saved job would write a file that exists or that a queued job writes. \
                  Whether it asks at all is on the Warnings page.",
    fields: &[
        choice!(
            "dialogs.conflict.preselect",
            "Chosen when it opens",
            "The way out the dialog chooses for you; Enter takes it. Overwrite is only chosen when the file exists.",
            dialogs.conflict.preselect,
            [
                (ConflictPreselect::NextNumber, "Next free number"),
                (ConflictPreselect::Variant, "Add \"(n)\""),
                (ConflictPreselect::Overwrite, "Overwrite")
            ]
        ),
        switch!(
            "dialogs.conflict.show_series",
            "List the numbered series",
            "Every file named like this job with another number (\"Name - 01.mkv\", \"Name - 02.mp4\", ...), in any \
             format, on the \"Similar files\" tab.",
            dialogs.conflict.show_series
        ),
        switch!(
            "dialogs.conflict.show_similar",
            "List other similar names",
            "Other files whose names start with the job's name, on the \"Similar files\" tab.",
            dialogs.conflict.show_similar
        ),
        choice!(
            "dialogs.conflict.pictures",
            "Pictures",
            "Which files get a picture. Without one, a file still shows its size, length and resolution.",
            dialogs.conflict.pictures,
            [
                (PictureScope::All, "Every file"),
                (PictureScope::Existing, "The existing file only"),
                (PictureScope::Off, "None")
            ]
        ),
        number!(
            "dialogs.conflict.picture_at_percent",
            "Picture taken at",
            "Where in a video its picture is taken, in percent of its length (the very first frame is often black).",
            dialogs.conflict.picture_at_percent: u8,
            0..=90,
            5,
            "%"
        ),
        number!(
            "dialogs.conflict.parallel_pictures",
            "Pictures made at once",
            "How many files are read at the same time. More is faster on many cores and slows a running render a \
             little.",
            dialogs.conflict.parallel_pictures: u8,
            1..=8,
            1,
            ""
        ),
        choice!(
            "dialogs.conflict.tile_size",
            "Picture size in the list",
            "The size of the pictures in the list on the \"Similar files\" tab.",
            dialogs.conflict.tile_size,
            [
                (TileSize::Small, "Small"),
                (TileSize::Medium, "Medium"),
                (TileSize::Large, "Large")
            ]
        ),
    ],
}];
