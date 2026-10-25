//! The Sections card's context menus (P9.5, the review's note 07): one for the selected sections (a right click on a
//! row selects it first when it is not selected; the entries always act on the whole selection, PyCutRv bug 17) and
//! one for the empty space below the rows (what applies to every section). Deleting is held (user decision Q-Q4).

use crate::icons::Icon;
use crate::ui::menu::{MenuAction, MenuItem};

use super::SectionsState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionAction {
    Duplicate,
    MoveUp,
    MoveDown,
    Delete,
    SelectAll,
    ClearSelection,
    /// Delete every section.
    ClearAll,
}

impl SectionAction {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::MoveUp => "up",
            Self::MoveDown => "down",
            Self::Delete => "delete",
            Self::SelectAll => "select_all",
            Self::ClearSelection => "clear_selection",
            Self::ClearAll => "clear_all",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [
            Self::Duplicate,
            Self::MoveUp,
            Self::MoveDown,
            Self::Delete,
            Self::SelectAll,
            Self::ClearSelection,
            Self::ClearAll,
        ]
        .into_iter()
        .find(|a| a.id() == id)
    }
}

/// "Duplicate" for one section, "Duplicate (3 sections)" for several.
fn counted(title: &str, selected: usize) -> String {
    match selected {
        0 | 1 => title.to_owned(),
        n => format!("{title} ({n} sections)"),
    }
}

/// The menu of the selected sections; `delete_key` is the key that deletes them at once (shown beside the entry).
pub fn selection_menu(sections: &SectionsState, delete_key: &str) -> Vec<MenuItem> {
    let n = sections.selected_ids().len();
    let entry = |action: SectionAction, title: String, icon: Icon, enabled: bool| {
        MenuAction::new(action.id(), title, Some(icon)).enabled(enabled)
    };
    vec![
        entry(
            SectionAction::Duplicate,
            counted("Duplicate", n),
            Icon::SectionDuplicate,
            n > 0,
        )
        .tooltip("Inserts a copy after the last selected section")
        .into(),
        MenuItem::Separator,
        entry(
            SectionAction::MoveUp,
            "Move Up".to_owned(),
            Icon::MoveUp,
            sections.can_move(true),
        )
        .into(),
        entry(
            SectionAction::MoveDown,
            "Move Down".to_owned(),
            Icon::MoveDown,
            sections.can_move(false),
        )
        .into(),
        MenuItem::Separator,
        entry(SectionAction::Delete, counted("Delete", n), Icon::SectionDelete, n > 0)
            .hold()
            .shortcut(delete_key)
            .into(),
        MenuItem::Separator,
        entry(
            SectionAction::SelectAll,
            "Select All".to_owned(),
            Icon::SelectAll,
            n < sections.list().len(),
        )
        .into(),
        entry(
            SectionAction::ClearSelection,
            "Clear Selection".to_owned(),
            Icon::SelectionClear,
            n > 0,
        )
        .into(),
    ]
}

/// The menu of the empty space below the rows.
pub fn list_menu(sections: &SectionsState) -> Vec<MenuItem> {
    let count = sections.list().len();
    vec![
        MenuAction::new(SectionAction::SelectAll.id(), "Select All", Some(Icon::SelectAll))
            .enabled(count > 0 && sections.selected_ids().len() < count)
            .into(),
        MenuItem::Separator,
        MenuAction::new(
            SectionAction::ClearAll.id(),
            match count {
                0 | 1 => "Clear All Sections".to_owned(),
                n => format!("Clear All Sections ({n})"),
            },
            Some(Icon::SectionsClearAll),
        )
        .enabled(count > 0)
        .hold()
        .tooltip("Deletes every section of the list (the marks stay)")
        .into(),
    ]
}

#[cfg(test)]
mod tests {
    use rustcut_core::time::Timestamp;

    use super::*;
    use crate::ui::sections::Bound;

    fn titles(items: &[MenuItem]) -> Vec<(String, bool, bool)> {
        items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action(a) => Some((a.title.clone(), a.enabled, a.hold)),
                _ => None,
            })
            .collect()
    }

    fn three() -> SectionsState {
        let mut s = SectionsState::default();
        s.set_duration(Some(Timestamp::from_secs(100)));
        for i in 0..3 {
            s.set_mark(Bound::Start, Timestamp::from_secs(i * 10));
            s.set_mark(Bound::End, Timestamp::from_secs(i * 10 + 5));
            s.add_current().unwrap();
        }
        s
    }

    #[test]
    fn entries_follow_the_selection() {
        let mut s = three();
        s.pick(0, false, false);
        let one = titles(&selection_menu(&s, "Del"));
        assert_eq!(
            one,
            [
                ("Duplicate".into(), true, false),
                ("Move Up".into(), false, false),
                ("Move Down".into(), true, false),
                ("Delete".into(), true, true),
                ("Select All".into(), true, false),
                ("Clear Selection".into(), true, false),
            ]
        );
        s.pick(2, false, true);
        let all = titles(&selection_menu(&s, "Del"));
        assert_eq!(all.first().map(|e| e.0.as_str()), Some("Duplicate (3 sections)"));
        assert!(!all.iter().any(|e| e.0.starts_with("Move") && e.1), "nothing can move");
        assert!(all.iter().any(|e| e.0 == "Select All" && !e.1));
        for action in [
            SectionAction::Duplicate,
            SectionAction::ClearAll,
            SectionAction::ClearSelection,
        ] {
            assert_eq!(SectionAction::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn the_empty_space_clears_everything_when_held() {
        let s = three();
        assert_eq!(
            titles(&list_menu(&s)),
            [
                ("Select All".into(), true, false),
                ("Clear All Sections (3)".into(), true, true),
            ]
        );
        assert!(titles(&list_menu(&SectionsState::default())).iter().all(|e| !e.1));
    }
}
