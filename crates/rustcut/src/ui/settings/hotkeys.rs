//! The Hotkeys page (pure): every keymap action with its keys, the filter, and recording a new key. A key that another
//! action has is not taken at once: the page names that action, and pressing the same key again moves it (D-S3; no
//! question dialog, user decision Q-Q4). What is stored keeps only the actions whose keys differ from their defaults.

use slint::platform::Key;

use crate::ui::keymap::{Action, Chord, KeyPress, Keymap, KeymapTable};

/// One action of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyRow {
    pub action: Action,
    /// (stored text, label) per key.
    pub keys: Vec<(String, String)>,
    pub changed: bool,
    /// The default keys' labels, joined.
    pub defaults: String,
}

/// What a key press while recording did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// A modifier alone, or a key RustCut cannot name: still waiting.
    Waiting,
    Bound(String),
    /// The key belongs to `other`; the same key again moves it.
    Taken { chord: String, other: Action },
    Moved { chord: String, from: Action },
}

#[derive(Debug, Clone, Default)]
pub struct HotkeysPage {
    pub search: String,
    /// The action waiting for a key.
    pub recording: Option<Action>,
    /// The key that was refused for being another action's: pressing it again moves it.
    pending_move: Option<String>,
}

/// The group an action is listed under.
pub const fn group(action: Action) -> &'static str {
    match action {
        Action::Open | Action::Save | Action::Quit | Action::OpenSettings => "File",
        Action::TogglePause
        | Action::ShuttleReverse
        | Action::ShuttleStop
        | Action::ShuttleForward
        | Action::FrameBack
        | Action::FrameForward
        | Action::SeekBack
        | Action::SeekForward
        | Action::SeekBackLong
        | Action::SeekForwardLong
        | Action::ToggleMute
        | Action::Reload => "Playback",
        Action::PreviousCut
        | Action::NextCut
        | Action::SetStart
        | Action::SetEnd
        | Action::GoToStart
        | Action::GoToEnd
        | Action::AddSection
        | Action::DeleteSection => "Sections",
        Action::ZoomIn | Action::ZoomOut | Action::ZoomFit => "Timeline",
        Action::CropEdit
        | Action::AutoCrop
        | Action::ToggleFilterPreview
        | Action::ToggleKeepFilters
        | Action::RotateLeft
        | Action::RotateRight
        | Action::MirrorHorizontal
        | Action::MirrorVertical
        | Action::ResetFilters
        | Action::ProbeRender => "Filters",
        Action::QueueMoveUp
        | Action::QueueMoveDown
        | Action::QueueMoveTop
        | Action::QueueMoveBottom
        | Action::QueueSelectAll => "Queue",
    }
}

/// The parsed default keys of `action`.
fn default_chords(action: Action) -> Vec<Chord> {
    action.default_keys().iter().filter_map(|k| Chord::parse(k).ok()).collect()
}

fn texts(chords: &[Chord]) -> Vec<&str> {
    chords.iter().map(Chord::text).collect()
}

/// The rows that match `search` (title, group or a key; case-insensitive).
pub fn rows(keymap: &Keymap, search: &str) -> Vec<HotkeyRow> {
    let needle = search.trim().to_lowercase();
    Action::ALL
        .into_iter()
        .map(|action| {
            let chords = keymap.chords(action);
            let defaults = default_chords(action);
            HotkeyRow {
                action,
                keys: chords.iter().map(|c| (c.text().to_owned(), c.label())).collect(),
                changed: texts(chords) != texts(&defaults),
                defaults: defaults.iter().map(Chord::label).collect::<Vec<_>>().join(", "),
            }
        })
        .filter(|row| {
            needle.is_empty()
                || row.action.title().to_lowercase().contains(&needle)
                || group(row.action).to_lowercase().contains(&needle)
                || row.keys.iter().any(|(_, label)| label.to_lowercase().contains(&needle))
        })
        .collect()
}

/// The table for `config.toml`: only the actions whose keys differ from their defaults.
pub fn table(keymap: &Keymap) -> KeymapTable {
    let full = keymap.to_table();
    full.into_iter()
        .filter(|(id, _)| {
            Action::from_id(id).is_some_and(|a| texts(keymap.chords(a)) != texts(&default_chords(a)))
        })
        .collect()
}

/// Slint's names of the keys that are not a plain character.
const NAMED_KEYS: [&str; 46] = [
    "LeftArrow", "RightArrow", "UpArrow", "DownArrow", "Home", "End", "PageUp", "PageDown", "Insert", "Delete",
    "Backspace", "Return", "Tab", "Space", "Plus", "HyphenMinus", "Comma", "Period", "Equals", "ScrollLock", "Pause",
    "SysReq", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16",
    "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

/// The chord a key press makes, written with Slint's key names; `None` for a modifier alone or a key without a name.
pub fn chord_of(press: &KeyPress<'_>) -> Option<Chord> {
    let modifier_keys = [
        Key::Shift,
        Key::ShiftR,
        Key::Control,
        Key::ControlR,
        Key::Alt,
        Key::AltGr,
        Key::Meta,
        Key::MetaR,
        Key::CapsLock,
    ];
    if press.text.is_empty() || modifier_keys.iter().any(|k| press.text == slint::SharedString::from(*k).as_str()) {
        return None;
    }
    let mut prefix = String::new();
    for (on, name) in [
        (press.control, "Control+"),
        (press.alt, "Alt+"),
        (press.shift, "Shift+"),
        (press.meta, "Meta+"),
    ] {
        if on {
            prefix.push_str(name);
        }
    }
    let digit = press.text.chars().next().filter(char::is_ascii_digit).map(|d| format!("Digit{d}"));
    let upper = press.text.to_uppercase();
    NAMED_KEYS
        .iter()
        .map(|k| (*k).to_owned())
        .chain(digit)
        .chain([upper, press.text.to_owned()])
        .filter_map(|name| Chord::parse(&format!("{prefix}{name}")).ok())
        .find(|chord| chord.matches(press))
}

impl HotkeysPage {
    pub fn new(search: String) -> Self {
        Self {
            search,
            ..Self::default()
        }
    }

    pub fn start(&mut self, action: Action) {
        self.recording = Some(action);
        self.pending_move = None;
    }

    pub fn cancel(&mut self) {
        self.recording = None;
        self.pending_move = None;
    }

    /// A key press while recording: binds it, names the action that has it, or (pressed again) moves it.
    pub fn press(&mut self, keymap: &mut Keymap, press: &KeyPress<'_>) -> Recorded {
        let Some(action) = self.recording else {
            return Recorded::Waiting;
        };
        let Some(chord) = chord_of(press) else {
            return Recorded::Waiting;
        };
        let text = chord.text().to_owned();
        match keymap.conflict(&chord, action) {
            None => {
                // A key the action has already is not added twice; recording ends either way.
                let _ = keymap.bind(action, chord);
                self.cancel();
                Recorded::Bound(text)
            }
            Some(other) if self.pending_move.as_deref() == Some(&text) => {
                let taken: Vec<String> = keymap
                    .chords(other)
                    .iter()
                    .filter(|c| c.overlaps(&chord))
                    .map(|c| c.text().to_owned())
                    .collect();
                for t in &taken {
                    keymap.unbind(other, t);
                }
                let _ = keymap.bind(action, chord);
                self.cancel();
                Recorded::Moved { chord: text, from: other }
            }
            Some(other) => {
                self.pending_move = Some(text.clone());
                Recorded::Taken { chord: text, other }
            }
        }
    }
}

/// Puts `action`'s default keys back; a default another action uses now stays with that action. Returns the labels of
/// the keys that could not come back.
pub fn reset(keymap: &mut Keymap, action: Action) -> Vec<String> {
    let current: Vec<String> = keymap.chords(action).iter().map(|c| c.text().to_owned()).collect();
    for text in &current {
        keymap.unbind(action, text);
    }
    default_chords(action)
        .into_iter()
        .filter_map(|chord| {
            let label = chord.label();
            keymap.bind(action, chord).err().map(|_| label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &str, control: bool, shift: bool) -> KeyPress<'_> {
        KeyPress {
            text,
            control,
            alt: false,
            shift,
            meta: false,
            repeat: false,
        }
    }

    #[test]
    fn presses_become_chords() {
        assert_eq!(chord_of(&press("s", true, false)).unwrap().text(), "Control+S");
        let left = slint::SharedString::from(Key::LeftArrow);
        assert_eq!(chord_of(&press(&left, false, true)).unwrap().text(), "Shift+LeftArrow");
        assert_eq!(chord_of(&press(",", true, false)).unwrap().text(), "Control+Comma");
        let f9 = slint::SharedString::from(Key::F9);
        assert_eq!(chord_of(&press(&f9, false, false)).unwrap().text(), "F9");
        let shift = slint::SharedString::from(Key::Shift);
        assert_eq!(chord_of(&press(&shift, false, true)), None);
        assert_eq!(chord_of(&press("", false, false)), None);
    }

    #[test]
    fn a_taken_key_moves_on_the_second_press_and_only_changes_are_stored() {
        let mut keymap = Keymap::default();
        let mut page = HotkeysPage::default();
        assert!(table(&keymap).is_empty());
        assert!(rows(&keymap, "").iter().all(|r| !r.changed));

        page.start(Action::MirrorHorizontal);
        assert_eq!(page.press(&mut keymap, &press("h", false, false)), Recorded::Bound("H".into()));
        assert_eq!(page.recording, None);
        page.start(Action::MirrorVertical);
        let taken = page.press(&mut keymap, &press("m", false, false));
        assert_eq!(
            taken,
            Recorded::Taken {
                chord: "M".into(),
                other: Action::ToggleMute
            }
        );
        assert_eq!(page.recording, Some(Action::MirrorVertical));
        let moved = page.press(&mut keymap, &press("m", false, false));
        assert_eq!(
            moved,
            Recorded::Moved {
                chord: "M".into(),
                from: Action::ToggleMute
            }
        );
        assert!(keymap.chords(Action::ToggleMute).is_empty());
        let stored = table(&keymap);
        assert_eq!(stored.len(), 3, "{stored:?}");
        assert_eq!(stored.get("toggle_mute"), Some(&Some(Vec::new())));
        let (again, notes) = Keymap::from_table(&stored);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(again, keymap);

        // The default of mute is taken by the mirror now: it cannot come back until the mirror lets it go.
        assert_eq!(reset(&mut keymap, Action::ToggleMute), vec!["M".to_owned()]);
        assert!(reset(&mut keymap, Action::MirrorVertical).is_empty());
        assert!(reset(&mut keymap, Action::ToggleMute).is_empty());
        assert_eq!(keymap.chords(Action::ToggleMute).len(), 1);

        let found = rows(&keymap, "mirror");
        assert_eq!(found.len(), 2);
        assert!(found.first().unwrap().changed, "H was added");
        assert_eq!(rows(&keymap, "ctrl+o").len(), 1);
        assert_eq!(rows(&keymap, "queue").len(), 5);
    }
}
