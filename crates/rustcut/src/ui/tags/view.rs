//! The panel's texts: RustView's connection state in the tree's place (the one place it is shown, ADR-6), the eye's
//! tooltip and what the tree area says while it has no rows.

use rustcut_core::rustview::ConnectionState;

use super::state::TagPanel;
use crate::ui::jump::JumpTarget;
use crate::ui::settings::page::Page;

/// The card in the tree's place while RustView is unavailable or switched off (connecting and connected show none): one
/// short line (P9.8, review note 28#1); the error is the tooltip of the button that leads to the setting behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub text: String,
    /// The current error in short: the jump button's tooltip.
    pub error: String,
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
    match state {
        ConnectionState::Unknown => ConnectionView {
            text: "Connecting to RustView...".into(),
            error: String::new(),
            kind: 0,
            can_retry: false,
            jump: None,
        },
        ConnectionState::Connected => ConnectionView {
            text: "RustView connected".into(),
            error: String::new(),
            kind: 1,
            can_retry: false,
            jump: None,
        },
        ConnectionState::Unavailable { reason, .. } => ConnectionView {
            text: "RustView unavailable".into(),
            error: reason.clone(),
            kind: 2,
            can_retry: true,
            jump: Some(rustview_option("rustview.binary")),
        },
        ConnectionState::Disabled => ConnectionView {
            text: "RustView is switched off".into(),
            error: "The RustView link is switched off in the settings.".into(),
            kind: 3,
            can_retry: false,
            jump: Some(rustview_option("rustview.enabled")),
        },
    }
}

/// The eye's tooltip (P9.8: it only shows or hides the hidden tags; they are chosen in the tag filter window).
pub fn eye_tooltip(panel: &TagPanel) -> String {
    if panel.show_hidden {
        return "Hide the hidden tags again".to_owned();
    }
    let hidden = panel.hidden_count();
    if hidden == 0 {
        return "No tag is hidden (the tag filter hides tags)".to_owned();
    }
    let tags = if hidden == 1 { "tag" } else { "tags" };
    match panel.assigned_in_hidden() {
        0 => format!("Show the {hidden} hidden {tags}"),
        assigned => format!("Show the {hidden} hidden {tags} ({assigned} of them assigned)"),
    }
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
    "Every tag is hidden: the eye button shows them.".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustcut_core::rustview::{TagItem, TagTree};
    use rustcut_core::settings::TagPanelState;

    #[test]
    fn connection_texts() {
        assert_eq!(connection(&ConnectionState::Connected).kind, 1);
        let down = connection(&ConnectionState::Unavailable {
            reason: "RustView cannot be started: not installed".into(),
            retry_in_secs: 4,
        });
        assert!(down.can_retry && down.text == "RustView unavailable");
        assert_eq!(
            down.error, "RustView cannot be started: not installed",
            "the error is the button's tooltip"
        );
        assert!(!connection(&ConnectionState::Disabled).can_retry);
    }

    #[test]
    fn eye_and_empty_texts() {
        let mut panel = TagPanel::new(&TagPanelState::default());
        assert_eq!(
            empty_text(&panel, &ConnectionState::Unknown),
            "Loading RustView's tags..."
        );
        panel.set_tree(std::sync::Arc::new(TagTree::default()));
        assert!(empty_text(&panel, &ConnectionState::Connected).contains("no tags yet"));

        let tags = [(1, None, "People"), (2, Some(1), "Anna"), (3, None, "Travel")]
            .into_iter()
            .map(|(id, parent_id, name)| TagItem {
                id,
                parent_id,
                name: name.into(),
            })
            .collect();
        panel.set_tree(std::sync::Arc::new(TagTree {
            success: true,
            total_tags: 3,
            tags,
        }));
        assert_eq!(eye_tooltip(&panel), "No tag is hidden (the tag filter hides tags)");
        panel.toggle_branch(1);
        assert_eq!(eye_tooltip(&panel), "Show the 2 hidden tags");
        panel.click_tag(2);
        assert_eq!(eye_tooltip(&panel), "Show the 2 hidden tags (1 of them assigned)");
        panel.toggle_show_hidden();
        assert_eq!(eye_tooltip(&panel), "Hide the hidden tags again");
        panel.toggle_branch(3);
        panel.toggle_show_hidden();
        assert_eq!(
            empty_text(&panel, &ConnectionState::Connected),
            "Every tag is hidden: the eye button shows them."
        );
    }
}
