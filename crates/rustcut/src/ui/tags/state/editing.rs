//! Editing the RustView entries of queued jobs in the Tag & Rate panel (user decision Q-G3; several at once since 0.0.102,
//! user request 2026-11-02 with RustView's multi-selection logic): every job keeps its own tags and rating; a tag that
//! all of them have is "assigned" and a click removes it from all, a tag that only some have is "partial" and a click,
//! like a click on a tag none has, gives it to all. A rating all of them share is chosen and a click on it clears it
//! from all (RustCut's single job did that already, user decision 2026-11-02); several ratings are "mixed": none is
//! chosen and each one the jobs have shows faintly, and a click sets that rating on all. Nothing is written until Apply.

use std::collections::{HashMap, HashSet};

use rustcut_core::job::{JobId, JobMetadata};

use super::Choice;

/// The edited jobs' ratings: one they share (`None` = none of them has one), or several.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ratings {
    Shared(Option<u8>),
    Mixed,
}

/// One queued job being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedJob {
    pub id: JobId,
    pub name: String,
    /// What the job holds (its source hash stays).
    pub original: JobMetadata,
    pub choice: Choice,
}

impl EditedJob {
    pub fn new(id: JobId, name: String, original: JobMetadata) -> Self {
        let choice = Choice {
            tags: original.tags.clone(),
            rating: original.rating,
        };
        Self {
            id,
            name,
            original,
            choice,
        }
    }

    fn original_choice(&self) -> Choice {
        Choice {
            tags: self.original.tags.clone(),
            rating: self.original.rating,
        }
    }

    /// The job's new metadata when it differs from what the job holds.
    fn edited(&self) -> Option<JobMetadata> {
        let mut new = self.choice.clone();
        let mut old = self.original_choice();
        new.tags.sort_unstable();
        old.tags.sort_unstable();
        (new != old).then(|| JobMetadata {
            tags: new.tags,
            rating: new.rating,
            source_hash: self.original.source_hash.clone(),
            without_rustview: self.original.without_rustview,
        })
    }
}

/// The queued jobs whose entries are edited (at least one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub jobs: Vec<EditedJob>,
}

impl Editing {
    /// "Holiday - 01" for one job, "3 jobs" for several.
    pub fn title(&self) -> String {
        match self.jobs.as_slice() {
            [one] => one.name.clone(),
            jobs => format!("{} jobs", jobs.len()),
        }
    }

    /// How many of the jobs have each tag.
    pub fn counts(&self) -> HashMap<u32, usize> {
        let mut counts = HashMap::new();
        for job in &self.jobs {
            for tag in &job.choice.tags {
                *counts.entry(*tag).or_insert(0) += 1;
            }
        }
        counts
    }

    /// The choice all of the jobs share: the tags every one has (in the first job's order), the rating when they agree.
    pub fn common(&self) -> Choice {
        let total = self.jobs.len();
        let counts = self.counts();
        let mut tags: Vec<u32> = Vec::new();
        for job in &self.jobs {
            for tag in &job.choice.tags {
                if counts.get(tag) == Some(&total) && !tags.contains(tag) {
                    tags.push(*tag);
                }
            }
        }
        Choice {
            tags,
            rating: match self.ratings() {
                Ratings::Shared(rating) => rating,
                Ratings::Mixed => None,
            },
        }
    }

    /// The tags only some of the jobs have.
    pub fn partial_tags(&self) -> HashSet<u32> {
        let total = self.jobs.len();
        self.counts()
            .into_iter()
            .filter(|(_, n)| *n < total)
            .map(|(tag, _)| tag)
            .collect()
    }

    /// Whether every job has the same rating (also none), or they differ.
    fn ratings(&self) -> Ratings {
        let first = self.jobs.first().and_then(|j| j.choice.rating);
        if self.jobs.iter().all(|j| j.choice.rating == first) {
            Ratings::Shared(first)
        } else {
            Ratings::Mixed
        }
    }

    /// The ratings the rating bar shows faintly (0 to 5): those the jobs have while they do not agree.
    pub fn partial_ratings(&self) -> [bool; 6] {
        let mut partial = [false; 6];
        if self.ratings() == Ratings::Mixed {
            for rating in self.jobs.iter().filter_map(|j| j.choice.rating) {
                if let Some(slot) = partial.get_mut(usize::from(rating)) {
                    *slot = true;
                }
            }
        }
        partial
    }

    /// A tag click: removed from all when all have it, else given to all.
    pub fn click_tag(&mut self, id: u32) {
        let all = self.jobs.iter().all(|j| j.choice.tags.contains(&id));
        for job in &mut self.jobs {
            if all {
                job.choice.tags.retain(|t| *t != id);
            } else if !job.choice.tags.contains(&id) {
                job.choice.tags.push(id);
            }
        }
    }

    /// A rating click: cleared from all when all have it, else set on all.
    pub fn click_rating(&mut self, rating: Option<u8>) {
        let next = if self.ratings() == Ratings::Shared(rating) {
            None
        } else {
            rating
        };
        self.set_rating(next);
    }

    pub fn set_rating(&mut self, rating: Option<u8>) {
        for job in &mut self.jobs {
            job.choice.rating = rating;
        }
    }

    /// Gives `tags` to every job (those it has stay).
    pub fn add_tags(&mut self, tags: &[u32]) {
        for job in &mut self.jobs {
            job.choice.add(tags);
        }
    }

    /// Every job gets exactly `choice`.
    pub fn set_all(&mut self, choice: &Choice) {
        for job in &mut self.jobs {
            job.choice = choice.clone();
        }
    }

    /// Something can be cleared: a job has a tag or a rating.
    pub fn has_anything(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| !j.choice.tags.is_empty() || j.choice.rating.is_some())
    }

    /// The jobs whose entries changed, with their new metadata (tags sorted, as RustView stores them).
    pub fn edited(&self) -> Vec<(JobId, JobMetadata)> {
        self.jobs.iter().filter_map(|j| Some((j.id, j.edited()?))).collect()
    }

    /// Apply wrote job `id`: what it holds now is its choice.
    pub fn written(&mut self, id: JobId) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id)
            && let Some(metadata) = job.edited()
        {
            job.original = metadata;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: i64, tags: &[u32], rating: Option<u8>) -> EditedJob {
        EditedJob::new(
            JobId(id),
            format!("Clip {id}"),
            JobMetadata {
                tags: tags.to_vec(),
                rating,
                source_hash: Some(format!("hash{id}")),
                without_rustview: false,
            },
        )
    }

    fn three() -> Editing {
        Editing {
            jobs: vec![
                job(1, &[1, 2], Some(3)),
                job(2, &[2, 3], Some(3)),
                job(3, &[2], Some(5)),
            ],
        }
    }

    #[test]
    fn full_and_partial_tags_and_mixed_ratings() {
        let e = three();
        assert_eq!(e.title(), "3 jobs");
        assert_eq!(
            e.common(),
            Choice {
                tags: vec![2],
                rating: None
            },
            "the ratings differ"
        );
        assert_eq!(e.partial_tags(), HashSet::from([1, 3]));
        assert_eq!(e.partial_ratings(), [false, false, false, true, false, true]);
        let one = Editing {
            jobs: vec![job(4, &[7], Some(2))],
        };
        assert_eq!(one.title(), "Clip 4");
        assert_eq!(
            one.common(),
            Choice {
                tags: vec![7],
                rating: Some(2)
            }
        );
        assert!(one.partial_tags().is_empty() && one.partial_ratings() == [false; 6]);
    }

    #[test]
    fn clicks_follow_rustview_full_removes_partial_and_none_add() {
        let mut e = three();
        e.click_tag(1);
        assert!(
            e.jobs.iter().all(|j| j.choice.tags.contains(&1)),
            "partial: given to all"
        );
        e.click_tag(1);
        assert!(
            e.jobs.iter().all(|j| !j.choice.tags.contains(&1)),
            "full: removed from all"
        );
        e.click_tag(9);
        assert!(e.jobs.iter().all(|j| j.choice.tags.contains(&9)), "none: given to all");
        e.click_rating(Some(4));
        assert!(e.jobs.iter().all(|j| j.choice.rating == Some(4)), "mixed: set on all");
        e.click_rating(Some(4));
        assert!(
            e.jobs.iter().all(|j| j.choice.rating.is_none()),
            "shared: cleared from all"
        );
        e.click_rating(Some(0));
        assert!(e.jobs.iter().all(|j| j.choice.rating == Some(0)), "0 is a rating");
    }

    #[test]
    fn apply_writes_only_the_changed_jobs() {
        let mut e = three();
        assert!(e.edited().is_empty());
        e.click_tag(3);
        let edited = e.edited();
        let ids: Vec<JobId> = edited.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [JobId(1), JobId(3)], "job 2 had tag 3 already");
        let (_, first) = edited.first().unwrap();
        assert_eq!((first.tags.clone(), first.rating), (vec![1, 2, 3], Some(3)));
        assert_eq!(first.source_hash.as_deref(), Some("hash1"), "the hash stays");
        e.written(JobId(1));
        assert_eq!(e.edited().len(), 1, "job 1 holds it now");
        // The same tags in another order are no change.
        let mut same = Editing {
            jobs: vec![job(5, &[2, 1], None)],
        };
        same.jobs[0].choice.tags = vec![1, 2];
        assert!(same.edited().is_empty());
    }
}
