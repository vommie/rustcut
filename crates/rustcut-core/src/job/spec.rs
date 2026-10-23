//! The versioned description of a job. Old rows stay readable: new fields get serde defaults; reshaped data raises
//! [`JOB_SPEC_VERSION`] and gets a migration step in the store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::edit::{FilterChain, SectionList, Size};
use crate::naming::file_name;
use crate::render::RenderProfile;
use crate::time::{FrameRate, Timestamp};

/// Format version of [`JobSpec`] written by this build.
pub const JOB_SPEC_VERSION: u32 = 1;

const fn version_one() -> u32 {
    1
}

/// Everything needed to render one output file and to describe it in RustView.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    #[serde(default = "version_one")]
    pub version: u32,
    pub source: PathBuf,
    /// Facts about the source at the time the job was made (from the probe).
    #[serde(default)]
    pub source_info: Option<SourceSummary>,
    pub sections: SectionList,
    #[serde(default)]
    pub filters: FilterChain,
    /// A snapshot of the render profile; editing the profile later does not change queued jobs.
    pub profile: RenderProfile,
    pub target: TargetSpec,
    #[serde(default)]
    pub metadata: JobMetadata,
}

impl JobSpec {
    /// The output file: target folder, stem and the profile's container extension.
    pub fn target_path(&self) -> PathBuf {
        self.target
            .folder
            .join(file_name(&self.target.stem, self.profile.container))
    }

    /// Length of the output (sum of the sections).
    pub fn output_duration(&self) -> Timestamp {
        self.sections.total_duration()
    }

    pub fn source_path(&self) -> &Path {
        &self.source
    }
}

/// Where the output goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSpec {
    pub folder: PathBuf,
    /// File name without extension (already rendered from the naming template and sanitized).
    pub stem: String,
    /// The user chose "Overwrite" for an existing file: the render replaces it only after a verified success (ADR-9), and
    /// the RustView record of that file is updated, never deleted (ADR-6).
    #[serde(default)]
    pub overwrite: bool,
}

/// The metadata the job wants in RustView (ADR-6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobMetadata {
    /// RustView tag ids.
    pub tags: Vec<u32>,
    /// 0 to 5; `None` = leave the rating alone.
    pub rating: Option<u8>,
    /// MD5 of the source (hex), registered in RustView's hash list when the job is created.
    pub source_hash: Option<String>,
}

/// What the probe found about the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub size: Size,
    #[serde(default)]
    pub frame_rate: Option<FrameRate>,
    pub duration: Timestamp,
    #[serde(default)]
    pub audio_tracks: u32,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::edit::Section;
    use crate::render::builtin_profiles;

    pub(crate) fn sample() -> JobSpec {
        let section = Section::new(Timestamp::from_secs(1), Timestamp::from_secs(4)).unwrap();
        JobSpec {
            version: JOB_SPEC_VERSION,
            source: PathBuf::from("/videos/in.mp4"),
            source_info: Some(SourceSummary {
                size: Size::new(1920, 1080),
                frame_rate: FrameRate::new(30_000, 1_001),
                duration: Timestamp::from_secs(60),
                audio_tracks: 2,
            }),
            sections: vec![section].into(),
            filters: FilterChain::default(),
            profile: builtin_profiles().remove(0),
            target: TargetSpec {
                folder: PathBuf::from("/videos/out"),
                stem: "Clip - 01".into(),
                overwrite: false,
            },
            metadata: JobMetadata {
                tags: vec![3, 9],
                rating: Some(4),
                source_hash: None,
            },
        }
    }

    #[test]
    fn target_path_uses_the_profile_container() {
        assert_eq!(sample().target_path(), PathBuf::from("/videos/out/Clip - 01.mkv"));
        assert_eq!(sample().output_duration(), Timestamp::from_secs(3));
    }

    #[test]
    fn round_trips_and_reads_minimal_rows() {
        let spec = sample();
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<JobSpec>(&json).unwrap(), spec);
        // The smallest row an old or hand-written database can hold: defaults fill the rest.
        let profile = serde_json::to_string(&spec.profile).unwrap();
        let minimal = format!(
            r#"{{"source":"/a.mkv","sections":[{{"start":0,"end":1000000}}],"profile":{profile},"target":{{"folder":"/o","stem":"a"}}}}"#
        );
        let read: JobSpec = serde_json::from_str(&minimal).unwrap();
        assert_eq!(read.version, 1);
        assert_eq!(read.filters, FilterChain::default());
        assert_eq!(read.metadata, JobMetadata::default());
        assert!(!read.target.overwrite);
    }
}
