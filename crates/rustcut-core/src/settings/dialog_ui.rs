//! `[dialogs]` in `ui-state.toml` (0.0.20, user request with M5.4): the size the user gave each dialog and the folded
//! groups of the conflict dialog's "Similar files" list. A file without the section reads the defaults.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};

/// A dialog's size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogSize {
    pub width: u32,
    pub height: u32,
}

#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogUiState {
    /// By dialog id (`conflict`, `metadata`, `job_load`, `quit`); a missing dialog has its default size. A value this
    /// version cannot read counts as missing.
    #[serde_as(as = "DefaultOnError<BTreeMap<_, DefaultOnError<Option<_>>>>")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sizes: BTreeMap<String, Option<DialogSize>>,
    /// The conflict dialog's numbered series is folded.
    pub conflict_series_folded: bool,
    /// The conflict dialog's other similar names are folded.
    pub conflict_others_folded: bool,
}

impl DialogUiState {
    /// Sizes a dialog can have (smaller ones are a broken file, larger ones no screen).
    pub const SIZE: std::ops::RangeInclusive<u32> = 200..=16_384;

    /// The size of the dialog `id`, if the user gave it one.
    pub fn size(&self, id: &str) -> Option<DialogSize> {
        self.sizes.get(id).copied().flatten()
    }

    /// Unreadable and impossible sizes are dropped (the dialog takes its default); the notes say what was dropped.
    pub fn sanitize(&mut self, notes: &mut Vec<String>) {
        let before = self.sizes.len();
        self.sizes
            .retain(|_, size| size.is_some_and(|s| Self::SIZE.contains(&s.width) && Self::SIZE.contains(&s.height)));
        if self.sizes.len() != before {
            notes.push(format!("{} dialog sizes dropped", before - self.sizes.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_trip_and_broken_ones_are_dropped() {
        let text = "conflict_series_folded = true\n[sizes]\nconflict = { width = 700, height = 540 }\n\
                    quit = { width = 10, height = 300 }\nmetadata = \"wide\"\n";
        let mut state: DialogUiState = toml::from_str(text).unwrap();
        assert!(state.conflict_series_folded && !state.conflict_others_folded);
        assert_eq!(
            state.size("conflict"),
            Some(DialogSize {
                width: 700,
                height: 540
            })
        );
        assert_eq!(state.size("metadata"), None, "unreadable = missing");
        let mut notes = Vec::new();
        state.sanitize(&mut notes);
        assert_eq!(
            state.sizes.len(),
            1,
            "the tiny quit dialog and the unreadable one are gone"
        );
        assert_eq!(notes.len(), 1);
        let written = toml::to_string(&state).unwrap();
        assert_eq!(toml::from_str::<DialogUiState>(&written).unwrap(), state);
    }
}
