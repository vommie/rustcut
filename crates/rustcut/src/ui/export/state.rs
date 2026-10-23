//! The Job Finalization card without Slint: the target folder (the source's folder or a named one), the typed name,
//! the counter with its auto-increment, and the file name they make with the naming template.

use std::path::{Path, PathBuf};

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
        }
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
        used_variables(&self.template).is_ok_and(|v| v.iter().any(|n| n == "counter"))
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
