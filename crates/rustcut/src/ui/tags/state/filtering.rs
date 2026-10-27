//! The tree's rows through the tag filter field (moved out of `state.rs` to keep it short): the filter applies only while
//! its field is shown (the toggle under the history, user request 2026-10-27).

use std::collections::HashSet;

use regex::{Regex, RegexBuilder};

use super::super::tree::{TagFlattenContext, TagTreeBuilder};
use super::TagPanel;
use crate::TagNodeView;

impl TagPanel {
    /// The filter that applies: none while its field is hidden.
    fn filter_text(&self) -> &str {
        if self.search_shown { self.filter.trim() } else { "" }
    }

    /// The eye: the hidden tags show in the tree (dimmed) or not.
    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
    }

    /// The filter as a regular expression (case-insensitive) when the regex switch is on; `Err` = it does not compile
    /// and the text is matched as a substring (RustView's fallback).
    pub fn filter_regex(&self) -> Result<Option<Regex>, regex::Error> {
        let text = self.filter_text();
        if !self.regex || text.is_empty() {
            return Ok(None);
        }
        RegexBuilder::new(text)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .map(Some)
            .inspect_err(|e| {
                tracing::debug!(error = %e, "[Tags] The tag filter is not a valid regular expression; matching as text");
            })
    }

    /// The visible rows of the tree.
    pub fn rows(&self) -> Vec<TagNodeView> {
        let regex = self.filter_regex().ok().flatten();
        let filter = self.filter_text().to_lowercase();
        let assigned: HashSet<u32> = self.choice().tags.iter().copied().collect();
        let locked: HashSet<u32> = if self.editing.is_some() {
            HashSet::new()
        } else {
            self.locked.iter().copied().collect()
        };
        let ctx = TagFlattenContext {
            expanded_set: &self.expanded,
            filter_text: &filter,
            filter_regex: regex.as_ref(),
            assigned: &assigned,
            locked: &locked,
            hidden: &self.marks.hidden,
            shown: &self.marks.shown,
            show_hidden: self.show_hidden,
        };
        let mut rows = Vec::new();
        TagTreeBuilder::flatten(&self.forest, 0, &ctx, &mut rows);
        rows
    }
}
