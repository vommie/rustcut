//! Which choices show as a segmented row instead of a list (P9.1, D-P9j, review note 09#1): at most three short entries
//! fit side by side in a field's width ("8-bit · 10-bit", "None · Static · Adaptive"); longer lists stay choices.

/// The longest entry a segment shows without cutting it.
const SHORT: usize = 9;

/// Whether a choice of these entries is drawn as a segmented row.
pub fn segmented<S: AsRef<str>>(items: &[S]) -> bool {
    (2..=3).contains(&items.len()) && items.iter().all(|item| item.as_ref().chars().count() <= SHORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_or_three_short_entries_are_segmented() {
        assert!(segmented(&["8-bit", "10-bit"]));
        assert!(segmented(&["None", "Static", "Adaptive"]));
    }

    #[test]
    fn long_or_many_entries_stay_a_list() {
        assert!(!segmented(&["Black", "Previous frame"]));
        assert!(!segmented(&["bwdif", "yadif", "w3fdif", "kerndeint"]));
        assert!(!segmented(&["Only"]));
        assert!(!segmented::<&str>(&[]));
    }
}
