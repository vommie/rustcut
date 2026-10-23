//! File-name hygiene: sanitizing a stem, free `Name (n)` variants, and whether two paths name the same file.

use std::path::Path;

use super::NameError;
use crate::render::Container;

/// Longest stem in bytes. The render first writes `.<stem>.rustcut-part.<ext>` (ADR-9), which must fit the 255-byte limit
/// of Linux file systems too: 255 - 1 (leading dot) - 14 (`.rustcut-part.`) - 4 (longest extension).
pub const MAX_STEM_BYTES: usize = 236;

/// Makes `raw` usable as a file stem: `/`, NUL and control characters become `_`, surrounding whitespace goes, the result is
/// cut to [`MAX_STEM_BYTES`] at a character boundary. `.` and `..` are refused.
pub fn sanitize_stem(raw: &str) -> Result<String, NameError> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c == '/' || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    let mut end = trimmed.len().min(MAX_STEM_BYTES);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let stem = trimmed.get(..end).unwrap_or_default().trim_end();
    match stem {
        "" => Err(NameError::Empty),
        "." | ".." => Err(NameError::Reserved(stem.to_owned())),
        _ => Ok(stem.to_owned()),
    }
}

/// `stem.ext` for the container.
pub fn file_name(stem: &str, container: Container) -> String {
    format!("{stem}.{}", container.extension())
}

/// The first `Base (n)` that `is_taken` rejects not, starting after the number `stem` already carries (PyCutRv's
/// "auto-rename"). `None` when 9 999 variants are taken.
pub fn next_free_variant(stem: &str, mut is_taken: impl FnMut(&str) -> bool) -> Option<String> {
    let (base, start) = split_variant(stem).unwrap_or((stem, 0));
    (start.saturating_add(1)..10_000)
        .map(|n| format!("{base} ({n})"))
        .find(|candidate| !is_taken(candidate))
}

/// `"Name (3)"` -> `("Name", 3)`.
fn split_variant(stem: &str) -> Option<(&str, u32)> {
    let inner = stem.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let number = inner.get(open + 1..)?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((inner.get(..open)?.trim_end(), number.parse().ok()?))
}

/// Whether two paths name the same file, decided lexically (`a/./b/../c` = `a/c`) without touching the drive. Links are
/// not resolved; the render checks the real file again when it starts.
pub fn same_path(a: &Path, b: &Path) -> bool {
    path_clean::clean(a) == path_clean::clean(b)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn sanitizing() {
        assert_eq!(sanitize_stem("  a/b\u{0}c\nd  ").unwrap(), "a_b_c_d");
        assert_eq!(sanitize_stem("Übung – Teil 1").unwrap(), "Übung – Teil 1");
        assert_eq!(sanitize_stem("   "), Err(NameError::Empty));
        assert_eq!(sanitize_stem(".."), Err(NameError::Reserved("..".into())));
        let long = "ä".repeat(200);
        let cut = sanitize_stem(&long).unwrap();
        assert!(cut.len() <= MAX_STEM_BYTES && cut.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn variants_continue_the_number() {
        let taken = ["Clip (1)", "Clip (2)"];
        assert_eq!(
            next_free_variant("Clip", |c| taken.contains(&c)).as_deref(),
            Some("Clip (3)")
        );
        assert_eq!(next_free_variant("Clip (5)", |_| false).as_deref(), Some("Clip (6)"));
        assert_eq!(
            next_free_variant("Clip (x)", |_| false).as_deref(),
            Some("Clip (x) (1)")
        );
        assert_eq!(next_free_variant("Clip", |_| true), None);
    }

    #[test]
    fn lexical_path_identity() {
        assert!(same_path(Path::new("/v/a/../b.mkv"), Path::new("/v/./b.mkv")));
        assert!(!same_path(Path::new("/v/b.mkv"), Path::new("/v/B.mkv")));
    }

    proptest! {
        #[test]
        fn sanitized_stems_are_valid(raw in "\\PC{0,300}") {
            if let Ok(stem) = sanitize_stem(&raw) {
                prop_assert!(!stem.is_empty() && stem.len() <= MAX_STEM_BYTES);
                prop_assert!(!stem.contains('/') && !stem.chars().any(char::is_control));
                prop_assert_eq!(stem.trim(), stem.as_str());
                prop_assert_eq!(sanitize_stem(&stem).unwrap(), stem);
            }
        }

        #[test]
        fn a_free_variant_is_never_taken(stem in "[a-z]{1,8}( \\([0-9]{1,3}\\))?", taken in prop::collection::hash_set(1u32..40, 0..30)) {
            let is_taken = |c: &str| taken.iter().any(|n| c.ends_with(&format!("({n})")));
            if let Some(v) = next_free_variant(&stem, is_taken) {
                prop_assert!(!is_taken(&v));
            }
        }
    }
}
