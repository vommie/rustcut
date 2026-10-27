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
    /// The final size rule (Settings > Filters > Final size, 0.0.60, Q-P9s): the output's width and height are cut
    /// down to multiples of it at the end of the chain (`edit::FINAL_MULTIPLES`, 0 = off). A spec written before reads
    /// 2 (even), the rule those versions applied.
    #[serde(default = "crate::edit::even_multiple")]
    pub final_multiple: u8,
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
    /// What the Export card had when the job was saved, so the editor shows the same name and counter again when the
    /// job is loaded into it. `None` for jobs of 0.0.12 and older (the editor then shows the stem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<TargetNaming>,
}

/// The parts the target's stem was made of (the naming template is a setting and may have changed since).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetNaming {
    /// The name typed in the card (the template's `name`).
    pub name: String,
    /// The counter, when it was on.
    #[serde(default)]
    pub counter: Option<u32>,
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
    /// Saved while the Tag & Rate panel's "Active" switch was off (P8b, user decision Q-P8b): RustView learns nothing
    /// about the job, neither a record nor the source hash, also when it finishes or is edited later.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub without_rustview: bool,
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
            final_multiple: 2,
            profile: builtin_profiles().remove(0),
            target: TargetSpec {
                folder: PathBuf::from("/videos/out"),
                stem: "Clip - 01".into(),
                overwrite: false,
                naming: Some(TargetNaming {
                    name: "Clip".into(),
                    counter: Some(1),
                }),
            },
            metadata: JobMetadata {
                tags: vec![3, 9],
                rating: Some(4),
                source_hash: None,
                without_rustview: false,
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
        assert!(
            !read.metadata.without_rustview,
            "rows of 0.0.32 and older were saved with RustView"
        );
        assert!(!read.target.overwrite);
        assert_eq!(read.target.naming, None, "rows of 0.0.12 have no naming");
    }

    #[test]
    fn a_row_of_0_0_47_with_rotate_and_mirror_reads() {
        let profile = serde_json::to_string(&sample().profile).unwrap();
        let row = format!(
            r#"{{"version":1,"source":"/a.mkv","sections":[{{"start":0,"end":1000000}}],"profile":{profile},
            "target":{{"folder":"/o","stem":"a"}},"filters":[
            {{"enabled":true,"filter":{{"kind":"deinterlace","deinterlacer":"bwdif","rate":"field"}}}},
            {{"enabled":false,"filter":{{"kind":"crop","top":0,"right":0,"bottom":0,"left":0}}}},
            {{"enabled":false,"filter":{{"kind":"deshake","shakiness":5,"accuracy":10,"stepsize":12,"smoothing":10,
              "zoom":0,"optzoom":0,"crop":"black","interpolation":"bicubic","tripod":false}}}},
            {{"enabled":true,"filter":{{"kind":"rotate","rotation":"upside_down"}}}},
            {{"enabled":false,"filter":{{"kind":"mirror","horizontal":true,"vertical":false}}}},
            {{"enabled":true,"filter":{{"kind":"resize","width":640,"height":null,"algorithm":"bicubic"}}}}]}}"#
        );
        let read: JobSpec = serde_json::from_str(&row).unwrap();
        assert_eq!(
            read.final_multiple, 2,
            "a spec of 0.0.59 and older keeps the even rule it rendered with"
        );
        let render = read
            .filters
            .to_lavfi(crate::edit::LavfiTarget::Render {
                source: Size::new(1920, 1080),
                deshake: crate::edit::DeshakePass::None,
                multiple: read.final_multiple,
            })
            .unwrap();
        assert_eq!(
            render, "bwdif=mode=send_field,hflip,vflip,scale=w=640:h=-2:flags=bicubic,setsar=1",
            "the same filters as 0.0.47 rendered"
        );
        let deshake = read
            .filters
            .get(crate::edit::FilterKind::Deshake)
            .map(|slot| slot.filter);
        assert!(
            matches!(deshake, Some(crate::edit::Filter::Deshake(d)) if !d.sharpen),
            "a spec written before 0.0.59 does not sharpen (it renders as it did)"
        );
    }

    #[test]
    fn a_profile_snapshot_of_0_0_8_still_reads() {
        // What 0.0.8 stored: no AQ mode, scene cuts, variance boost or color fields.
        let row = r#"{"source":"/a.mkv","sections":[{"start":0,"end":1000000}],"target":{"folder":"/o","stem":"a"},
            "profile":{"id":"av1-balanced","name":"AV1 balanced","container":"mkv","video":{"encoder":"svt_av1",
            "rate_control":{"mode":"quality","value":26},"preset":"6","tune":"0","profile":null,"level":null,
            "bit_depth":"ten","keyint":null,"film_grain":8,"codec_params":[{"key":"enable-variance-boost","value":"1"},
            {"key":"variance-boost-strength","value":"2"}]},"audio":{"codec":"opus","bitrate_kbps":128,"channels":null,
            "sample_rate":null}}}"#;
        let read: JobSpec = serde_json::from_str(row).unwrap();
        let v = &read.profile.video;
        assert_eq!(
            (v.aq_mode.as_deref(), v.scene_cuts, v.variance_boost),
            (None, None, None)
        );
        assert_eq!(v.color, crate::render::ColorSettings::default());
        assert_eq!(v.codec_params.len(), 2);
        assert!(read.profile.is_usable(), "{:?}", read.profile.validate());
    }
}
