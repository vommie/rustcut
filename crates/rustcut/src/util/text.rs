//! Where a path or file name is cut when it does not fit (P9.1, review notes 15#9, 30#1, 50#8): `MiddleText`
//! (`ui/controls_text.slint`) cuts the head at its end and always shows the tail, so the cut lands in the middle and the
//! end of the name (its counter, its extension) stays readable: "basic_h264… - 03.mkv", "/home/vo…/Clips/Holiday.mkv".

/// Texts this short are never split (they rarely need cutting, and a split would gain nothing).
const SHORT: usize = 12;
/// A file name up to this many characters is kept whole as the tail of a path.
const WHOLE_NAME: usize = 28;
/// How far from the end of a name's stem a break may start the tail.
const WINDOW: usize = 12;
/// Where a stem without a break is cut: this many characters stay in the tail.
const KEEP: usize = 6;
/// Breaks in the order they are preferred ("Clip - 03" keeps " - 03", "Clip (2)" keeps " (2)").
const BREAKS: [&str; 6] = [" - ", " (", " ", "_", "-", "."];

/// `(head, tail)` with `head + tail == text`; the tail is empty when the text is not split.
pub fn split_middle(text: &str) -> (String, String) {
    if text.chars().count() <= SHORT {
        return (text.to_owned(), String::new());
    }
    // A folder written with a trailing separator is split like the folder itself.
    let body = text.strip_suffix('/').unwrap_or(text);
    let at = match body.rfind('/') {
        Some(slash) => {
            let name = body.get(slash + 1..).unwrap_or_default();
            if name.chars().count() <= WHOLE_NAME {
                Some(slash)
            } else {
                name_break(name).map(|i| slash + 1 + i)
            }
        }
        None => name_break(body),
    };
    match at {
        Some(i) if i > 0 => {
            let (head, tail) = text.split_at(i);
            (head.to_owned(), tail.to_owned())
        }
        _ => (text.to_owned(), String::new()),
    }
}

/// The byte index in `name` where its tail starts: a break near the end of the stem, else the stem's last characters,
/// always with the extension.
fn name_break(name: &str) -> Option<usize> {
    let stem_end = extension_start(name).unwrap_or(name.len());
    let stem = name.get(..stem_end)?;
    let chars: Vec<(usize, char)> = stem.char_indices().collect();
    let window_start = chars
        .len()
        .checked_sub(WINDOW)
        .map_or(0, |i| chars.get(i).map_or(0, |(b, _)| *b));
    for brk in BREAKS {
        if let Some(i) = stem.rfind(brk)
            && i >= window_start
            && i > 0
        {
            return Some(i);
        }
    }
    let keep_from = chars.len().checked_sub(KEEP)?;
    chars.get(keep_from).map(|(b, _)| *b).filter(|b| *b > 0)
}

/// The byte index of the dot of a short alphanumeric extension (`.mkv`, `.md5`), if the name has one.
fn extension_start(name: &str) -> Option<usize> {
    let dot = name.rfind('.')?;
    let ext = name.get(dot + 1..)?;
    let len = ext.chars().count();
    ((1..=5).contains(&len) && dot > 0 && ext.chars().all(char::is_alphanumeric)).then_some(dot)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn split(text: &str) -> (String, String) {
        split_middle(text)
    }

    #[test]
    fn short_texts_stay_whole() {
        assert_eq!(split("Clip 03.mkv"), ("Clip 03.mkv".to_owned(), String::new()));
        assert_eq!(split(""), (String::new(), String::new()));
    }

    #[test]
    fn a_file_name_keeps_its_counter_and_extension() {
        assert_eq!(
            split("basic_h264_720p - 03.mkv"),
            ("basic_h264_720p".to_owned(), " - 03.mkv".to_owned())
        );
        assert_eq!(
            split("Holiday in Rome (2).mp4"),
            ("Holiday in Rome".to_owned(), " (2).mp4".to_owned())
        );
        assert_eq!(
            split("Holiday in Rome 2026"),
            ("Holiday in Rome".to_owned(), " 2026".to_owned())
        );
    }

    #[test]
    fn a_name_without_breaks_keeps_its_last_characters() {
        assert_eq!(
            split("abcdefghijklmnopqrst.mkv"),
            ("abcdefghijklmn".to_owned(), "opqrst.mkv".to_owned())
        );
    }

    #[test]
    fn a_path_keeps_its_file_name() {
        assert_eq!(
            split("/home/alice/Videos/Clips/Holiday.mkv"),
            ("/home/alice/Videos/Clips".to_owned(), "/Holiday.mkv".to_owned())
        );
        assert_eq!(
            split("/home/alice/Videos/Clips/"),
            ("/home/alice/Videos".to_owned(), "/Clips/".to_owned())
        );
    }

    #[test]
    fn a_path_with_a_long_file_name_cuts_inside_the_name() {
        assert_eq!(
            split("/home/alice/Videos/a very long holiday recording from Rome - 12.mkv"),
            (
                "/home/alice/Videos/a very long holiday recording from Rome".to_owned(),
                " - 12.mkv".to_owned()
            )
        );
    }

    #[test]
    fn umlauts_and_other_scripts_split_on_characters() {
        let (head, tail) = split("Übungsaufnahme_größer_München - 07.mkv");
        assert_eq!(tail, " - 07.mkv");
        assert_eq!(head, "Übungsaufnahme_größer_München");
    }

    proptest! {
        #[test]
        fn the_parts_always_make_the_whole(text in "\\PC{0,60}") {
            let (head, tail) = split_middle(&text);
            prop_assert_eq!(format!("{head}{tail}"), text);
        }

        #[test]
        fn a_split_never_leaves_an_empty_head(text in "[a-z /._-]{13,60}") {
            let (head, tail) = split_middle(&text);
            prop_assert!(tail.is_empty() || !head.is_empty());
        }
    }
}
