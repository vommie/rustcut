//! A queued job loaded into the editor, without Slint (user decisions Q-Q2, Q-Q3, Q-N10): which job it is, what its
//! target was, whether saving can update it (a waiting or held job whose target file stays the same) or makes a new
//! job, and whether RustCut held it for the editing.

use std::path::{Path, PathBuf};

use rustcut_core::job::{JobId, JobSpec, JobStateKind};
use rustcut_core::naming::same_path;

#[derive(Debug, Clone)]
pub struct JobEdit {
    pub id: JobId,
    /// The target's file name (the card's banner).
    pub label: String,
    pub target: PathBuf,
    pub overwrite: bool,
    /// What the job was when it was loaded, for what a save keeps when the panels do not know it (yet).
    pub spec: Box<JobSpec>,
    /// It was waiting or held: saving with the same target updates it.
    pub updatable: bool,
    /// RustCut held it for the editing (it was waiting): it waits again when the editing ends.
    pub release: bool,
}

/// What a save while a job is edited does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAs {
    Update(JobId),
    New,
}

impl JobEdit {
    /// Starts the editing of job `id` that is in `state`.
    pub fn new(id: JobId, state: JobStateKind, spec: Box<JobSpec>) -> Self {
        let target = spec.target_path();
        Self {
            id,
            label: target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            overwrite: spec.target.overwrite,
            target,
            spec,
            updatable: matches!(state, JobStateKind::Waiting | JobStateKind::Held),
            release: state == JobStateKind::Waiting,
        }
    }

    /// A save that writes `target`: the job is updated only while it writes the same file (Q-Q2: another name, counter,
    /// folder or container makes a new job).
    pub fn save_as(&self, target: &Path, force_new: bool) -> SaveAs {
        if self.updatable && !force_new && same_path(target, &self.target) {
            SaveAs::Update(self.id)
        } else {
            SaveAs::New
        }
    }

    /// The card's banner.
    pub fn banner(&self) -> String {
        if self.updatable {
            format!("Editing job {}: {}", self.id, self.label)
        } else {
            format!("Job {} loaded as a new job: {}", self.id, self.label)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::edit::{CropAspect, Filter, FilterChain, FilterKind, Rotation, Section};
    use rustcut_core::job::{JOB_SPEC_VERSION, JobMetadata, TargetNaming, TargetSpec};
    use rustcut_core::render::{DEFAULT_PROFILE_ID, ProfileLibrary, RateControl, Selection};
    use rustcut_core::settings::{ExportPanelState, ExportSettings, FilterDefaults, TagPanelState, TargetFolder};
    use rustcut_core::time::Timestamp;

    use super::*;
    use crate::ui::export::{ExportState, JobFacts};
    use crate::ui::filters::state::FiltersState;
    use crate::ui::sections::SectionsState;
    use crate::ui::tags::{Choice, TagPanel};

    fn waiting_job() -> JobSpec {
        let s = |a, b| Section::new(Timestamp::from_secs(a), Timestamp::from_secs(b)).unwrap();
        let mut filters = FilterChain::default();
        filters.set(Filter::Rotate {
            rotation: Rotation::CounterClockwise,
        });
        filters.set_enabled(FilterKind::Rotate, true);
        filters.move_up(FilterKind::Rotate);
        let library = ProfileLibrary::new(Vec::new());
        let mut profile = library.get("x265-quality").unwrap().clone();
        profile.video.rate_control = RateControl::Quality { value: 19 };
        JobSpec {
            version: JOB_SPEC_VERSION,
            source: PathBuf::from("/videos/Holiday.mp4"),
            source_info: None,
            sections: vec![s(30, 45), s(2, 9)].into(),
            filters,
            profile,
            target: TargetSpec {
                folder: PathBuf::from("/out/clips"),
                stem: "Beach - 04".into(),
                overwrite: false,
                naming: Some(TargetNaming {
                    name: "Beach".into(),
                    counter: Some(4),
                }),
            },
            metadata: JobMetadata {
                tags: vec![2, 7],
                rating: Some(3),
                source_hash: None,
                without_rustview: false,
            },
        }
    }

    /// The editor round trip of a waiting job: its spec loaded into the panels' states and saved again (as the export
    /// builds a job from them) is the same spec, so "Update job" without a change changes nothing.
    #[test]
    fn a_waiting_job_survives_the_editor_unchanged() {
        let job = waiting_job();
        let library = ProfileLibrary::new(Vec::new());

        let mut sections = SectionsState::default();
        sections.set_mark(crate::ui::sections::Bound::Start, Timestamp::from_secs(1));
        sections.clear();
        sections.load(job.sections.clone());
        sections.set_duration(Some(Timestamp::from_secs(600)));

        let mut filters = FiltersState::new(FilterDefaults::default(), true, false, None, CropAspect::default());
        filters.file_opened();
        filters.load_chain(job.filters.clone());

        let mut render = Selection::restore(&library, None, None, DEFAULT_PROFILE_ID);
        render.load(&library, &job.profile);

        let settings = ExportSettings {
            target_folders: vec![TargetFolder {
                label: "Clips".into(),
                path: PathBuf::from("/out/clips"),
            }],
            ..ExportSettings::default()
        };
        let mut export = ExportState::new(&settings, &ExportPanelState::default());
        export.file_opened(&job.source);
        let facts = JobFacts::new(&render.working, sections.list().len(), sections.list().total_duration());
        assert!(!export.load_target(&job.target, &facts));

        let mut tags = TagPanel::new(&TagPanelState::default());
        tags.load_draft(Choice {
            tags: job.metadata.tags.clone(),
            rating: job.metadata.rating,
        });

        // What the export's save builds from the panels.
        let profile = render.working.clone();
        let list = sections.list().clone();
        let facts = JobFacts::new(&profile, list.len(), list.total_duration());
        let (stem, _) = export.file(&facts).unwrap();
        let (tag_ids, rating) = tags.draft_metadata();
        let saved = JobSpec {
            version: JOB_SPEC_VERSION,
            source: job.source.clone(),
            source_info: None,
            sections: list,
            filters: filters.chain().clone(),
            profile,
            target: TargetSpec {
                folder: export.target_folder().unwrap(),
                stem,
                overwrite: false,
                naming: export.naming(export.active_counter()),
            },
            metadata: JobMetadata {
                tags: tag_ids,
                rating,
                source_hash: None,
                without_rustview: false,
            },
        };
        assert_eq!(saved, job);

        let edit = JobEdit::new(JobId(4), JobStateKind::Waiting, Box::new(job.clone()));
        assert_eq!(edit.save_as(&saved.target_path(), false), SaveAs::Update(JobId(4)));
        assert!(edit.release);
        assert_eq!(edit.banner(), "Editing job #4: Beach - 04.mkv");

        // Another counter number is another file: a new job (Q-Q2), as is "Save as new".
        export.set_counter(5);
        let (stem, file) = export.file(&facts).unwrap();
        assert_eq!(stem, "Beach - 05");
        assert_eq!(
            edit.save_as(&PathBuf::from("/out/clips").join(file), false),
            SaveAs::New
        );
        assert_eq!(edit.save_as(&saved.target_path(), true), SaveAs::New);
    }

    #[test]
    fn done_and_held_jobs() {
        let job = Box::new(waiting_job());
        let done = JobEdit::new(JobId(1), JobStateKind::Finished, job.clone());
        assert_eq!(
            done.save_as(&job.target_path(), false),
            SaveAs::New,
            "a done job is only a template"
        );
        assert!(!done.release);
        assert!(done.banner().starts_with("Job #1 loaded as a new job"));
        let held = JobEdit::new(JobId(2), JobStateKind::Held, job.clone());
        assert_eq!(held.save_as(&job.target_path(), false), SaveAs::Update(JobId(2)));
        assert!(!held.release, "it was held by the user: it stays held");
    }
}
