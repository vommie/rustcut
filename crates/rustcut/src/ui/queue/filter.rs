//! The queue panel's filter (the filter menu, P9.6): the states, target folders and render profiles to show (none =
//! every job; several values of one kind = any of them) and a search over the job's names, paths, profile, note and
//! failure. The kinds that are set combine with ALL (a job matches every one) or ANY (one is enough; Q-P9). Everything
//! is kept in `ui-state.toml` (`UiState.queue`).

use std::collections::BTreeMap;
use std::path::Path;

use rustcut_core::job::{JobState, JobStateKind};
use rustcut_core::queue::JobSummary;

/// One state choice and the states it stands for (Running covers paused renders too).
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

/// The three choices of the filter menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    State,
    Folder,
    Profile,
}

impl FilterKind {
    pub const ALL: [Self; 3] = [Self::State, Self::Folder, Self::Profile];

    /// The choice's first entry, which shows every job.
    pub const fn everything(self) -> &'static str {
        match self {
            Self::State => "Any state",
            Self::Folder => "Any target folder",
            Self::Profile => "Any profile",
        }
    }

    /// "3 states" for the summary of several values.
    const fn plural(self) -> &'static str {
        match self {
            Self::State => "states",
            Self::Folder => "folders",
            Self::Profile => "profiles",
        }
    }
}

/// One entry of a choice: the value it filters by, what it shows, how many jobs have it, whether it is picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterChoice {
    pub value: String,
    pub label: String,
    pub count: usize,
    pub on: bool,
    /// The state whose symbol and color the entry shows (`text::kind_number`; none for folders and profiles).
    pub tone: Option<JobStateKind>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The states of the picked state choices (empty = all).
    pub states: Vec<JobStateKind>,
    /// Target folders (the whole path) and profile names to show (empty = all).
    pub folders: Vec<String>,
    pub profiles: Vec<String>,
    pub search: String,
    /// The kinds that are set combine with ANY instead of ALL.
    pub any: bool,
}

impl Filter {
    pub fn new(states: &[JobStateKind], search: &str) -> Self {
        let mut filter = Self {
            states: Vec::new(),
            search: search.to_owned(),
            ..Self::default()
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

    /// The filter `ui-state.toml` kept: states, folders, profiles, search and the rule.
    pub fn restore(states: &[JobStateKind], folders: &[String], profiles: &[String], search: &str, any: bool) -> Self {
        Self {
            folders: folders.to_vec(),
            profiles: profiles.to_vec(),
            any,
            ..Self::new(states, search)
        }
    }

    /// Nothing filtered and no search (the rule stays).
    pub fn clear(&mut self) {
        self.states.clear();
        self.folders.clear();
        self.profiles.clear();
        self.search.clear();
    }

    pub fn is_active(&self) -> bool {
        self.active_count() > 0
    }

    /// How many kinds filter (the badge on the filter button): states, folders, profiles, the search.
    pub fn active_count(&self) -> usize {
        [
            !self.states.is_empty(),
            !self.folders.is_empty(),
            !self.profiles.is_empty(),
            !self.search.trim().is_empty(),
        ]
        .iter()
        .filter(|on| **on)
        .count()
    }

    pub fn matches(&self, row: &JobSummary) -> bool {
        let tests = [
            (!self.states.is_empty()).then(|| self.states.contains(&row.state.kind())),
            (!self.folders.is_empty()).then(|| self.folders.contains(&folder_of(row))),
            (!self.profiles.is_empty()).then(|| self.profiles.contains(&row.profile)),
            (!self.search.trim().is_empty()).then(|| self.found(row)),
        ];
        let mut set = tests.into_iter().flatten().peekable();
        if set.peek().is_none() {
            return true;
        }
        if self.any {
            set.any(|hit| hit)
        } else {
            set.all(|hit| hit)
        }
    }

    /// Whether the search finds `row`.
    fn found(&self, row: &JobSummary) -> bool {
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

    /// Jobs per state choice.
    pub fn counts(rows: &[JobSummary]) -> [usize; CHIPS.len()] {
        CHIPS.map(|chip| rows.iter().filter(|r| chip.kinds.contains(&r.state.kind())).count())
    }

    /// The entries of the choice `kind` for `rows`: every state; the folders and profiles the jobs have, sorted, and
    /// the picked ones no job has any more (so they can be taken off).
    pub fn choices(&self, kind: FilterKind, rows: &[JobSummary]) -> Vec<FilterChoice> {
        match kind {
            FilterKind::State => CHIPS
                .iter()
                .zip(Self::counts(rows))
                .map(|(chip, count)| FilterChoice {
                    value: chip.id.to_owned(),
                    label: chip.title.to_owned(),
                    count,
                    on: self.is_on(chip),
                    tone: chip.kinds.first().copied(),
                })
                .collect(),
            FilterKind::Folder | FilterKind::Profile => {
                let (picked, value): (&[String], fn(&JobSummary) -> String) = if kind == FilterKind::Folder {
                    (&self.folders, folder_of)
                } else {
                    (&self.profiles, |r| r.profile.clone())
                };
                let mut counts: BTreeMap<String, usize> = picked.iter().map(|v| (v.clone(), 0)).collect();
                for row in rows {
                    *counts.entry(value(row)).or_default() += 1;
                }
                counts.remove("");
                let labels = if kind == FilterKind::Folder {
                    folder_labels(counts.keys())
                } else {
                    counts.keys().cloned().collect()
                };
                counts
                    .into_iter()
                    .zip(labels)
                    .map(|((value, count), label)| FilterChoice {
                        on: picked.contains(&value),
                        value,
                        label,
                        count,
                        tone: None,
                    })
                    .collect()
            }
        }
    }

    /// The entry `value` of the choice `kind` was clicked: alone it becomes the only one, with Ctrl it is added or
    /// taken off. `None` = the first entry ("Any ..."): that kind filters nothing.
    pub fn pick(&mut self, kind: FilterKind, value: Option<&str>, control: bool) {
        let Some(value) = value else {
            match kind {
                FilterKind::State => self.states.clear(),
                FilterKind::Folder => self.folders.clear(),
                FilterKind::Profile => self.profiles.clear(),
            }
            return;
        };
        match kind {
            FilterKind::State => {
                if !control {
                    self.states.clear();
                }
                self.toggle(value);
            }
            FilterKind::Folder | FilterKind::Profile => {
                let values = if kind == FilterKind::Folder {
                    &mut self.folders
                } else {
                    &mut self.profiles
                };
                let had = values.iter().any(|v| v == value);
                if !control {
                    values.clear();
                    values.push(value.to_owned());
                } else if had {
                    values.retain(|v| v != value);
                } else {
                    values.push(value.to_owned());
                }
            }
        }
    }

    /// What the choice `kind` shows: "Any state", the one picked label, or "3 states".
    pub fn summary(choices: &[FilterChoice], kind: FilterKind) -> String {
        let on: Vec<&FilterChoice> = choices.iter().filter(|c| c.on).collect();
        match on.as_slice() {
            [] => kind.everything().to_owned(),
            [one] => one.label.clone(),
            several => format!("{} {}", several.len(), kind.plural()),
        }
    }
}

/// The target folder of a job (the whole path).
pub fn folder_of(row: &JobSummary) -> String {
    row.target
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folders' names; folders of the same name show their parent too ("trip/clips", "work/clips").
fn folder_labels<'a>(folders: impl Iterator<Item = &'a String>) -> Vec<String> {
    let folders: Vec<&String> = folders.collect();
    let name = |path: &str, parts: usize| {
        let components: Vec<String> = Path::new(path)
            .components()
            .rev()
            .take(parts)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        components.into_iter().rev().collect::<Vec<_>>().join("/")
    };
    folders
        .iter()
        .map(|path| {
            let short = name(path, 1);
            let twins = folders.iter().filter(|other| name(other, 1) == short).count();
            if twins > 1 { name(path, 2) } else { short }
        })
        .collect()
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
            relocation: None,
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
    fn folders_profiles_and_the_rule() {
        let mut other = row(5, JobState::Waiting, "Trip");
        other.target = PathBuf::from("/trip/clips/Trip.mkv");
        other.profile = "x265".into();
        let rows = [row(1, JobState::Waiting, "a"), row(2, JobState::Finished, "b"), other];
        let shown = |f: &Filter| rows.iter().filter(|r| f.matches(r)).map(|r| r.id.0).collect::<Vec<_>>();
        let mut f = Filter::default();
        let folders = f.choices(FilterKind::Folder, &rows);
        assert_eq!(
            folders.iter().map(|c| (c.label.as_str(), c.count)).collect::<Vec<_>>(),
            [("out/clips", 2), ("trip/clips", 1)],
            "two folders called clips show their parents"
        );
        f.pick(FilterKind::Folder, Some("/trip/clips"), false);
        assert_eq!(shown(&f), vec![5]);
        f.pick(FilterKind::Folder, Some("/out/clips"), true);
        assert_eq!(shown(&f), vec![1, 2, 5], "Ctrl adds a folder");
        f.pick(FilterKind::State, Some("finished"), false);
        assert_eq!(shown(&f), vec![2], "ALL: in one of the folders and finished");
        f.any = true;
        assert_eq!(shown(&f), vec![1, 2, 5], "ANY: one of the kinds is enough");
        f.pick(FilterKind::Folder, None, false);
        f.pick(FilterKind::Profile, Some("x265"), false);
        assert_eq!(shown(&f), vec![2, 5]);
        assert_eq!(f.active_count(), 2);
        let profiles = f.choices(FilterKind::Profile, &rows);
        assert_eq!(Filter::summary(&profiles, FilterKind::Profile), "x265");
        f.pick(FilterKind::Profile, Some("AV1 balanced"), true);
        let profiles = f.choices(FilterKind::Profile, &rows);
        assert_eq!(Filter::summary(&profiles, FilterKind::Profile), "2 profiles");
        f.pick(FilterKind::Profile, Some("gone"), true);
        assert!(
            f.choices(FilterKind::Profile, &rows)
                .iter()
                .any(|c| c.value == "gone" && c.count == 0 && c.on),
            "a picked profile no job has stays so it can be taken off"
        );
        f.clear();
        assert!(!f.is_active() && f.any, "clearing keeps the rule");
        assert_eq!(
            Filter::summary(&f.choices(FilterKind::State, &rows), FilterKind::State),
            "Any state"
        );
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
