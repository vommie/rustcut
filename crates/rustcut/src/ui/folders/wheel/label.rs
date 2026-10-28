//! A wheel field's label without Slint (the user's answers 2026-10-28, `2026-10-28-rustcut-long-names`): at most two
//! lines broken at word boundaries (spaces, after `_ - .`, before a capital that follows a small letter); when it does
//! not fit, the font first shrinks to 85 %; a word that still does not fit is cut in the middle ("einzusamm…" over
//! "…ordnername"), so its start and end stay. Measured against the field's real width, never a character count; the
//! width of a text is estimated from the UI font's typical advances (with a margin).

/// The smallest font scale before cutting.
pub const MIN_SCALE: f32 = 0.85;
const SCALES: [f32; 4] = [1.0, 0.95, 0.9, MIN_SCALE];
/// Share of the field's width the text may use (the estimate's margin).
const USE: f32 = 0.92;
const ELLIPSIS: char = '…';

/// What a field shows: its lines and the font scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub lines: Vec<String>,
    pub scale: f32,
}

impl Label {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// The advance of `c` in ems (Noto Sans / DejaVu Sans proportions, rounded up).
fn advance(c: char) -> f32 {
    match c {
        ' ' => 0.27,
        'i' | 'l' | 'j' | 'I' | '.' | ',' | ';' | ':' | '\'' | '!' | '|' | '`' => 0.3,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '-' | '/' | '\\' => 0.38,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.86,
        '…' => 0.9,
        '_' | '&' => 0.62,
        c if c.is_ascii_uppercase() => 0.68,
        c if c.is_ascii_digit() => 0.57,
        c if u32::from(c) >= 0x2E80 => 1.0,
        _ => 0.56,
    }
}

/// The estimated width of `text` at `font` pixels.
pub fn width(text: &str, font: f32) -> f32 {
    text.chars().map(advance).sum::<f32>() * font
}

/// A piece of the name between break opportunities; `space` = a space stood before it.
#[derive(Debug, Clone, PartialEq)]
struct Piece {
    text: String,
    space: bool,
}

fn pieces(name: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    for word in name.split_whitespace() {
        let mut current = String::new();
        let mut first = true;
        let mut previous: Option<char> = None;
        for c in word.chars() {
            let camel = previous.is_some_and(char::is_lowercase) && c.is_uppercase();
            if camel && !current.is_empty() {
                out.push(Piece {
                    text: std::mem::take(&mut current),
                    space: first,
                });
                first = false;
            }
            current.push(c);
            if matches!(c, '_' | '-' | '.') {
                out.push(Piece {
                    text: std::mem::take(&mut current),
                    space: first,
                });
                first = false;
            }
            previous = Some(c);
        }
        if !current.is_empty() {
            out.push(Piece {
                text: current,
                space: first,
            });
        }
    }
    out
}

/// Fills lines greedily with the pieces (no line limit).
fn wrap(pieces: &[Piece], room: f32, font: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for piece in pieces {
        let joined = if line.is_empty() {
            piece.text.clone()
        } else if piece.space {
            format!("{line} {}", piece.text)
        } else {
            format!("{line}{}", piece.text)
        };
        if line.is_empty() || width(&joined, font) <= room {
            line = joined;
        } else {
            lines.push(std::mem::take(&mut line));
            line.clone_from(&piece.text);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The longest start of `text` that fits `room` with "…" after it.
fn head(text: &str, room: f32, font: f32) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let mut next = out.clone();
        next.push(c);
        if width(&format!("{next}{ELLIPSIS}"), font) > room {
            break;
        }
        out = next;
    }
    format!("{out}{ELLIPSIS}")
}

/// The longest end of `text` that fits `room` with "…" before it.
fn tail(text: &str, room: f32, font: f32) -> String {
    let mut kept: Vec<char> = Vec::new();
    for c in text.chars().rev() {
        kept.insert(0, c);
        let candidate: String = std::iter::once(ELLIPSIS).chain(kept.iter().copied()).collect();
        if width(&candidate, font) > room {
            kept.remove(0);
            break;
        }
    }
    std::iter::once(ELLIPSIS).chain(kept).collect()
}

/// `text` cut in the middle to fit `room` on one line ("Kunde…Abgabe").
fn middle(text: &str, room: f32, font: f32) -> String {
    if width(text, font) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut front, mut back) = (0, 0);
    loop {
        // Alternate: one more from the start, then one more from the end.
        let (f, b) = if front <= back {
            (front + 1, back)
        } else {
            (front, back + 1)
        };
        if f + b >= chars.len() {
            break;
        }
        let candidate: String = chars
            .iter()
            .take(f)
            .chain(std::iter::once(&ELLIPSIS))
            .chain(chars.iter().skip(chars.len() - b))
            .collect();
        if width(&candidate, font) > room {
            break;
        }
        (front, back) = (f, b);
    }
    chars
        .iter()
        .take(front)
        .chain(std::iter::once(&ELLIPSIS))
        .chain(chars.iter().skip(chars.len() - back))
        .collect()
}

/// The label of `name` in a field `room` pixels wide at the wheel's font size `font`.
pub fn fit(name: &str, room: f32, font: f32) -> Label {
    let room = room * USE;
    let parts = pieces(name);
    if parts.is_empty() {
        return Label {
            lines: Vec::new(),
            scale: 1.0,
        };
    }
    for scale in SCALES {
        let size = font * scale;
        let lines = wrap(&parts, room, size);
        if lines.len() <= 2 && lines.iter().all(|l| width(l, size) <= room) {
            return Label { lines, scale };
        }
    }
    let size = font * MIN_SCALE;
    let lines = wrap(&parts, room, size);
    let first_fits = lines.first().is_some_and(|l| width(l, size) <= room);
    let lines = if first_fits {
        // The first line stands; the rest is cut in the middle so its end stays.
        let first = lines.first().cloned().unwrap_or_default();
        let rest = lines.get(1..).map(|r| r.join(" ")).unwrap_or_default();
        vec![first, middle(&rest, room, size)]
    } else {
        // One word too long for a line: its start on the first line, its end on the second.
        let whole = lines.join(" ");
        vec![head(&whole, room, size), tail(&whole, room, size)]
    };
    Label {
        lines,
        scale: MIN_SCALE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fits(label: &Label, room: f32, font: f32) -> bool {
        label
            .lines
            .iter()
            .all(|l| width(l, font * label.scale) <= room * USE + 0.01)
    }

    #[test]
    fn short_names_stay_whole() {
        let l = fit("Shorts", 120.0, 14.0);
        assert_eq!((l.lines.clone(), l.scale), (vec!["Shorts".to_owned()], 1.0));
        assert!(fit("", 120.0, 14.0).lines.is_empty());
    }

    #[test]
    fn names_break_at_words_and_separators() {
        let l = fit("ein sehr langer ordnername", 120.0, 14.0);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines.join(" "), "ein sehr langer ordnername", "nothing lost");
        assert!(fits(&l, 120.0, 14.0));
        let l = fit("Urlaub_Kreta_2026", 90.0, 14.0);
        assert_eq!(l.lines.concat(), "Urlaub_Kreta_2026", "broken after _ without a space");
        assert_eq!(l.lines.len(), 2);
        let l = fit("KundeNordAbgabe", 90.0, 14.0);
        assert_eq!(l.lines.concat(), "KundeNordAbgabe", "broken before a capital");
        assert!(l.lines.len() == 2 && fits(&l, 90.0, 14.0));
    }

    #[test]
    fn the_font_shrinks_before_anything_is_cut() {
        // Three words of 2.24 em: at full size one word per line (three lines), at 90 % two words share a line.
        let room = 62.0 / USE;
        let l = fit("aaaa bbbb cccc", room, 14.0);
        assert!((l.scale - 0.9).abs() < f32::EPSILON, "{l:?}");
        assert_eq!(l.lines, ["aaaa bbbb", "cccc"]);
    }

    #[test]
    fn a_long_word_keeps_its_start_and_end() {
        let l = fit("einzusammenhängenderordnername", 110.0, 14.0);
        assert_eq!(l.scale, MIN_SCALE);
        let [first, second] = l.lines.as_slice() else {
            panic!("{l:?}")
        };
        assert!(first.starts_with("einzu") && first.ends_with('…'), "{first}");
        assert!(second.starts_with('…') && second.ends_with("ordnername"), "{second}");
        assert!(fits(&l, 110.0, 14.0));
        // Several words beyond two lines: the first line stands, the rest keeps its end.
        let l = fit("Archiv der alten Kundenprojekte von zweitausendzwanzig", 110.0, 14.0);
        assert_eq!(l.lines.len(), 2);
        assert!(l.lines.first().is_some_and(|f| f.starts_with("Archiv")));
        assert!(l.lines.get(1).is_some_and(|s| s.contains('…') && s.ends_with('g')));
        assert!(fits(&l, 110.0, 14.0));
    }
}
