use super::*;

fn hits(text: &str, regex: bool, name: &str) -> Option<Vec<String>> {
    let pattern = Pattern::parse(text, regex).unwrap().unwrap();
    pattern.is_match(name).then(|| {
        pattern
            .spans(name)
            .into_iter()
            .map(|r| name.get(r).unwrap_or_default().to_owned())
            .collect()
    })
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn plain_text_is_found_anywhere_in_any_case() {
    assert_eq!(hits("harbor", false, "2026-07-12 Harbor 03.mp4"), Some(s(&["Harbor"])));
    assert_eq!(hits("a", false, "Banana.mp4"), Some(s(&["a", "a", "a"])));
    assert_eq!(hits("x", false, "Banana.mp4"), None);
    // Characters with a meaning in a regex mean nothing here.
    assert_eq!(hits("(1)", false, "clip (1).mp4"), Some(s(&["(1)"])));
    assert_eq!(hits(".mp4", false, "a.mp4"), Some(s(&[".mp4"])));
    assert!(Pattern::parse("", false).unwrap().is_none(), "empty keeps everything");
}

#[test]
fn wildcards_match_the_whole_name_like_dolphin() {
    assert_eq!(
        hits("*harbor*.mp4", false, "2026 Harbor 03.mp4"),
        Some(s(&["Harbor", ".mp4"]))
    );
    assert_eq!(hits("*harbor*.mp4", false, "2026 Harbor 03.mkv"), None);
    assert_eq!(hits("clip ??.mp4", false, "clip 07.mp4"), Some(s(&["clip ", ".mp4"])));
    assert_eq!(hits("clip ??.mp4", false, "clip 107.mp4"), None);
    assert_eq!(hits("clip 0[1-3]*", false, "clip 02 b.mp4"), Some(s(&["clip 0"])));
    assert_eq!(hits("clip 0[1-3]*", false, "clip 05.mp4"), None);
    assert_eq!(hits("clip [!0]*", false, "clip 5.mp4"), Some(s(&["clip "])));
    assert_eq!(hits("clip [!0]*", false, "clip 0.mp4"), None);
    // Regex characters in the literal parts are taken literally.
    assert_eq!(hits("a+b*", false, "a+b.mp4"), Some(s(&["a+b"])));
    assert_eq!(hits("a+b*", false, "aab.mp4"), None);
}

#[test]
fn a_regex_highlights_every_match() {
    assert_eq!(
        hits("^2026-07-1[0-4]", true, "2026-07-12 Harbor.mp4"),
        Some(s(&["2026-07-12"]))
    );
    assert_eq!(
        hits("harbor|beach", true, "Beach and Harbor.mp4"),
        Some(s(&["Beach", "Harbor"]))
    );
    // An empty match counts as a hit but draws nothing.
    assert_eq!(hits("x*", true, "abc.mp4"), Some(s(&[])));
}

#[test]
fn a_wrong_pattern_says_why_with_an_example() {
    let err = Pattern::parse("(harbor|beach", true).unwrap_err();
    assert_eq!(err.message, "Not a valid regular expression");
    assert!(err.example.starts_with("Unclosed group. Example: "), "{}", err.example);
    let err = Pattern::parse("clip [1-3", false).unwrap_err();
    assert_eq!(err.message, "Not a complete pattern");
    assert!(err.example.contains("“]” is missing"), "{}", err.example);
    // A bracket alone is no class.
    assert!(Pattern::parse("[]", false).is_err());
    assert!(Pattern::parse("[!]", false).is_err());
}
