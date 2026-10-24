//! Where a jump mark leads (user request Q-N6): the places in the UI that a message can show the user directly.

use rustcut_core::job::JobId;

use crate::ui::render_settings::rows::Row;
use crate::ui::render_settings::view;
use crate::ui::settings::page::Page;

/// A place a message can lead the user to: its panel or window is shown, the element scrolls into view and flashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpTarget {
    /// A row of the Codecs panel (the working copy of the render settings).
    Codecs(Row),
    /// An option of the Settings window: its page and the option's key (`Field::key`).
    Settings { page: Page, key: &'static str },
    /// A job of the queue panel (a failed job's toast, M6.5e): selected, scrolled into view, flashed.
    QueueJob(JobId),
    /// The Sections card (saving without a section, M6.5e).
    Sections,
    /// A row of the section list (saving with overlapping sections).
    SectionRow(usize),
}

impl JumpTarget {
    /// The key the element in Slint carries (`JumpFlash.key`).
    pub fn element_key(self) -> String {
        match self {
            Self::Codecs(row) => format!("codecs:{}", row.key()),
            Self::Settings { key, .. } => format!("settings:{key}"),
            Self::QueueJob(id) => format!("queue:{}", id.0),
            Self::Sections => "export:sections".to_owned(),
            Self::SectionRow(row) => format!("sections:{row}"),
        }
    }

    /// The tooltip of the button that jumps there.
    pub fn hint(self) -> String {
        match self {
            Self::Codecs(row) => format!("Show the option in the Codecs panel ({})", view::row_label(row)),
            Self::Settings { page, .. } => format!("Show the option in Settings > {}", page.title()),
            Self::QueueJob(id) => format!("Show job {id} in the queue"),
            Self::Sections => "Show the Sections card".to_owned(),
            Self::SectionRow(row) => format!("Show section {} in the list", row + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_name_their_element_and_place() {
        let codecs = JumpTarget::Codecs(Row::AudioCodec);
        assert_eq!(codecs.element_key(), "codecs:audio-codec");
        assert_eq!(codecs.hint(), "Show the option in the Codecs panel (Audio)");
        let settings = JumpTarget::Settings {
            page: Page::RustView,
            key: "rustview.binary",
        };
        assert_eq!(settings.element_key(), "settings:rustview.binary");
        assert_eq!(settings.hint(), "Show the option in Settings > RustView");
        let job = JumpTarget::QueueJob(JobId(7));
        assert_eq!(
            (job.element_key(), job.hint()),
            ("queue:7".to_owned(), "Show job #7 in the queue".to_owned())
        );
        assert_eq!(JumpTarget::SectionRow(1).element_key(), "sections:1");
    }
}
