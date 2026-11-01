//! The menus of the control bar (pure, tested): the normalization's kinds, the current time's menu and the "..." menu with
//! what does not fit into the bar.

use rustcut_core::settings::Normalization;

use super::layout::Fit;
use crate::icons::Icon;
use crate::ui::menu::{MenuAction, MenuItem};

pub const OPEN_MPV: &str = "open_mpv";
pub const OPEN_SECTION: &str = "open_section";
pub const COPY_TIME: &str = "copy_time";
pub const GO_TO_TIME: &str = "go_to_time";
const KIND_PREFIX: &str = "normalization:";
pub const TOGGLE_POINTER: &str = "toggle_pointer";

/// The id of a kind's entry.
fn kind_id(kind: Normalization) -> String {
    format!("{KIND_PREFIX}{}", kind.title())
}

/// The kind an entry's id names.
pub fn picked_kind(id: &str) -> Option<Normalization> {
    let title = id.strip_prefix(KIND_PREFIX)?;
    std::iter::once(Normalization::None)
        .chain(Normalization::ACTIVE)
        .find(|k| k.title() == title)
}

const fn kind_icon(kind: Normalization) -> Icon {
    match kind {
        Normalization::None => Icon::NormalizeNone,
        Normalization::Loudness => Icon::NormalizeLoudness,
        Normalization::Dynamic => Icon::NormalizeDynamic,
        Normalization::Speech => Icon::NormalizeSpeech,
        Normalization::Compressor => Icon::NormalizeCompressor,
    }
}

fn kind_entry(kind: Normalization, current: Normalization) -> MenuItem {
    MenuAction::new(kind_id(kind), kind.title(), Some(kind_icon(kind)))
        .checked(kind == current)
        .tooltip(kind.explanation())
        .into()
}

/// "None", then the kinds; a check mark at the current one.
pub fn normalization(current: Normalization) -> Vec<MenuItem> {
    let mut items = vec![kind_entry(Normalization::None, current), MenuItem::Separator];
    items.extend(Normalization::ACTIVE.map(|k| kind_entry(k, current)));
    items
}

/// The menu of the current time. `in_section` = the time lies in a section or between the marks.
pub fn time(has_file: bool, in_section: bool) -> Vec<MenuItem> {
    vec![
        MenuAction::new(OPEN_MPV, "Open in mpv here", Some(Icon::OpenExternal))
            .enabled(has_file)
            .tooltip("Opens the video in your mpv, paused at this time")
            .into(),
        MenuAction::new(OPEN_SECTION, "Open section in mpv as AB-loop", Some(Icon::TimeSectionLoop))
            .enabled(has_file && in_section)
            .tooltip(if in_section {
                "Opens the section around this time in your mpv, paused at its start, repeating it"
            } else {
                "This time is not in a section"
            })
            .into(),
        MenuItem::Separator,
        MenuAction::new(COPY_TIME, "Copy time", Some(Icon::TimeCopy))
            .enabled(has_file)
            .into(),
        MenuAction::new(GO_TO_TIME, "Go to time...", Some(Icon::TimeGoTo))
            .enabled(has_file)
            .tooltip("Type a time and jump to it")
            .into(),
    ]
}

/// What the "..." menu shows: the state of the entries it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct More<'a> {
    pub has_video: bool,
    pub has_media: bool,
    pub can_add: bool,
    pub normalization: Normalization,
    pub hide_pointer: bool,
    /// The keys for the entries, in the order frame back, frame forward, start, end, add.
    pub keys: [&'a str; 5],
}

/// The entries the bar has no room for; their ids are keymap actions, except the two audio ones.
pub fn more(fit: Fit, state: &More<'_>) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let [back, forward, start, end, add] = state.keys;
    if !fit.frames {
        items.push(
            MenuAction::new("frame_back", "One frame back", Some(Icon::FrameBack))
                .enabled(state.has_video)
                .shortcut(back)
                .into(),
        );
        items.push(
            MenuAction::new("frame_forward", "One frame forward", Some(Icon::FrameForward))
                .enabled(state.has_video)
                .shortcut(forward)
                .into(),
        );
    }
    if !fit.sections {
        if !items.is_empty() {
            items.push(MenuItem::Separator);
        }
        items.push(
            MenuAction::new("set_start", "Set the section start here", Some(Icon::SectionMarkStart))
                .enabled(state.has_media)
                .shortcut(start)
                .into(),
        );
        items.push(
            MenuAction::new("set_end", "Set the section end here", Some(Icon::SectionMarkEnd))
                .enabled(state.has_media)
                .shortcut(end)
                .into(),
        );
        items.push(
            MenuAction::new("add_section", "Add the section", Some(Icon::SectionAdd))
                .enabled(state.can_add)
                .shortcut(add)
                .into(),
        );
    }
    if !fit.audio {
        if !items.is_empty() {
            items.push(MenuItem::Separator);
        }
        items.push(MenuItem::Submenu {
            title: format!("Audio normalization: {}", state.normalization.title()),
            icon: Icon::AudioNormalize,
            enabled: true,
            items: normalization(state.normalization),
        });
        items.push(
            MenuAction::new(TOGGLE_POINTER, "Hide the resting pointer", Some(Icon::PointerHide))
                .checked(state.hide_pointer)
                .into(),
        );
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: More<'static> = More {
        has_video: true,
        has_media: true,
        can_add: false,
        normalization: Normalization::Dynamic,
        hide_pointer: true,
        keys: ["a", "b", "c", "d", "e"],
    };

    fn ids(items: &[MenuItem]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action(a) => Some(a.id.as_str()),
                MenuItem::Submenu { .. } => Some("submenu"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_kind_is_found_again_from_its_entry() {
        for item in normalization(Normalization::Speech) {
            if let MenuItem::Action(a) = item {
                let kind = picked_kind(&a.id).unwrap();
                assert_eq!(a.title, kind.title());
                assert_eq!(a.checked, kind == Normalization::Speech);
                assert!(!a.tooltip.is_empty());
            }
        }
        assert_eq!(picked_kind("open_mpv"), None);
    }

    #[test]
    fn the_section_entry_needs_a_section() {
        let off = time(true, false);
        let MenuItem::Action(a) = off.get(1).unwrap() else { panic!() };
        assert!(!a.enabled);
        let on = time(true, true);
        let MenuItem::Action(a) = on.get(1).unwrap() else { panic!() };
        assert!(a.enabled);
    }

    #[test]
    fn the_more_menu_holds_only_what_is_hidden() {
        let base = crate::ui::control_bar::layout::fit(240.0, 64.0, 7.0);
        let all = more(base, &STATE);
        assert_eq!(
            ids(&all),
            ["frame_back", "frame_forward", "set_start", "set_end", "add_section", "submenu", TOGGLE_POINTER]
        );
        let sections_only = Fit {
            frames: true,
            audio: true,
            ..base
        };
        assert_eq!(ids(&more(sections_only, &STATE)), ["set_start", "set_end", "add_section"]);
    }
}
