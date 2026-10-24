//! The Job Finalization card without Slint: the target folder (the source's folder or a named one), the typed name,
//! the counter with its auto-increment, and the file name they make with the naming template.

use std::path::{Path, PathBuf};

use rustcut_core::job::{TargetNaming, TargetSpec};
use rustcut_core::naming::{NameContext, NameError, file_name, render_stem, same_path, used_variables};
use rustcut_core::render::{CodecFamily, Container, RenderProfile};
use rustcut_core::settings::{ExportPanelState, ExportSettings, TargetFolder};
use rustcut_core::time::Timestamp;

/// The entry for "next to the source" in the folder list (always first; user decision Q-X3).
pub const SOURCE_FOLDER: &str = "Source folder";
/// Highest counter value.
pub const MAX_COUNTER: u32 = 999_999;

/// What the job will be, for the template's variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFacts {
    pub profile: String,
    pub codec: &'static str,
    pub container: Container,
    pub sections: usize,
    pub duration: Timestamp,
}

impl JobFacts {
    pub fn new(profile: &RenderProfile, sections: usize, duration: Timestamp) -> Self {
        Self {
            profile: profile.name.clone(),
            codec: codec_label(profile.video.encoder.spec().family),
            container: profile.container,
            sections,
            duration,
        }
    }
}

/// The short codec name of the template variable `codec`.
pub const fn codec_label(family: CodecFamily) -> &'static str {
    match family {
        CodecFamily::Av1 => "AV1",
        CodecFamily::Hevc => "HEVC",
        CodecFamily::H264 => "H.264",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportState {
    pub folders: Vec<TargetFolder>,
    /// `None` = the source's folder, else an index into `folders`.
    pub folder: Option<usize>,
    pub source: Option<PathBuf>,
    pub name: String,
    pub counter_enabled: bool,
    pub counter: u32,
    pub auto_increment: bool,
    pub clear_sections: bool,
    pub template: String,
    /// The name is the file name as it is, without the template (a loaded job whose name the template cannot make
    /// again); typing a name or switching the counter ends it.
    pub verbatim: bool,
}

/// What the card had before a job was loaded into the editor (user decision Q-Q3, "set aside").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDraft {
    pub folder: Option<usize>,
    pub name: String,
    pub counter_enabled: bool,
    pub counter: u32,
    pub verbatim: bool,
}

impl ExportState {
    pub fn new(settings: &ExportSettings, panel: &ExportPanelState) -> Self {
        let folder = panel.target_folder.filter(|i| *i < settings.target_folders.len());
        Self {
            folders: settings.target_folders.clone(),
            folder,
            source: None,
            name: String::new(),
            counter_enabled: panel.counter_enabled,
            counter: panel.counter.clamp(1, MAX_COUNTER),
            auto_increment: panel.auto_increment,
            clear_sections: panel.clear_sections_after_save,
            template: settings.name_template.clone(),
            verbatim: false,
        }
    }

    /// The named folders or the template changed: the selection stays on the same folder (by path) when it is still
    /// listed, else it goes back to the source folder. `true` = the target folder or the name changed.
    pub fn set_settings(&mut self, settings: &ExportSettings) -> bool {
        let mut changed = false;
        if self.folders != settings.target_folders {
            let before = self.target_folder();
            self.folder =
                crate::ui::settings::folders::follow_selection(&self.folders, &settings.target_folders, self.folder);
            self.folders.clone_from(&settings.target_folders);
            changed = self.target_folder() != before;
        }
        if self.template != settings.name_template {
            self.template.clone_from(&settings.name_template);
            changed = true;
        }
        changed
    }

    pub fn save_into(&self, panel: &mut ExportPanelState) {
        panel.target_folder = self.folder;
        panel.counter_enabled = self.counter_enabled;
        panel.counter = self.counter;
        panel.auto_increment = self.auto_increment;
        panel.clear_sections_after_save = self.clear_sections;
    }

    /// Another video: the name becomes its file name, and the counter starts at 1 again (user decision Q-X4).
    pub fn file_opened(&mut self, path: &Path) {
        self.source = Some(path.to_path_buf());
        self.name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.counter = 1;
        self.verbatim = false;
    }

    pub fn draft(&self) -> ExportDraft {
        ExportDraft {
            folder: self.folder,
            name: self.name.clone(),
            counter_enabled: self.counter_enabled,
            counter: self.counter,
            verbatim: self.verbatim,
        }
    }

    /// Puts a draft back after its video opened again (a folder that is gone falls back to the source's folder).
    pub fn put_draft(&mut self, draft: ExportDraft) {
        self.folder = draft.folder.filter(|i| *i < self.folders.len());
        self.name = draft.name;
        self.counter_enabled = draft.counter_enabled;
        self.counter = draft.counter.clamp(1, MAX_COUNTER);
        self.verbatim = draft.verbatim;
    }

    /// The target of a job loaded into the editor (after its source opened): its folder (added to the named folders
    /// when it is neither the source's folder nor one of them; `true` = the list changed), and the name and counter it
    /// was saved with, as long as the template makes the same file name from them; else the file name itself.
    pub fn load_target(&mut self, target: &TargetSpec, facts: &JobFacts) -> bool {
        let source_folder = self.source.as_deref().and_then(Path::parent);
        let mut added = false;
        self.folder = if source_folder.is_some_and(|f| same_path(f, &target.folder)) {
            None
        } else if let Some(i) = self.folders.iter().position(|f| same_path(&f.path, &target.folder)) {
            Some(i)
        } else {
            added = self.add_folder(&target.folder);
            self.folder
        };
        self.verbatim = false;
        if let Some(naming) = &target.naming {
            self.name.clone_from(&naming.name);
            self.counter_enabled = naming.counter.is_some();
            if let Some(n) = naming.counter {
                self.set_counter(n);
            }
            if self.file(facts).is_ok_and(|(stem, _)| stem == target.stem) {
                return added;
            }
        }
        self.name.clone_from(&target.stem);
        self.counter_enabled = false;
        if self.file(facts).is_ok_and(|(stem, _)| stem == target.stem) {
            return added;
        }
        self.verbatim = true;
        added
    }

    /// What the job's target remembers of the card (to show it again when the job is loaded).
    pub fn naming(&self, counter: Option<u32>) -> Option<TargetNaming> {
        (!self.verbatim).then(|| TargetNaming {
            name: self.name.trim().to_owned(),
            counter,
        })
    }

    /// The folder the job writes into: the chosen named folder, else the source's folder.
    pub fn target_folder(&self) -> Option<PathBuf> {
        match self.folder {
            Some(i) => self.folders.get(i).map(|f| f.path.clone()),
            None => self.source.as_deref().and_then(Path::parent).map(Path::to_path_buf),
        }
    }

    /// The entries of the folder list: the source folder, then the named folders.
    pub fn folder_items(&self) -> Vec<String> {
        std::iter::once(SOURCE_FOLDER.to_owned())
            .chain(self.folders.iter().map(|f| f.label.clone()))
            .collect()
    }

    /// The list position of the chosen folder.
    pub fn folder_index(&self) -> usize {
        self.folder.map_or(0, |i| i + 1)
    }

    pub fn select_folder(&mut self, index: usize) {
        self.folder = index.checked_sub(1).filter(|i| *i < self.folders.len());
    }

    /// Adds a named folder (labeled with its own name) and chooses it; a folder already in the list is only chosen.
    /// `true` when the list changed.
    pub fn add_folder(&mut self, path: &Path) -> bool {
        if let Some(i) = self.folders.iter().position(|f| same_path(&f.path, path)) {
            self.folder = Some(i);
            return false;
        }
        let label = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.folders.push(TargetFolder {
            label,
            path: path.to_path_buf(),
        });
        self.folder = Some(self.folders.len() - 1);
        true
    }

    /// Removes the chosen named folder from the list (the folder itself stays); the source folder is chosen.
    pub fn remove_folder(&mut self) -> bool {
        let Some(i) = self.folder.filter(|i| *i < self.folders.len()) else {
            return false;
        };
        self.folders.remove(i);
        self.folder = None;
        true
    }

    pub fn set_counter(&mut self, value: u32) {
        self.counter = value.clamp(1, MAX_COUNTER);
    }

    pub fn step_counter(&mut self, steps: i32) {
        let value = i64::from(self.counter).saturating_add(i64::from(steps));
        self.set_counter(u32::try_from(value.max(1)).unwrap_or(MAX_COUNTER));
    }

    /// The template's variables for counter `counter` (`None` = no counter).
    pub fn context(&self, facts: &JobFacts, counter: Option<u32>) -> NameContext {
        let now = chrono::Local::now();
        let source = self
            .source
            .as_deref()
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let secs = facts.duration.as_micros().max(0) / 1_000_000;
        NameContext {
            name: self.name.trim().to_owned(),
            source,
            counter,
            date: now.format("%Y-%m-%d").to_string(),
            time: now.format("%H-%M-%S").to_string(),
            profile: facts.profile.clone(),
            codec: facts.codec.to_owned(),
            sections: facts.sections,
            duration: format!("{}-{:02}-{:02}", secs / 3600, secs / 60 % 60, secs % 60),
        }
    }

    /// The counter the name uses now.
    pub fn active_counter(&self) -> Option<u32> {
        self.counter_enabled.then_some(self.counter)
    }

    /// The stem with counter `counter`.
    pub fn stem_with(&self, facts: &JobFacts, counter: Option<u32>) -> Result<String, NameError> {
        if self.verbatim {
            return render_stem("{{ name }}", &self.context(facts, None));
        }
        render_stem(&self.template, &self.context(facts, counter))
    }

    /// The stem and the file name of the job as it is set up now.
    pub fn file(&self, facts: &JobFacts) -> Result<(String, String), NameError> {
        let stem = self.stem_with(facts, self.active_counter())?;
        let file = file_name(&stem, facts.container);
        Ok((stem, file))
    }

    /// Whether the template shows the counter (else a different counter cannot free a name).
    pub fn template_uses_counter(&self) -> bool {
        !self.verbatim && used_variables(&self.template).is_ok_and(|v| v.iter().any(|n| n == "counter"))
    }

    /// After a save with counter `used`: the next job gets the next number when auto-increment is on.
    pub fn saved(&mut self, used: Option<u32>) {
        if let Some(n) = used {
            self.counter_enabled = true;
            self.counter = n;
        }
        if self.auto_increment && self.counter_enabled {
            self.set_counter(self.counter.saturating_add(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::render::builtin_profiles;

    use super::*;

    fn state() -> ExportState {
        let settings = ExportSettings {
            target_folders: vec![TargetFolder {
                label: "Clips".into(),
                path: PathBuf::from("/out/clips"),
            }],
            ..ExportSettings::default()
        };
        let panel = ExportPanelState {
            target_folder: Some(5),
            counter_enabled: true,
            counter: 0,
            ..ExportPanelState::default()
        };
        ExportState::new(&settings, &panel)
    }

    fn facts() -> JobFacts {
        JobFacts::new(&builtin_profiles().remove(0), 2, Timestamp::from_secs(3_725))
    }

    #[test]
    fn folders_names_and_counters() {
        let mut st = state();
        assert_eq!(
            (st.folder, st.counter),
            (None, 1),
            "a stale index and a zero counter are repaired"
        );
        assert_eq!(st.target_folder(), None, "no source yet");
        st.file_opened(Path::new("/videos/Holiday.mp4"));
        assert_eq!(st.target_folder(), Some(PathBuf::from("/videos")));
        assert_eq!(st.folder_items(), vec![SOURCE_FOLDER, "Clips"]);
        assert_eq!(st.file(&facts()).unwrap().1, "Holiday - 01.mkv");
        st.select_folder(1);
        assert_eq!(st.target_folder(), Some(PathBuf::from("/out/clips")));
        assert!(
            !st.add_folder(Path::new("/out/./clips")),
            "known folders are only chosen"
        );
        assert!(st.add_folder(Path::new("/media/Archive")));
        assert_eq!((st.folder_index(), st.folders[1].label.as_str()), (2, "Archive"));
        assert!(st.remove_folder());
        assert_eq!((st.folder, st.folders.len()), (None, 1));
        st.saved(None);
        assert_eq!(st.counter, 2, "auto-increment after a save");
        st.saved(Some(7));
        assert_eq!(st.counter, 8, "a number chosen in the conflict dialog continues");
        st.step_counter(-20);
        assert_eq!(st.counter, 1);
        st.file_opened(Path::new("/videos/Next.mkv"));
        assert_eq!((st.name.as_str(), st.counter), ("Next", 1));
        let mut panel = ExportPanelState::default();
        st.save_into(&mut panel);
        assert_eq!(
            (panel.target_folder, panel.counter, panel.counter_enabled),
            (None, 1, true)
        );
    }

    #[test]
    fn a_loaded_target_shows_its_name_and_counter_again() {
        use rustcut_core::job::{TargetNaming, TargetSpec};
        let mut st = state();
        st.file_opened(Path::new("/videos/Holiday.mp4"));
        let target = |folder: &str, stem: &str, naming: Option<TargetNaming>| TargetSpec {
            folder: PathBuf::from(folder),
            stem: stem.into(),
            overwrite: false,
            naming,
        };
        let saved = target(
            "/out/clips",
            "Beach - 03",
            Some(TargetNaming {
                name: "Beach".into(),
                counter: Some(3),
            }),
        );
        assert!(!st.load_target(&saved, &facts()));
        assert_eq!(
            (st.folder, st.name.as_str(), st.active_counter()),
            (Some(0), "Beach", Some(3))
        );
        assert_eq!(st.file(&facts()).unwrap().1, "Beach - 03.mkv");
        assert_eq!(st.naming(st.active_counter()), saved.naming);

        // A job of 0.0.12 (no naming) in the source's folder: the stem is the name.
        assert!(!st.load_target(&target("/videos", "Old - 07", None), &facts()));
        assert_eq!(
            (st.folder, st.name.as_str(), st.active_counter()),
            (None, "Old - 07", None)
        );
        assert_eq!(st.file(&facts()).unwrap().0, "Old - 07");

        // A folder that is not in the list joins it; a template that cannot make the stem keeps it as it is.
        st.template = "{{ name }} {{ codec }}".into();
        assert!(st.load_target(&target("/media/New", "Odd name", None), &facts()));
        assert_eq!(st.target_folder(), Some(PathBuf::from("/media/New")));
        assert!(st.verbatim);
        assert_eq!(st.file(&facts()).unwrap().0, "Odd name");
        assert_eq!(st.naming(None), None);
        assert!(!st.template_uses_counter());

        let draft = st.draft();
        st.file_opened(Path::new("/videos/Other.mp4"));
        assert!(!st.verbatim);
        st.put_draft(draft.clone());
        assert_eq!(st.draft(), draft);
    }

    #[test]
    fn template_variables() {
        let mut st = state();
        st.file_opened(Path::new("/v/Src.mp4"));
        st.name = "  Trip ".into();
        st.template = "{{ name }} {{ codec }} {{ sections }} {{ duration }} {{ source }}".into();
        assert_eq!(st.stem_with(&facts(), None).unwrap(), "Trip AV1 2 1-02-05 Src");
        assert!(!st.template_uses_counter());
        st.counter_enabled = false;
        st.template = rustcut_core::naming::DEFAULT_TEMPLATE.into();
        assert!(st.template_uses_counter());
        assert_eq!(st.file(&facts()).unwrap().0, "Trip");
        st.name = String::new();
        assert!(st.file(&facts()).is_err());
    }
}
