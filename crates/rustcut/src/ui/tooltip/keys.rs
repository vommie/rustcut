//! The key caps of a tooltip (user decision Q-T1: on their own line under the text). A shortcut text is what
//! `Chord::label` writes ("Ctrl+S", "Shift+LeftArrow", "Ctrl++"); an action with several chords joins them with
//! [`OR`]. Every key becomes its own cap, the arrows show as symbols, long key names get the short names keyboards
//! print. A text that is no key (RustView's ".*" badge of the regex switch) stays one plain chip without the "Key"
//! label.

use crate::icons::Icon;

/// Between the chords of an action with several keys ("I or Home").
pub const OR: &str = " or ";

/// One cap: a key's short name, or a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cap {
    pub text: String,
    pub icon: Option<Icon>,
    /// Another chord starts with this cap ("or" before it).
    pub alternative: bool,
}

/// The caps of a shortcut text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Caps {
    pub caps: Vec<Cap>,
    /// Keys (the tooltip says "Key"/"Keys"), or a plain chip.
    pub keys: bool,
    /// More than one chord.
    pub alternatives: bool,
}

const MODIFIERS: [(&str, &str); 6] = [
    ("super", "Super"),
    ("meta", "Super"),
    ("ctrl", "Ctrl"),
    ("control", "Ctrl"),
    ("alt", "Alt"),
    ("shift", "Shift"),
];

/// The caps of `shortcut` ("" gives none).
pub fn caps(shortcut: &str) -> Caps {
    let shortcut = shortcut.trim();
    if shortcut.is_empty() {
        return Caps::default();
    }
    let mut out = Caps {
        caps: Vec::new(),
        keys: true,
        alternatives: false,
    };
    for (i, chord) in shortcut.split(OR).enumerate() {
        let Some(chord_caps) = chord_caps(chord.trim()) else {
            return Caps {
                caps: vec![plain(shortcut)],
                keys: false,
                alternatives: false,
            };
        };
        out.alternatives |= i > 0;
        out.caps.extend(chord_caps.into_iter().enumerate().map(|(j, mut cap)| {
            cap.alternative = i > 0 && j == 0;
            cap
        }));
    }
    out
}

fn plain(text: &str) -> Cap {
    Cap {
        text: text.to_owned(),
        icon: None,
        alternative: false,
    }
}

/// The caps of one chord, `None` when it is no key. Modifiers come first, each followed by "+"; what is left is the
/// key, which may be "+" itself ("Ctrl++").
fn chord_caps(chord: &str) -> Option<Vec<Cap>> {
    let mut rest = chord;
    let mut caps = Vec::new();
    loop {
        let found = MODIFIERS.iter().find_map(|(name, label)| {
            let head = rest.get(..name.len())?;
            let after = rest.get(name.len()..)?;
            (head.eq_ignore_ascii_case(name) && after.starts_with('+') && after.len() > 1)
                .then(|| (*label, after.get(1..).unwrap_or_default()))
        });
        let Some((label, after)) = found else { break };
        caps.push(plain(label));
        rest = after;
    }
    caps.push(key_cap(rest)?);
    Some(caps)
}

/// The cap of a key name as Slint's `Keys` writes it, `None` when it is no key.
fn key_cap(key: &str) -> Option<Cap> {
    let symbol = |icon, text: &str| Cap {
        text: text.to_owned(),
        icon: Some(icon),
        alternative: false,
    };
    let lower = key.to_ascii_lowercase();
    let cap = match lower.as_str() {
        "leftarrow" | "left" => symbol(Icon::KeyLeft, "Left"),
        "rightarrow" | "right" => symbol(Icon::KeyRight, "Right"),
        "uparrow" | "up" => symbol(Icon::KeyUp, "Up"),
        "downarrow" | "down" => symbol(Icon::KeyDown, "Down"),
        "return" | "enter" => plain("Enter"),
        "escape" | "esc" => plain("Esc"),
        "pageup" | "pgup" => plain("PgUp"),
        "pagedown" | "pgdn" => plain("PgDn"),
        "delete" | "del" => plain("Del"),
        "insert" | "ins" => plain("Ins"),
        "scrolllock" => plain("ScrLk"),
        "sysreq" => plain("SysRq"),
        "backspace" | "tab" | "space" | "home" | "end" | "pause" | "menu" | "capslock" => plain(&capitalized(&lower)),
        _ if key.chars().count() == 1 => plain(&key.to_uppercase()),
        _ if is_function_key(&lower) => plain(&key.to_uppercase()),
        _ => return None,
    };
    Some(cap)
}

fn capitalized(lower: &str) -> String {
    let mut chars = lower.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// F1 ... F24.
fn is_function_key(lower: &str) -> bool {
    lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(c: &Caps) -> Vec<&str> {
        c.caps.iter().map(|cap| cap.text.as_str()).collect()
    }

    #[test]
    fn every_key_of_a_chord_is_its_own_cap() {
        let c = caps("Ctrl+Shift+S");
        assert!(c.keys);
        assert_eq!(texts(&c), ["Ctrl", "Shift", "S"]);
        assert!(!c.alternatives);
    }

    #[test]
    fn the_plus_key_survives_the_separator() {
        assert_eq!(texts(&caps("Ctrl++")), ["Ctrl", "+"]);
        assert_eq!(texts(&caps("+")), ["+"]);
        assert_eq!(texts(&caps("Shift++")), ["Shift", "+"]);
    }

    #[test]
    fn alternatives_are_marked_where_they_start() {
        let c = caps("I or Home");
        assert_eq!(texts(&c), ["I", "Home"]);
        assert!(c.alternatives);
        assert_eq!(
            c.caps.iter().map(|cap| cap.alternative).collect::<Vec<_>>(),
            [false, true]
        );
        let c = caps("Ctrl+UpArrow or Alt+K");
        assert_eq!(
            c.caps.iter().map(|cap| cap.alternative).collect::<Vec<_>>(),
            [false, false, true, false]
        );
    }

    #[test]
    fn arrows_are_symbols_and_long_names_short() {
        let c = caps("Shift+LeftArrow");
        assert_eq!(c.caps.get(1).and_then(|cap| cap.icon), Some(Icon::KeyLeft));
        assert_eq!(texts(&caps("Escape")), ["Esc"]);
        assert_eq!(texts(&caps("Esc")), ["Esc"]);
        assert_eq!(texts(&caps("PageDown")), ["PgDn"]);
        assert_eq!(texts(&caps("Return")), ["Enter"]);
        assert_eq!(texts(&caps("ScrollLock")), ["ScrLk"]);
        assert_eq!(texts(&caps("Space")), ["Space"]);
        assert_eq!(texts(&caps("Ctrl+,")), ["Ctrl", ","]);
        assert_eq!(texts(&caps("F9")), ["F9"]);
        assert_eq!(texts(&caps("Super+e")), ["Super", "E"]);
    }

    #[test]
    fn a_text_that_is_no_key_stays_one_chip() {
        let c = caps(".*");
        assert!(!c.keys);
        assert_eq!(texts(&c), [".*"]);
        assert!(!caps("Ctrl+Hello").keys);
        assert!(!caps("F25").keys);
    }

    #[test]
    fn nothing_gives_no_caps() {
        assert_eq!(caps(""), Caps::default());
        assert_eq!(caps("  "), Caps::default());
    }

    #[test]
    fn a_word_that_starts_like_a_modifier_is_a_key() {
        // "Alt" alone, without a key after it, is no modifier prefix; "Shift" alone is no known key.
        assert!(!caps("Alt").keys);
        assert_eq!(texts(&caps("Ctrl+Alt+Delete")), ["Ctrl", "Alt", "Del"]);
    }
}
