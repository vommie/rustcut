//! The Codecs panel's bubbles (M6.5c, user decision Q-N10): the issues of the settings at the first row of their field
//! that is shown (else at the profile choice, when the row's card is folded), and a refused typed value at its row
//! until a value is kept or the field closes; one bubble per row, the refused value first. While the field with a
//! refused text stays open, every edit is checked again after the typing grace time.

use std::collections::HashMap;

use rustcut_core::render::ProfileField;

use super::RenderSettingsBridge;
use super::state::Row;
use super::view::PanelView;
use crate::ui::notices::{self, Area, Kind, Level, Notice, TYPING};

/// The profile choice: where an issue stands when no row of its field is shown.
const PROFILE: &str = "codecs:profile";

/// The longest typed text a bubble's title quotes.
const QUOTED: usize = 24;

#[derive(Default)]
pub(super) struct CodecBubbles {
    /// Rows whose field keeps a refused text, with that text.
    refused: HashMap<Row, String>,
    /// The keys of the issue bubbles posted last.
    issues: Vec<String>,
}

fn anchor(row: Row) -> String {
    format!("codecs:{}", row.key())
}

fn value_key(row: Row) -> String {
    format!("codecs-value:{}", row.key())
}

impl RenderSettingsBridge {
    /// The issues of `panel` at their rows (after every publish).
    pub(super) fn publish_bubbles(&self, panel: &PanelView) {
        let mut groups: Vec<(ProfileField, Vec<&super::view::IssueView>)> = Vec::new();
        for issue in &panel.issues {
            match groups.iter_mut().find(|(field, _)| *field == issue.field) {
                Some((_, list)) => list.push(issue),
                None => groups.push((issue.field, vec![issue])),
            }
        }
        let mut posted = Vec::new();
        {
            let bubbles = self.bubbles.borrow();
            for (field, issues) in groups {
                let Some(first) = issues.first() else { continue };
                let anchors: Vec<String> = panel
                    .video
                    .iter()
                    .chain(&panel.audio)
                    .chain(&panel.expert)
                    .map(|r| r.row)
                    .filter(|row| row.field() == Some(field) && !bubbles.refused.contains_key(row))
                    .map(anchor)
                    .chain([PROFILE.to_owned()])
                    .collect();
                let key = format!("codecs-issue:{field:?}");
                let level = if issues.iter().any(|i| i.error) {
                    Level::Error
                } else {
                    Level::Warning
                };
                let rest: Vec<&str> = issues.iter().skip(1).map(|i| i.text.as_str()).collect();
                notices::post(
                    Notice::new(Kind::ProfileIssue, first.text.as_str())
                        .level(level)
                        .detail(rest.join("\n"))
                        .key(key.clone())
                        .at(Area::Codecs, anchors),
                );
                posted.push(key);
            }
        }
        let before = std::mem::replace(&mut self.bubbles.borrow_mut().issues, posted.clone());
        for gone in before.iter().filter(|k| !posted.contains(k)) {
            notices::clear(gone);
        }
    }

    /// A typed text of `row` was refused: its bubble says what the row takes.
    pub(super) fn text_refused(&self, row: Row, text: &str) {
        self.bubbles.borrow_mut().refused.insert(row, text.to_owned());
        self.post_refused(row, text, std::time::Duration::ZERO);
        self.publish();
    }

    /// The field of `row` with a refused text was typed in: checked again after the grace time.
    pub(super) fn text_typed(&self, row: Row, text: &str) {
        if !self.bubbles.borrow().refused.contains_key(&row) {
            return;
        }
        if self.state.borrow().check_text(row, text).is_ok() {
            self.bubbles.borrow_mut().refused.remove(&row);
            notices::clear_soon(&value_key(row), TYPING);
            self.publish();
        } else {
            self.bubbles.borrow_mut().refused.insert(row, text.to_owned());
            self.post_refused(row, text, TYPING);
        }
    }

    /// The field of `row` closed (a value was kept, or the refused text was dropped).
    pub(super) fn text_done(&self, row: Row) {
        if self.bubbles.borrow_mut().refused.remove(&row).is_some() {
            notices::clear(&value_key(row));
            self.publish();
        }
    }

    fn post_refused(&self, row: Row, text: &str, grace: std::time::Duration) {
        let expected = self.state.borrow().expected(row);
        let shown: String = text.trim().chars().take(QUOTED).collect();
        let title = if shown.is_empty() {
            "A value is needed here".to_owned()
        } else if shown.len() < text.trim().len() {
            format!("\"{shown}...\" is not a value here")
        } else {
            format!("\"{shown}\" is not a value here")
        };
        notices::post(
            Notice::new(Kind::ValueInvalid, title)
                .detail(expected)
                .key(value_key(row))
                .at(Area::Codecs, [anchor(row)])
                .grace(grace),
        );
    }
}
