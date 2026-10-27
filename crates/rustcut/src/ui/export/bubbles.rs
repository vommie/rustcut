//! The card's check as a bubble (M6.5c, user decision Q-N10): at the name field or the folder choice instead of a line
//! under the file name, after the grace time of typing; a conflict and similar names lead to the target folder's files
//! (the conflict dialog to look, `browse.rs`). A conflict also offers one icon button that solves it (user decision
//! Q-P8b): the next free number when the name has a numbering scheme, else " (n)" like a file manager. A bubble is
//! posted only when it changes, so the card's many refreshes never restart its grace time.

use std::path::Path;
use std::rc::Weak;

use rustcut_core::media::is_video_file;
use rustcut_core::naming::{FolderScan, QueuedTarget, Taken, next_free_counter_except, series};

use super::ExportBridge;
use super::check::{At, CardCheck, Level as CheckLevel};
use super::state::{ExportState, JobFacts};
use crate::icons::Icon;
use crate::ui::notices::{self, Area, Kind, Level, Notice, TYPING};

/// The anchors' keys (`BubbleAnchor` in `export_card.slint`).
const NAME: &str = "export.name";
const FOLDER: &str = "export.folder";

/// What the check's bubble shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckBubble {
    at: At,
    level: CheckLevel,
    title: String,
    detail: String,
    browse: Option<&'static str>,
    fix: Option<Fix>,
}

/// The way out of a conflict the bubble offers as a button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// The counter on, at this number (its file name is free).
    Number(u32),
    /// The typed name with " (n)" (its file name is free).
    Variant(String),
}

/// The bubble's way out of a conflict: the next free number (the conflict dialog's rule, Q-X7) when the template uses
/// the counter and the counter is on or the folder holds numbered videos of the name; else the typed name with the
/// first free " (n)" (the template makes the file name from it).
pub fn conflict_fix(
    st: &ExportState,
    facts: &JobFacts,
    folder: &Path,
    scan: &FolderScan,
    queue: &[QueuedTarget],
) -> Option<Fix> {
    let taken = Taken { folder, scan, queue };
    if st.template_uses_counter() {
        let numbered = series(scan.names.iter().filter(|n| is_video_file(n)), |n| {
            st.stem_with(facts, Some(n)).ok()
        });
        if st.active_counter().is_some() || !numbered.is_empty() {
            let used: Vec<u32> = numbered.iter().map(|f| f.counter).collect();
            let start = st.active_counter().unwrap_or(0);
            return next_free_counter_except(start, facts.container, &taken, &used, |n| st.stem_with(facts, Some(n)))
                .map(|(n, _)| Fix::Number(n));
        }
    }
    let base = st.name.trim();
    (1..1_000).find_map(|n| {
        let mut probe = st.clone();
        probe.name = format!("{base} ({n})");
        let (stem, _) = probe.file(facts).ok()?;
        taken
            .stem_free(&stem, facts.container)
            .then_some(Fix::Variant(probe.name))
    })
}

const fn key(at: At) -> &'static str {
    match at {
        At::Name => NAME,
        At::Folder => FOLDER,
    }
}

impl ExportBridge {
    /// Shows the check `line` as its bubble (or none) with `fix` as its button; a check that is still reading the folder
    /// changes nothing.
    pub(super) fn show_check(&self, line: &CardCheck, fix: Option<Fix>) {
        if line.level == CheckLevel::Hint && line.at.is_none() {
            return;
        }
        let wanted = line.at.map(|at| CheckBubble {
            at,
            level: line.level,
            title: line.text.clone(),
            detail: line.detail.clone(),
            browse: line.browse,
            fix: fix.filter(|_| line.level == CheckLevel::Conflict),
        });
        if *self.shown_check.borrow() == wanted {
            return;
        }
        let before = self.shown_check.replace(wanted.clone());
        if let Some(before) = before
            && wanted.as_ref().is_none_or(|w| w.at != before.at)
        {
            notices::clear_soon(key(before.at), TYPING);
        }
        let Some(bubble) = wanted else { return };
        let (kind, level) = match bubble.level {
            CheckLevel::Error => (Kind::TargetProblem, Level::Error),
            CheckLevel::Conflict => (Kind::TargetConflict, Level::Warning),
            CheckLevel::Hint | CheckLevel::Free => (Kind::TargetSimilar, Level::Info),
        };
        let mut notice = Notice::new(kind, bubble.title)
            .level(level)
            .detail(bubble.detail)
            .at(Area::Export, [key(bubble.at)])
            .grace(TYPING);
        if let Some(link) = bubble.browse {
            let me = Weak::clone(&self.me);
            notice = notice.action(link, move || {
                if let Some(bridge) = me.upgrade() {
                    bridge.browse_target();
                }
            });
        }
        let me = Weak::clone(&self.me);
        notice = match bubble.fix {
            Some(Fix::Number(n)) => notice.icon_action(
                Icon::CounterNextFree,
                format!("Take the next free number: {n:02}"),
                move || {
                    if let Some(bridge) = me.upgrade() {
                        bridge.take_fix(Fix::Number(n));
                    }
                },
            ),
            Some(Fix::Variant(name)) => notice.icon_action(
                Icon::NameVariant,
                format!("Add a free number in brackets: \"{name}\""),
                move || {
                    if let Some(bridge) = me.upgrade() {
                        bridge.take_fix(Fix::Variant(name.clone()));
                    }
                },
            ),
            None => notice,
        };
        notices::post(notice);
    }

    /// A button of the conflict bubble: the card takes the free number or name (its check then clears the bubble).
    fn take_fix(&self, fix: Fix) {
        tracing::info!(?fix, "[Export] The name check's way out was taken");
        {
            let mut st = self.state.borrow_mut();
            match fix {
                Fix::Number(n) => {
                    st.counter_enabled = true;
                    st.set_counter(n);
                }
                Fix::Variant(name) => st.name = name,
            }
        }
        self.publish();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::render::builtin_profiles;
    use rustcut_core::settings::{ExportPanelState, ExportSettings};
    use rustcut_core::time::Timestamp;

    use super::*;

    fn card(counter: bool) -> (ExportState, JobFacts) {
        let panel = ExportPanelState {
            counter_enabled: counter,
            counter: 4,
            auto_increment: counter,
            ..ExportPanelState::default()
        };
        let mut st = ExportState::new(&ExportSettings::default(), &panel);
        st.file_opened(Path::new("/in/Holiday.mp4"));
        st.set_counter(4);
        (
            st,
            JobFacts::new(&builtin_profiles().remove(0), 1, Timestamp::from_secs(9)),
        )
    }

    fn scan(names: &[&str]) -> FolderScan {
        FolderScan {
            readable: true,
            missing: false,
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    #[test]
    fn a_numbered_name_takes_the_next_free_number() {
        // User decision Q-P8b: 01, 02 and 04 exist, the counter stands at 04 after a save of 03.
        let folder = PathBuf::from("/out");
        let files = scan(&["Holiday - 01.mkv", "Holiday - 02.mp4", "Holiday - 04.mkv"]);
        let (st, facts) = card(true);
        assert_eq!(conflict_fix(&st, &facts, &folder, &files, &[]), Some(Fix::Number(5)));
        // The counter off, but the folder holds the series: the counter is switched on at the first free number.
        let (st, facts) = card(false);
        let plain = scan(&["Holiday.mkv", "Holiday - 01.mkv"]);
        assert_eq!(conflict_fix(&st, &facts, &folder, &plain, &[]), Some(Fix::Number(2)));
    }

    #[test]
    fn a_name_without_numbers_gets_brackets() {
        let folder = PathBuf::from("/out");
        let (st, facts) = card(false);
        let files = scan(&["Holiday.mkv", "Holiday (1).mkv", "Other - 01.mkv"]);
        assert_eq!(
            conflict_fix(&st, &facts, &folder, &files, &[]),
            Some(Fix::Variant("Holiday (2)".into()))
        );
    }
}
