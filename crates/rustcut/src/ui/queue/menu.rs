//! What the queue can do with the selected jobs: the context menu's entries (with the number of jobs each one reaches
//! when several are selected; PyCutRv bug 17: actions always use the selection, never just the row under the pointer)
//! and the jobs an action applies to, and the menu of the list's empty space (P9.6). The panel's buttons use the same
//! rules.

use rustcut_core::job::{JobId, JobState, JobStateKind};
use rustcut_core::queue::JobSummary;

use crate::icons::Icon;
use crate::ui::menu::{MenuAction, MenuItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    /// Load the job into the editor as a copy: saving makes a new job, the job stays as it is (user decision Q-P8b).
    Edit,
    Details,
    ShowLog,
    EditTags,
    Play,
    Reveal,
    Hold,
    Release,
    Retry,
    /// Render again, held right away (it waits until released; P9.6).
    RetryHeld,
    /// Render again with the profile of this id (`""` = the Codecs panel's current settings).
    RetryWith(String),
    MoveTop,
    MoveUp,
    MoveDown,
    MoveBottom,
    Cancel,
    Remove,
    RemoveFinished,
    SelectAll,
    ClearSelection,
    /// Every filter and the search off (the empty space's menu while a filter hides jobs).
    ClearFilter,
}

/// The id of "the Codecs panel's current settings" in the "Render Again With" submenu.
pub const CURRENT_SETTINGS: &str = "";

impl QueueAction {
    pub fn id(&self) -> String {
        match self {
            Self::Edit => "edit".into(),
            Self::Details => "details".into(),
            Self::ShowLog => "log".into(),
            Self::EditTags => "tags".into(),
            Self::Play => "play".into(),
            Self::Reveal => "reveal".into(),
            Self::Hold => "hold".into(),
            Self::Release => "release".into(),
            Self::Retry => "retry".into(),
            Self::RetryHeld => "retry_held".into(),
            Self::RetryWith(profile) => format!("retry_with:{profile}"),
            Self::MoveTop => "top".into(),
            Self::MoveUp => "up".into(),
            Self::MoveDown => "down".into(),
            Self::MoveBottom => "bottom".into(),
            Self::Cancel => "cancel".into(),
            Self::Remove => "remove".into(),
            Self::RemoveFinished => "remove_finished".into(),
            Self::SelectAll => "select_all".into(),
            Self::ClearSelection => "clear_selection".into(),
            Self::ClearFilter => "clear_filter".into(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        if let Some(profile) = id.strip_prefix("retry_with:") {
            return Some(Self::RetryWith(profile.to_owned()));
        }
        Some(match id {
            "edit" => Self::Edit,
            "details" => Self::Details,
            "log" => Self::ShowLog,
            "tags" => Self::EditTags,
            "play" => Self::Play,
            "reveal" => Self::Reveal,
            "hold" => Self::Hold,
            "release" => Self::Release,
            "retry" => Self::Retry,
            "retry_held" => Self::RetryHeld,
            "top" => Self::MoveTop,
            "up" => Self::MoveUp,
            "down" => Self::MoveDown,
            "bottom" => Self::MoveBottom,
            "cancel" => Self::Cancel,
            "remove" => Self::Remove,
            "remove_finished" => Self::RemoveFinished,
            "select_all" => Self::SelectAll,
            "clear_selection" => Self::ClearSelection,
            "clear_filter" => Self::ClearFilter,
            _ => return None,
        })
    }

    /// Whether the action can do something to a job in `state`.
    pub fn applies_to(&self, state: &JobState) -> bool {
        let kind = state.kind();
        match self {
            Self::Retry | Self::RetryHeld | Self::RetryWith(_) | Self::ShowLog => kind.is_done(),
            Self::Play | Self::RemoveFinished => kind == JobStateKind::Finished,
            Self::Hold => kind == JobStateKind::Waiting,
            Self::Release => kind == JobStateKind::Held,
            Self::Cancel => !kind.is_done(),
            Self::Remove => !kind.is_active(),
            Self::Edit
            | Self::Details
            | Self::EditTags
            | Self::Reveal
            | Self::MoveTop
            | Self::MoveUp
            | Self::MoveDown
            | Self::MoveBottom
            | Self::SelectAll
            | Self::ClearSelection
            | Self::ClearFilter => true,
        }
    }

    /// Only for one job at a time.
    const fn single(&self) -> bool {
        matches!(
            self,
            Self::Edit | Self::Details | Self::ShowLog | Self::EditTags | Self::Play
        )
    }
}

/// The jobs of `selected` that `action` applies to (in their order).
pub fn targets(action: &QueueAction, selected: &[&JobSummary]) -> Vec<JobId> {
    if action.single() && selected.len() != 1 {
        return Vec::new();
    }
    selected
        .iter()
        .filter(|r| action.applies_to(&r.state))
        .map(|r| r.id)
        .collect()
}

/// "Hold" for one job, "Hold (3 jobs)" for several.
pub fn counted(title: &str, count: usize, selected: usize) -> String {
    match (selected, count) {
        (0 | 1, _) | (_, 0) => title.to_owned(),
        (_, 1) => format!("{title} (1 job)"),
        (_, n) => format!("{title} ({n} jobs)"),
    }
}

/// The key labels of the queue: the menu shows the first key next to its entries (`move_top`, `menu_up`,
/// `menu_down`, `move_bottom`, `select_all`), the tooltips of the move buttons every key (`move_up`, `move_down`).
#[derive(Debug, Clone, Default)]
pub struct MenuKeys {
    pub move_up: String,
    pub move_down: String,
    pub move_top: String,
    pub move_bottom: String,
    pub menu_up: String,
    pub menu_down: String,
    pub select_all: String,
}

/// What the menus need to know about the whole list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListFacts {
    /// Finished jobs in the whole queue.
    pub finished: usize,
    /// Jobs shown, and whether a filter hides some.
    pub shown: usize,
    pub filtered: bool,
    /// The selection can move one place up, down.
    pub can_up: bool,
    pub can_down: bool,
}

/// The context menu for `selected`; `profiles` are the render profiles (id, name) for "Render Again With".
pub fn queue_menu(
    selected: &[&JobSummary],
    facts: ListFacts,
    profiles: &[(String, String)],
    keys: &MenuKeys,
) -> Vec<MenuItem> {
    let n = selected.len();
    let entry = |action: QueueAction, title: &str, icon: Icon| {
        let count = targets(&action, selected).len();
        MenuAction::new(action.id(), counted(title, count, n), Some(icon)).enabled(count > 0)
    };
    let mut items: Vec<MenuItem> = vec![
        entry(QueueAction::Edit, "Load into the Editor", Icon::JobEdit)
            .tooltip("A copy: saving makes a new job, this one stays as it is")
            .into(),
        entry(QueueAction::Details, "Show in the Job Panel", Icon::JobDetails).into(),
        entry(QueueAction::ShowLog, "Show Log", Icon::PanelLogs).into(),
        entry(QueueAction::EditTags, "Edit Tags and Rating", Icon::JobTags).into(),
        MenuItem::Separator,
        entry(QueueAction::Play, "Play Output", Icon::PlayFile).into(),
        entry(QueueAction::Reveal, "Show in Folder", Icon::RevealFile).into(),
        MenuItem::Separator,
    ];
    let holds = targets(&QueueAction::Hold, selected).len();
    let releases = targets(&QueueAction::Release, selected).len();
    if holds > 0 || releases == 0 {
        items.push(entry(QueueAction::Hold, "Hold", Icon::JobHold).into());
    }
    if releases > 0 {
        items.push(entry(QueueAction::Release, "Release", Icon::QueueResume).into());
    }
    let again = targets(&QueueAction::Retry, selected).len();
    let mut with: Vec<MenuItem> = vec![
        MenuAction::new(
            QueueAction::RetryWith(CURRENT_SETTINGS.into()).id(),
            "The Codecs Panel's Settings",
            Some(Icon::PanelRenderSettings),
        )
        .into(),
        MenuItem::Separator,
    ];
    with.extend(
        profiles
            .iter()
            .map(|(id, name)| MenuAction::new(QueueAction::RetryWith(id.clone()).id(), name.as_str(), None).into()),
    );
    items.extend([
        entry(QueueAction::Retry, "Render Again", Icon::JobRetry).into(),
        entry(QueueAction::RetryHeld, "Render Again (Hold)", Icon::JobHold)
            .tooltip("Renders it again, held first: it waits until you release it")
            .into(),
        MenuItem::Submenu {
            title: counted("Render Again With", again, n),
            icon: Icon::JobRetryWith,
            enabled: again > 0,
            items: with,
        },
        MenuItem::Separator,
        entry(QueueAction::MoveTop, "Move to Top", Icon::MoveTop)
            .shortcut(keys.move_top.as_str())
            .into(),
        MenuAction::new(QueueAction::MoveUp.id(), "Move Up", Some(Icon::MoveUp))
            .enabled(facts.can_up)
            .shortcut(keys.menu_up.as_str())
            .into(),
        MenuAction::new(QueueAction::MoveDown.id(), "Move Down", Some(Icon::MoveDown))
            .enabled(facts.can_down)
            .shortcut(keys.menu_down.as_str())
            .into(),
        entry(QueueAction::MoveBottom, "Move to Bottom", Icon::MoveBottom)
            .shortcut(keys.move_bottom.as_str())
            .into(),
        MenuItem::Separator,
        entry(QueueAction::Cancel, "Cancel", Icon::JobCancel)
            .hold()
            .tooltip("Cancel: a running render stops, an existing target stays")
            .into(),
        entry(QueueAction::Remove, "Remove", Icon::JobRemove)
            .hold()
            .tooltip("Remove from the queue (output and source files stay)")
            .into(),
        remove_finished(facts.finished),
        MenuItem::Separator,
        MenuAction::new(QueueAction::SelectAll.id(), "Select All", Some(Icon::SelectAll))
            .enabled(n < facts.shown)
            .shortcut(keys.select_all.as_str())
            .into(),
        MenuAction::new(
            QueueAction::ClearSelection.id(),
            "Clear Selection",
            Some(Icon::SelectionClear),
        )
        .enabled(n > 0)
        .into(),
    ]);
    items
}

/// The menu of the list's empty space: what applies to every job.
pub fn list_menu(facts: ListFacts, keys: &MenuKeys) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = vec![
        MenuAction::new(QueueAction::SelectAll.id(), "Select All", Some(Icon::SelectAll))
            .enabled(facts.shown > 0)
            .shortcut(keys.select_all.as_str())
            .into(),
    ];
    if facts.filtered {
        items.push(
            MenuAction::new(QueueAction::ClearFilter.id(), "Show Every Job", Some(Icon::FilterClear))
                .tooltip("Clears the filters and the search")
                .into(),
        );
    }
    items.extend([MenuItem::Separator, remove_finished(facts.finished)]);
    items
}

fn remove_finished(finished: usize) -> MenuItem {
    MenuAction::new(
        QueueAction::RemoveFinished.id(),
        match finished {
            0 => "Remove Finished Jobs".to_owned(),
            n => format!("Remove Finished Jobs ({n})"),
        },
        Some(Icon::RemoveFinished),
    )
    .enabled(finished > 0)
    .hold()
    .tooltip("Remove every finished job from the queue (their files stay)")
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::queue::filter::tests::row;

    fn titles(items: &[MenuItem]) -> Vec<(String, bool)> {
        items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action(a) => Some((a.title.clone(), a.enabled)),
                MenuItem::Submenu { title, enabled, .. } => Some((title.clone(), *enabled)),
                MenuItem::Separator | MenuItem::Heading(_) => None,
            })
            .collect()
    }

    #[test]
    fn counts_follow_the_selection() {
        let jobs = [
            row(1, JobState::Waiting, "a"),
            row(2, JobState::Held, "b"),
            row(3, JobState::Finished, "c"),
            row(4, JobState::Rendering { pass: 1, passes: 1 }, "d"),
        ];
        let all: Vec<&JobSummary> = jobs.iter().collect();
        let profiles = vec![("x265".to_owned(), "x265 quality".to_owned())];
        let facts = ListFacts {
            finished: 5,
            shown: 4,
            filtered: false,
            can_up: false,
            can_down: false,
        };
        let menu = queue_menu(&all, facts, &profiles, &MenuKeys::default());
        let t = titles(&menu);
        let get = |name: &str| {
            t.iter()
                .find(|(title, _)| title.starts_with(name))
                .cloned()
                .unwrap_or_else(|| panic!("{name} not in {t:?}"))
        };
        assert_eq!(
            get("Load into"),
            ("Load into the Editor".into(), false),
            "one job at a time"
        );
        assert_eq!(get("Hold"), ("Hold (1 job)".into(), true));
        assert_eq!(get("Release"), ("Release (1 job)".into(), true));
        assert_eq!(get("Render Again With"), ("Render Again With (1 job)".into(), true));
        assert_eq!(get("Cancel"), ("Cancel (3 jobs)".into(), true));
        assert_eq!(get("Remove ("), ("Remove (3 jobs)".into(), true), "not the running one");
        assert_eq!(get("Remove Finished"), ("Remove Finished Jobs (5)".into(), true));
        assert_eq!(get("Render Again (Hold)"), ("Render Again (Hold) (1 job)".into(), true));
        assert_eq!(get("Move Up"), ("Move Up".into(), false));
        assert_eq!(
            get("Select All"),
            ("Select All".into(), false),
            "every shown job is selected"
        );
        assert_eq!(get("Clear Selection"), ("Clear Selection".into(), true));
        assert_eq!(targets(&QueueAction::Remove, &all), vec![JobId(1), JobId(2), JobId(3)]);

        let one = [&jobs[2]];
        let facts = ListFacts {
            finished: 1,
            can_up: true,
            ..facts
        };
        let t = titles(&queue_menu(&one, facts, &profiles, &MenuKeys::default()));
        assert!(t.contains(&("Move Up".into(), true)) && t.contains(&("Select All".into(), true)));
        assert!(
            t.contains(&("Load into the Editor".into(), true)),
            "a finished job too (Q-P8b)"
        );
        assert!(t.contains(&("Play Output".into(), true)));
        assert!(t.contains(&("Cancel".into(), false)));
        assert!(t.contains(&("Hold".into(), false)), "nothing to hold or release");
    }

    #[test]
    fn the_empty_space_menu() {
        let facts = ListFacts {
            finished: 0,
            shown: 3,
            filtered: true,
            ..ListFacts::default()
        };
        assert_eq!(
            titles(&list_menu(facts, &MenuKeys::default())),
            [
                ("Select All".into(), true),
                ("Show Every Job".into(), true),
                ("Remove Finished Jobs".into(), false),
            ]
        );
        let plain = titles(&list_menu(ListFacts::default(), &MenuKeys::default()));
        assert_eq!(plain.len(), 2, "no filter, nothing to show again");
    }

    #[test]
    fn ids_round_trip() {
        for action in [
            QueueAction::Edit,
            QueueAction::RetryHeld,
            QueueAction::MoveUp,
            QueueAction::ClearSelection,
            QueueAction::RetryWith("my-hevc".into()),
            QueueAction::RetryWith(CURRENT_SETTINGS.into()),
            QueueAction::RemoveFinished,
        ] {
            assert_eq!(QueueAction::from_id(&action.id()), Some(action));
        }
        assert_eq!(QueueAction::from_id("nope"), None);
    }
}
