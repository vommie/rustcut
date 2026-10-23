//! The queue panel's filter: state chips (none on = every job) and a search over the job's names, paths, profile,
//! note and failure, both kept in `ui-state.toml` (`UiState.queue`).

use rustcut_core::job::{JobState, JobStateKind};
use rustcut_core::queue::JobSummary;

/// One chip and the states it stands for (Running covers paused renders too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chip {
    pub id: &'static str,
    pub title: &'static str,
    pub kinds: &'static [JobStateKind],
}

pub const CHIPS: [Chip; 6] = [
    Chip {
        id: "waiting",
        title: "Waiting",
        kinds: &[JobStateKind::Waiting],
    },
    Chip {
        id: "running",
        title: "Running",
        kinds: &[JobStateKind::Rendering, JobStateKind::Paused],
    },
    Chip {
        id: "held",
        title: "Held",
        kinds: &[JobStateKind::Held],
    },
    Chip {
        id: "finished",
        title: "Finished",
        kinds: &[JobStateKind::Finished],
    },
    Chip {
        id: "failed",
        title: "Failed",
        kinds: &[JobStateKind::Failed],
    },
    Chip {
        id: "canceled",
        title: "Canceled",
        kinds: &[JobStateKind::Canceled],
    },
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The states of the chips that are on (empty = all).
    pub states: Vec<JobStateKind>,
    pub search: String,
}

impl Filter {
    pub fn new(states: &[JobStateKind], search: &str) -> Self {
        let mut filter = Self {
            states: Vec::new(),
            search: search.to_owned(),
        };
        // A chip is on only with all of its states (a file that holds half a chip shows the whole chip).
        for chip in &CHIPS {
            if chip.kinds.iter().any(|k| states.contains(k)) {
                filter.states.extend_from_slice(chip.kinds);
            }
        }
        filter
    }

    pub fn is_on(&self, chip: &Chip) -> bool {
        chip.kinds.iter().all(|k| self.states.contains(k))
    }

    /// Chip `id` on or off.
    pub fn toggle(&mut self, id: &str) {
        let Some(chip) = CHIPS.iter().find(|c| c.id == id) else {
            return;
        };
        if self.is_on(chip) {
            self.states.retain(|k| !chip.kinds.contains(k));
        } else {
            self.states.extend_from_slice(chip.kinds);
        }
    }

    /// Every chip off and no search.
    pub fn clear(&mut self) {
        self.states.clear();
        self.search.clear();
    }

    pub fn is_active(&self) -> bool {
        !self.states.is_empty() || !self.search.trim().is_empty()
    }

    pub fn matches(&self, row: &JobSummary) -> bool {
        if !self.states.is_empty() && !self.states.contains(&row.state.kind()) {
            return false;
        }
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        let id = row.id.0.to_string();
        let number = needle.strip_prefix('#').unwrap_or(&needle);
        if id == number {
            return true;
        }
        let reason = match &row.state {
            JobState::Failed { reason } => reason.as_str(),
            _ => "",
        };
        [
            row.name.as_str(),
            &row.target.to_string_lossy(),
            &row.source.to_string_lossy(),
            row.profile.as_str(),
            row.note.as_deref().unwrap_or_default(),
            reason,
        ]
        .iter()
        .any(|field| field.to_lowercase().contains(&needle))
    }

    /// Jobs per chip (the chips show their counts).
    pub fn counts(rows: &[JobSummary]) -> [usize; CHIPS.len()] {
        CHIPS.map(|chip| rows.iter().filter(|r| chip.kinds.contains(&r.state.kind())).count())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use rustcut_core::job::JobId;
    use rustcut_core::rustview::SyncState;
    use rustcut_core::time::Timestamp;

    use super::*;

    pub(crate) fn row(id: i64, state: JobState, name: &str) -> JobSummary {
        JobSummary {
            id: JobId(id),
            state,
            attempt: 1,
            name: name.into(),
            target: PathBuf::from(format!("/out/clips/{name}.mkv")),
            source: PathBuf::from("/in/Holiday.mp4"),
            profile: "AV1 balanced".into(),
            sections: 1,
            duration: Timestamp::from_secs(10),
            note: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            sync: SyncState::NotSynced,
        }
    }

    #[test]
    fn chips_and_search() {
        let rows = [
            row(1, JobState::Waiting, "Beach - 01"),
            row(2, JobState::Paused { pass: 1, passes: 1 }, "Beach - 02"),
            row(
                3,
                JobState::Failed {
                    reason: "Encoder refused the bit depth".into(),
                },
                "Hike",
            ),
            row(12, JobState::Finished, "Night"),
        ];
        let shown = |f: &Filter| rows.iter().filter(|r| f.matches(r)).map(|r| r.id.0).collect::<Vec<_>>();
        let mut f = Filter::default();
        assert_eq!(shown(&f), vec![1, 2, 3, 12]);
        assert!(!f.is_active());
        f.toggle("running");
        assert_eq!(shown(&f), vec![2], "paused renders are running");
        f.toggle("waiting");
        assert_eq!(shown(&f), vec![1, 2]);
        f.search = "02".into();
        assert_eq!(shown(&f), vec![2]);
        f.clear();
        f.search = "bit DEPTH".into();
        assert_eq!(shown(&f), vec![3], "the failure is searched, case-insensitive");
        f.search = "#12".into();
        assert_eq!(shown(&f), vec![12]);
        f.search = "holiday".into();
        assert_eq!(shown(&f).len(), 4, "the source is searched");
        assert_eq!(Filter::counts(&rows), [1, 1, 0, 1, 1, 0]);
    }

    #[test]
    fn stored_states_turn_whole_chips_on() {
        let f = Filter::new(&[JobStateKind::Paused, JobStateKind::Failed], "x");
        assert!(f.is_on(&CHIPS[1]));
        assert!(f.states.contains(&JobStateKind::Rendering));
        assert!(f.is_on(&CHIPS[4]));
        assert!(!f.is_on(&CHIPS[0]));
        assert_eq!(f.search, "x");
    }
}
