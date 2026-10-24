//! The panel's texts: RustView's connection state in the header (the one place it is shown, ADR-6), the line that
//! sums up the job's tags and rating, and what the tree area says while it has no rows.

use std::fmt::Write as _;

use rustcut_core::rustview::ConnectionState;

use super::state::TagPanel;
use crate::ui::jump::JumpTarget;
use crate::ui::settings::page::Page;

/// The header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub text: String,
    pub tooltip: String,
    /// 0 = unknown, 1 = connected, 2 = unavailable, 3 = switched off (`TagRate.connection-kind`).
    pub kind: i32,
    pub can_retry: bool,
    /// The setting behind the state (a jump mark, Q-N6): the program while RustView cannot be reached, the switch while
    /// the link is off.
    pub jump: Option<JumpTarget>,
}

/// An option of Settings > RustView.
const fn rustview_option(key: &'static str) -> JumpTarget {
    JumpTarget::Settings {
        page: Page::RustView,
        key,
    }
}

pub fn connection(state: &ConnectionState) -> ConnectionView {
    const WRITES: &str = "Jobs keep their tags and rating and are written into RustView's library once it answers.";
    match state {
        ConnectionState::Unknown => ConnectionView {
            text: "Connecting to RustView...".into(),
            tooltip: "RustCut asks RustView (`rustview api`) for its tags.".into(),
            kind: 0,
            can_retry: false,
            jump: None,
        },
        ConnectionState::Connected => ConnectionView {
            text: "RustView connected".into(),
            tooltip: "RustCut writes the tags and the rating of its jobs into RustView's library.".into(),
            kind: 1,
            can_retry: false,
            jump: None,
        },
        ConnectionState::Unavailable { reason, retry_in_secs } => ConnectionView {
            text: format!("RustView unavailable: {reason}"),
            tooltip: format!(
                "{reason}\nRustCut tries again by itself (the next try was {retry_in_secs} s after this failure, the pause \
                 doubles up to 2 minutes); Retry tries now.\n{WRITES}"
            ),
            kind: 2,
            can_retry: true,
            jump: Some(rustview_option("rustview.binary")),
        },
        ConnectionState::Disabled => ConnectionView {
            text: "RustView is switched off".into(),
            tooltip:
                "The RustView link is switched off in the settings (`[rustview] enabled`): jobs are not written into \
                      RustView's library."
                    .into(),
            kind: 3,
            can_retry: false,
            jump: Some(rustview_option("rustview.enabled")),
        },
    }
}

/// "2 tags, rating 4" (with the locked and hidden ones named when there are any).
pub fn summary(panel: &TagPanel) -> String {
    let choice = panel.choice();
    let tags = match choice.tags.len() {
        0 => "No tags".to_owned(),
        1 => "1 tag".to_owned(),
        n => format!("{n} tags"),
    };
    let rating = choice
        .rating
        .map_or_else(|| "no rating".to_owned(), |r| format!("rating {r}"));
    let mut text = format!("{tags}, {rating}");
    let locked = if panel.editing.is_some() { 0 } else { panel.locked.len() };
    if locked > 0 {
        let _ = write!(text, " ({locked} locked)");
    }
    let hidden = panel.assigned_in_hidden();
    if hidden > 0 {
        let _ = write!(text, ", {hidden} in hidden branches");
    }
    text
}

/// The last saved job's tags by name ("Beach, Anna").
pub fn tag_names(panel: &TagPanel, ids: &[u32]) -> String {
    ids.iter()
        .map(|id| panel.name(*id).map_or_else(|| format!("#{id}"), str::to_owned))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What the tree area says while it shows no rows.
pub fn empty_text(panel: &TagPanel, state: &ConnectionState) -> String {
    if !panel.has_tree() {
        return match state {
            // The card above says that RustView is off (one place per information).
            ConnectionState::Disabled => "No tags to choose. The rating is kept with the job.",
            ConnectionState::Unavailable { .. } => "RustView's tags appear here once it answers.",
            ConnectionState::Unknown | ConnectionState::Connected => "Loading RustView's tags...",
        }
        .to_owned();
    }
    if panel.tag_count() == 0 {
        return "RustView's library has no tags yet (tags are created in RustView).".to_owned();
    }
    if !panel.filter.trim().is_empty() {
        return "No tag matches the filter.".to_owned();
    }
    "Every branch is hidden: the hide button shows them again.".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustcut_core::settings::TagPanelState;

    #[test]
    fn connection_texts() {
        assert_eq!(connection(&ConnectionState::Connected).kind, 1);
        let down = connection(&ConnectionState::Unavailable {
            reason: "RustView cannot be started: not installed".into(),
            retry_in_secs: 4,
        });
        assert!(down.can_retry && down.text.ends_with("not installed") && down.tooltip.contains("4 s"));
        assert!(!connection(&ConnectionState::Disabled).can_retry);
    }

    #[test]
    fn summary_and_empty_texts() {
        let mut panel = TagPanel::new(&TagPanelState::default());
        assert_eq!(summary(&panel), "No tags, no rating");
        assert_eq!(
            empty_text(&panel, &ConnectionState::Unknown),
            "Loading RustView's tags..."
        );
        panel.set_tree(std::sync::Arc::new(rustcut_core::rustview::TagTree::default()));
        assert!(empty_text(&panel, &ConnectionState::Connected).contains("no tags yet"));
        panel.click_rating(4);
        assert_eq!(summary(&panel), "No tags, rating 4");
    }
}
