//! What the queue can do with the selected jobs: the context menu's entries (with the number of jobs each one reaches
//! when several are selected; PyCutRv bug 17: actions always use the selection, never just the row under the pointer)
//! and the jobs an action applies to. The panel's buttons use the same rules.

use rustcut_core::job::{JobId, JobState, JobStateKind};
use rustcut_core::queue::JobSummary;

use crate::icons::Icon;
use crate::ui::menu::{MenuAction, MenuItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    /// Load a waiting or held job into the editor ("Update job").
    Edit,
    /// Load a done job into the editor; saving makes a new job.
    LoadAsNew,
    Details,
    ShowLog,
    EditTags,
    Play,
    Reveal,
    Hold,
    Release,
    Retry,
    /// Render again with the profile of this id (`""` = the Codecs panel's current settings).
    RetryWith(String),
    Duplicate,
    MoveTop,
    MoveBottom,
    Cancel,
    Remove,
    RemoveFinished,
}

/// The id of "the Codecs panel's current settings" in the "Render Again With" submenu.
pub const CURRENT_SETTINGS: &str = "";

impl QueueAction {
    pub fn id(&self) -> String {
        match self {
            Self::Edit => "edit".into(),
            Self::LoadAsNew => "load_new".into(),
            Self::Details => "details".into(),
            Self::ShowLog => "log".into(),
            Self::EditTags => "tags".into(),
            Self::Play => "play".into(),
            Self::Reveal => "reveal".into(),
            Self::Hold => "hold".into(),
            Self::Release => "release".into(),
            Self::Retry => "retry".into(),
            Self::RetryWith(profile) => format!("retry_with:{profile}"),
            Self::Duplicate => "duplicate".into(),
            Self::MoveTop => "top".into(),
            Self::MoveBottom => "bottom".into(),
            Self::Cancel => "cancel".into(),
            Self::Remove => "remove".into(),
            Self::RemoveFinished => "remove_finished".into(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        if let Some(profile) = id.strip_prefix("retry_with:") {
            return Some(Self::RetryWith(profile.to_owned()));
        }
        Some(match id {
            "edit" => Self::Edit,
            "load_new" => Self::LoadAsNew,
            "details" => Self::Details,
            "log" => Self::ShowLog,
            "tags" => Self::EditTags,
            "play" => Self::Play,
            "reveal" => Self::Reveal,
            "hold" => Self::Hold,
            "release" => Self::Release,
            "retry" => Self::Retry,
            "duplicate" => Self::Duplicate,
            "top" => Self::MoveTop,
            "bottom" => Self::MoveBottom,
            "cancel" => Self::Cancel,
            "remove" => Self::Remove,
            "remove_finished" => Self::RemoveFinished,
            _ => return None,
        })
    }

    /// Whether the action can do something to a job in `state`.
    pub fn applies_to(&self, state: &JobState) -> bool {
        let kind = state.kind();
        match self {
            Self::Edit => matches!(kind, JobStateKind::Waiting | JobStateKind::Held),
            Self::LoadAsNew | Self::Retry | Self::RetryWith(_) | Self::ShowLog => kind.is_done(),
            Self::Play | Self::RemoveFinished => kind == JobStateKind::Finished,
            Self::Hold => kind == JobStateKind::Waiting,
            Self::Release => kind == JobStateKind::Held,
            Self::Cancel => !kind.is_done(),
            Self::Remove => !kind.is_active(),
            Self::Details | Self::EditTags | Self::Reveal | Self::Duplicate | Self::MoveTop | Self::MoveBottom => true,
        }
    }

    /// Only for one job at a time.
    const fn single(&self) -> bool {
        matches!(
            self,
            Self::Edit | Self::LoadAsNew | Self::Details | Self::ShowLog | Self::EditTags | Self::Play
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

/// The key labels of the queue: the menu shows the first key next to its entries (`move_top`, `move_bottom`), the
/// tooltips of the move buttons every key (`move_up`, `move_down`).
#[derive(Debug, Clone, Default)]
pub struct MenuKeys {
    pub move_up: String,
    pub move_down: String,
    pub move_top: String,
    pub move_bottom: String,
}

/// The context menu for `selected`; `finished` is the number of finished jobs in the whole queue, `profiles` the
/// render profiles (id, name) for "Render Again With".
pub fn queue_menu(
    selected: &[&JobSummary],
    finished: usize,
    profiles: &[(String, String)],
    keys: &MenuKeys,
) -> Vec<MenuItem> {
    let n = selected.len();
    let entry = |action: QueueAction, title: &str, icon: Icon| {
        let count = targets(&action, selected).len();
        MenuAction::new(action.id(), counted(title, count, n), Some(icon)).enabled(count > 0)
    };
    let done_one = n == 1 && selected.first().is_some_and(|r| r.state.kind().is_done());
    let mut items: Vec<MenuItem> = vec![
        if done_one {
            entry(
                QueueAction::LoadAsNew,
                "Load into the Editor as a New Job",
                Icon::JobLoadNew,
            )
        } else {
            entry(QueueAction::Edit, "Edit in the Editor", Icon::JobEdit)
                .tooltip("The job waits (held) while it is edited; saving updates it")
        }
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
        MenuItem::Submenu {
            title: counted("Render Again With", again, n),
            icon: Icon::JobRetryWith,
            enabled: again > 0,
            items: with,
        },
        entry(QueueAction::Duplicate, "Duplicate", Icon::JobDuplicate).into(),
        MenuItem::Separator,
        entry(QueueAction::MoveTop, "Move to Top", Icon::MoveTop)
            .shortcut(keys.move_top.as_str())
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
        .into(),
    ]);
    items
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
                MenuItem::Separator => None,
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
        let menu = queue_menu(&all, 5, &profiles, &MenuKeys::default());
        let t = titles(&menu);
        let get = |name: &str| {
            t.iter()
                .find(|(title, _)| title.starts_with(name))
                .cloned()
                .unwrap_or_else(|| panic!("{name} not in {t:?}"))
        };
        assert_eq!(
            get("Edit in"),
            ("Edit in the Editor".into(), false),
            "one job at a time"
        );
        assert_eq!(get("Hold"), ("Hold (1 job)".into(), true));
        assert_eq!(get("Release"), ("Release (1 job)".into(), true));
        assert_eq!(get("Render Again With"), ("Render Again With (1 job)".into(), true));
        assert_eq!(get("Cancel"), ("Cancel (3 jobs)".into(), true));
        assert_eq!(get("Remove ("), ("Remove (3 jobs)".into(), true), "not the running one");
        assert_eq!(get("Remove Finished"), ("Remove Finished Jobs (5)".into(), true));
        assert_eq!(targets(&QueueAction::Remove, &all), vec![JobId(1), JobId(2), JobId(3)]);

        let one = [&jobs[2]];
        let t = titles(&queue_menu(&one, 1, &profiles, &MenuKeys::default()));
        assert!(t.contains(&("Load into the Editor as a New Job".into(), true)));
        assert!(t.contains(&("Play Output".into(), true)));
        assert!(t.contains(&("Cancel".into(), false)));
        assert!(t.contains(&("Hold".into(), false)), "nothing to hold or release");
    }

    #[test]
    fn ids_round_trip() {
        for action in [
            QueueAction::Edit,
            QueueAction::RetryWith("my-hevc".into()),
            QueueAction::RetryWith(CURRENT_SETTINGS.into()),
            QueueAction::RemoveFinished,
        ] {
            assert_eq!(QueueAction::from_id(&action.id()), Some(action));
        }
        assert_eq!(QueueAction::from_id("nope"), None);
    }
}
