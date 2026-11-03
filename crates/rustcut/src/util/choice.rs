//! Which choices show as a segmented row instead of a list (P9.1, D-P9j, review note 09#1): at most three short entries
//! fit side by side in a field's width ("8-bit · 10-bit", "None · Static · Adaptive"); longer lists stay choices.
//! A long list has a search field (user report 2026-11-03: a plugin's tag and target folder lists); [`search`] says
//! which entries it keeps.

/// The longest entry a segment shows without cutting it.
const SHORT: usize = 9;

/// Whether a choice of these entries is drawn as a segmented row.
pub fn segmented<S: AsRef<str>>(items: &[S]) -> bool {
    (2..=3).contains(&items.len()) && items.iter().all(|item| item.as_ref().chars().count() <= SHORT)
}

/// The positions of the entries that hold every word of `query` (case is ignored), in their order; an empty query keeps
/// all of them. "holiday 24" finds "Travel › Holiday 2024".
pub fn search<S: AsRef<str>>(items: &[S], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let item = item.as_ref().to_lowercase();
            words.iter().all(|word| item.contains(word.as_str()))
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_keeps_entries_with_every_word() {
        let items = ["Not set", "Travel › Holiday 2024", "Travel › Holiday 2023", "Family"];
        assert_eq!(search(&items, ""), vec![0, 1, 2, 3]);
        assert_eq!(search(&items, "   "), vec![0, 1, 2, 3]);
        assert_eq!(search(&items, "holiday"), vec![1, 2]);
        assert_eq!(search(&items, "HOLIDAY 24"), vec![1]);
        assert_eq!(search(&items, "travel family"), Vec::<usize>::new());
        assert_eq!(search(&items, "›"), vec![1, 2]);
    }

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
