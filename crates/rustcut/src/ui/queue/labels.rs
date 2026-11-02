//! The queue menu's "Load Tags and Rating" (user request 2026-11-02): the tags and the rating of the selected jobs go
//! into the Tag & Rate panel's current choice. Several jobs give every tag any of them has (in queue order, each once)
//! and the highest rating among them. The entry names what it loads ("Load Tag", "Load Rating", "Load Tags and Rating",
//! ...) and is off when the jobs have neither, or while RustView's tags are not loaded.

use rustcut_core::queue::JobSummary;

/// What the selected jobs hold together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gathered {
    /// Every tag id of the jobs, in queue order, each once.
    pub tags: Vec<u32>,
    /// The highest rating of the jobs (0 is a rating).
    pub rating: Option<u8>,
    /// The jobs that have a tag or a rating.
    pub jobs: usize,
    /// Several jobs had different ratings (the highest one is taken).
    pub mixed_ratings: bool,
}

impl Gathered {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.rating.is_none()
    }
}

/// The tags and the rating of `selected` (in their order).
pub fn gather(selected: &[&JobSummary]) -> Gathered {
    let mut gathered = Gathered::default();
    let mut ratings: Vec<u8> = Vec::new();
    for metadata in selected.iter().filter_map(|row| row.metadata.as_ref()) {
        if metadata.tags.is_empty() && metadata.rating.is_none() {
            continue;
        }
        gathered.jobs += 1;
        for tag in &metadata.tags {
            if !gathered.tags.contains(tag) {
                gathered.tags.push(*tag);
            }
        }
        if let Some(rating) = metadata.rating
            && !ratings.contains(&rating)
        {
            ratings.push(rating);
        }
    }
    gathered.rating = ratings.iter().copied().max();
    gathered.mixed_ratings = ratings.len() > 1;
    gathered
}

/// The entry's title: what it loads.
pub fn title(gathered: &Gathered) -> &'static str {
    match (gathered.tags.len(), gathered.rating.is_some()) {
        (0, true) => "Load Rating",
        (1, false) => "Load Tag",
        (1, true) => "Load Tag and Rating",
        (2.., false) => "Load Tags",
        // Nothing to load: the entry is off and names both.
        (_, true) | (0, false) => "Load Tags and Rating",
    }
}

/// The entry's tooltip: what happens, or why it is off.
pub fn tooltip(gathered: &Gathered, rustview: bool, selected: usize) -> String {
    if !rustview {
        return "RustView's tags are not loaded (RustView is off or not reached)".to_owned();
    }
    if gathered.is_empty() {
        return if selected > 1 {
            "None of these jobs has tags or a rating".to_owned()
        } else {
            "The job has no tags and no rating".to_owned()
        };
    }
    let tags = match gathered.tags.len() {
        0 => String::new(),
        1 => "its tag".to_owned(),
        n => format!("its {n} tags"),
    };
    let mut text = if selected > 1 {
        let tags = match gathered.tags.len() {
            0 => String::new(),
            1 => "their tag".to_owned(),
            n => format!("all {n} of their tags"),
        };
        match (tags.is_empty(), gathered.rating) {
            (false, Some(r)) if gathered.mixed_ratings => format!("Sets {tags} and the highest of their ratings ({r})"),
            (false, Some(r)) => format!("Sets {tags} and their rating ({r})"),
            (false, None) => format!("Sets {tags}"),
            (true, Some(r)) if gathered.mixed_ratings => format!("Sets the highest of their ratings ({r})"),
            (true, Some(r)) => format!("Sets their rating ({r})"),
            (true, None) => String::new(),
        }
    } else {
        match (tags.is_empty(), gathered.rating) {
            (false, Some(r)) => format!("Sets {tags} and its rating ({r})"),
            (false, None) => format!("Sets {tags}"),
            (true, Some(r)) => format!("Sets its rating ({r})"),
            (true, None) => String::new(),
        }
    };
    text.push_str(" in the Tag & Rate panel, in place of its current choice");
    text
}

#[cfg(test)]
mod tests {
    use rustcut_core::job::{JobId, JobMetadata, JobState};

    use super::*;
    use crate::ui::queue::filter::tests::row;

    fn job(id: i64, tags: &[u32], rating: Option<u8>) -> JobSummary {
        JobSummary {
            metadata: Some(JobMetadata {
                tags: tags.to_vec(),
                rating,
                source_hash: None,
                without_rustview: false,
            }),
            ..row(id, JobState::Finished, "a")
        }
    }

    #[test]
    fn titles_name_what_is_loaded() {
        let t = |tags: &[u32], rating| title(&gather(&[&job(1, tags, rating)]));
        assert_eq!(t(&[], Some(3)), "Load Rating");
        assert_eq!(t(&[], Some(0)), "Load Rating", "0 is a rating");
        assert_eq!(t(&[4], None), "Load Tag");
        assert_eq!(t(&[4], Some(2)), "Load Tag and Rating");
        assert_eq!(t(&[4, 5], None), "Load Tags");
        assert_eq!(t(&[4, 5], Some(2)), "Load Tags and Rating");
        let empty = gather(&[&job(1, &[], None)]);
        assert!(empty.is_empty());
        assert_eq!((title(&empty), empty.jobs), ("Load Tags and Rating", 0));
    }

    #[test]
    fn several_jobs_map_their_tags_and_take_the_highest_rating() {
        let (a, b, c, d) = (
            job(1, &[3, 1], Some(2)),
            job(2, &[1, 7], Some(4)),
            job(3, &[], None),
            job(4, &[9], None),
        );
        let g = gather(&[&a, &b, &c, &d]);
        assert_eq!(g.tags, [3, 1, 7, 9], "every tag once, in queue order");
        assert_eq!(g.rating, Some(4));
        assert!(g.mixed_ratings);
        assert_eq!(g.jobs, 3, "the job without labels does not count");
        assert_eq!(title(&g), "Load Tags and Rating");
        assert_eq!(
            tooltip(&g, true, 4),
            "Sets all 4 of their tags and the highest of their ratings (4) in the Tag & Rate panel, in place of its \
             current choice"
        );
        let same = gather(&[&job(1, &[], Some(3)), &job(2, &[], Some(3))]);
        assert!(!same.mixed_ratings);
        assert_eq!(title(&same), "Load Rating");
        assert!(tooltip(&same, true, 2).starts_with("Sets their rating (3)"));
    }

    #[test]
    fn unreadable_jobs_and_a_missing_rustview_are_named() {
        let unreadable = JobSummary {
            id: JobId(5),
            ..row(5, JobState::Failed { reason: "x".into() }, "b")
        };
        assert!(gather(&[&unreadable]).is_empty());
        let one = gather(&[&job(1, &[2], Some(1))]);
        assert!(tooltip(&one, false, 1).starts_with("RustView's tags are not loaded"));
        assert_eq!(
            tooltip(&one, true, 1),
            "Sets its tag and its rating (1) in the Tag & Rate panel, in place of its current choice"
        );
        assert_eq!(
            tooltip(&Gathered::default(), true, 2),
            "None of these jobs has tags or a rating"
        );
    }
}
