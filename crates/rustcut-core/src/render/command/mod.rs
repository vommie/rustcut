//! The ffmpeg command lines of a job (pure; ADR-3, ADR-8, ADR-9): one input per section with accurate input seeking, the
//! `concat` graph that keeps every audio track, the job's filter chain, the profile's encoder arguments, and the hidden
//! temp file next to the target. A deshake job gets an analysis pass first, a two-pass profile a first video pass.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{ArgsError, AudioCodec, BitDepth, EncodeContext, EncoderBackend, VideoPass, profile_args, video_args};
use crate::edit::{ChainError, ConcatInput, DeshakePass, LavfiTarget, Section, Size, concat_graph};
use crate::job::{JobId, JobSpec};
use crate::media::{MediaInfo, StreamKind};
use crate::naming::{file_name, same_path};
use crate::time::Timestamp;

#[cfg(test)]
mod tests;

/// Middle part of the temp file name: `.<stem>.rustcut-part.<ext>` (ADR-9).
pub const TEMP_MARKER: &str = "rustcut-part";

/// Where the passes keep their side files, and the hardware the encoders may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDirs {
    /// vid.stab transform files: `<dir>/job-<id>.trf`.
    pub deshake: PathBuf,
    /// Two-pass statistics: `<dir>/job-<id>/`.
    pub passlog: PathBuf,
}

/// What the capability check found about hardware encoding (ADR-14).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardwareContext {
    /// The DRM render node VAAPI encoders work on (`/dev/dri/renderD128`).
    pub vaapi_device: Option<PathBuf>,
}

/// Every frame keeps its source timestamp in every container. ffmpeg's default (`auto`) decides per muxer, and many
/// versions made MP4 constant-rate that way, duplicating and dropping frames of a variable-frame-rate source (screen and
/// phone recordings); ffmpeg 9 keeps them, the explicit mode keeps it so. The first video pass gets it too, so both
/// passes see the same frames.
const FPS_MODE: [&str; 2] = ["-fps_mode:v", "vfr"];

/// Everything the builder needs besides the job.
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub job: JobId,
    pub spec: &'a JobSpec,
    /// The source as probed when the render starts (the file may have changed since the job was made).
    pub source: &'a MediaInfo,
    pub dirs: &'a WorkDirs,
    pub hardware: &'a HardwareContext,
}

/// What one ffmpeg run of a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassKind {
    /// vid.stab's motion analysis (`vidstabdetect`); the frames are discarded.
    DeshakeAnalysis,
    /// The first pass of a two-pass encode; only the statistics are kept.
    FirstVideoPass,
    /// The pass that writes the temp file.
    Encode,
}

impl PassKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DeshakeAnalysis => "deshake analysis",
            Self::FirstVideoPass => "first video pass",
            Self::Encode => "encode",
        }
    }
}

/// One ffmpeg run: its kind, the arguments after the program name, and the output time it reaches at the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPass {
    pub kind: PassKind,
    pub args: Vec<OsString>,
    pub duration: Timestamp,
}

/// What a finished render must contain (checked with ffprobe before the rename, ADR-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expectation {
    pub duration: Timestamp,
    pub audio_streams: usize,
    pub size: Size,
}

/// The whole render of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    pub target: PathBuf,
    /// The hidden file ffmpeg writes; renamed onto `target` after the verification.
    pub temp: PathBuf,
    pub passes: Vec<PlannedPass>,
    pub expected: Expectation,
    /// Side files to delete when the job ends: the transform file and the two-pass statistics folder.
    pub transforms: Option<PathBuf>,
    pub passlog: Option<PathBuf>,
}

impl RenderPlan {
    pub fn pass_count(&self) -> u8 {
        u8::try_from(self.passes.len()).unwrap_or(u8::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("the job has no section")]
    NoSections,
    #[error("every section lies after the end of the source ({0})")]
    SectionsPastEnd(String),
    #[error("the source has no video stream")]
    NoVideo,
    #[error("the size of the source video is unknown")]
    UnknownSize,
    #[error("the source and the target are the same file")]
    SameFile,
    #[error("audio cannot be copied when sections are cut and joined; choose an audio codec in the render profile")]
    AudioCopy,
    #[error("{0} needs a working VAAPI device, and none was found")]
    NoVaapiDevice(&'static str),
    #[error("the filters cannot be rendered: {0}")]
    Chain(#[from] ChainError),
    #[error("the encoder settings cannot be used: {0}")]
    Args(#[from] ArgsError),
}

/// The temp file of `target`: `.<file name without extension>.rustcut-part.<ext>` in the same folder.
pub fn temp_path(folder: &Path, stem: &str, extension: &str) -> PathBuf {
    folder.join(format!(".{stem}.{TEMP_MARKER}.{extension}"))
}

/// Builds every pass of the job.
pub fn plan(input: PlanInput<'_>) -> Result<RenderPlan, PlanError> {
    let spec = input.spec;
    let (sections, duration) = usable_sections(spec, input.source)?;
    let (_, video) = input.source.primary_video().ok_or(PlanError::NoVideo)?;
    let (width, height) = video.oriented_size().ok_or(PlanError::UnknownSize)?;
    // ffmpeg turns the frames by the display rotation before the filters, so the chain sees the oriented size.
    let source_size = Size::new(width, height);
    let target = spec.target_path();
    let extension = spec.profile.container.extension();
    let temp = temp_path(&spec.target.folder, &spec.target.stem, extension);
    if same_path(&spec.source, &target) || same_path(&spec.source, &temp) {
        return Err(PlanError::SameFile);
    }
    let audio_tracks = input.source.audio_streams().count();
    if audio_tracks > 0 && spec.profile.audio.codec == AudioCodec::Copy {
        return Err(PlanError::AudioCopy);
    }
    let encoder = spec.profile.video.encoder.spec();
    let vaapi = match encoder.backend {
        EncoderBackend::Vaapi => Some(
            input
                .hardware
                .vaapi_device
                .as_deref()
                .ok_or(PlanError::NoVaapiDevice(encoder.display_name))?,
        ),
        _ => None,
    };
    let builder = Builder {
        source: &spec.source,
        sections: &sections,
        vaapi,
    };

    let deshake = spec.filters.needs_two_passes();
    let transforms = deshake.then(|| input.dirs.deshake.join(format!("job-{}.trf", input.job.0)));
    let two_pass = spec.profile.video.rate_control.is_two_pass();
    let passlog = two_pass.then(|| input.dirs.passlog.join(format!("job-{}", input.job.0)));
    let stats = passlog.as_ref().map(|dir| dir.join("pass"));
    let render_target = |deshake| LavfiTarget::Render {
        source: source_size,
        deshake,
    };
    let final_pass = transforms
        .as_deref()
        .map_or(DeshakePass::None, |input| DeshakePass::Transform { input });

    let mut passes = Vec::new();
    if let Some(result) = transforms.as_deref() {
        let chain = spec.filters.to_lavfi(render_target(DeshakePass::Detect { result }))?;
        passes.push(PlannedPass {
            kind: PassKind::DeshakeAnalysis,
            args: builder.discarding(&chain, &[]),
            duration,
        });
    }
    let chain = spec.filters.to_lavfi(render_target(final_pass))?;
    let context = |pass| EncodeContext {
        duration,
        audio_streams: audio_tracks,
        pass,
    };
    if let Some(log) = stats.as_deref() {
        let video = video_args(
            &spec.profile.video,
            &context(VideoPass::First { log }),
            &spec.profile.audio,
        )?;
        passes.push(PlannedPass {
            kind: PassKind::FirstVideoPass,
            args: builder.discarding(&builder.encoder_chain(&chain, spec), &video),
            duration,
        });
    }
    let pass = stats
        .as_deref()
        .map_or(VideoPass::Single, |log| VideoPass::Second { log });
    let encode = profile_args(&spec.profile, &context(pass))?;
    passes.push(PlannedPass {
        kind: PassKind::Encode,
        args: builder.encoding(
            &builder.encoder_chain(&chain, spec),
            audio_tracks,
            input.source,
            &encode,
            &temp,
        ),
        duration,
    });
    Ok(RenderPlan {
        target,
        temp,
        passes,
        expected: Expectation {
            duration,
            audio_streams: audio_tracks,
            size: spec.filters.output_size(source_size)?.even_floor(),
        },
        transforms,
        passlog,
    })
}

/// The sections inside the source (cut at its end) and their total length.
fn usable_sections(spec: &JobSpec, source: &MediaInfo) -> Result<(Vec<Section>, Timestamp), PlanError> {
    if spec.sections.is_empty() {
        return Err(PlanError::NoSections);
    }
    let end = source.duration();
    let sections: Vec<Section> = spec
        .sections
        .sections()
        .filter_map(|s| end.map_or(Some(s), |end| s.clamped_to(end)))
        .filter(|s| s.duration() > Timestamp::ZERO)
        .collect();
    if sections.is_empty() {
        let end = end.unwrap_or_default().format(crate::time::Precision::Millis);
        return Err(PlanError::SectionsPastEnd(end));
    }
    let total = sections.iter().fold(Timestamp::ZERO, |acc, s| acc + s.duration());
    Ok((sections, total))
}

/// The argument pieces every pass shares.
struct Builder<'a> {
    source: &'a Path,
    sections: &'a [Section],
    vaapi: Option<&'a Path>,
}

impl Builder<'_> {
    /// Program options, the hardware device, and one accurately seeked input per section.
    fn head(&self) -> Vec<OsString> {
        let mut args = os(&[
            "-hide_banner",
            "-nostats",
            "-progress",
            "pipe:1",
            "-loglevel",
            "level+info",
            "-y",
        ]);
        if let Some(device) = self.vaapi {
            args.push("-vaapi_device".into());
            args.push(device.as_os_str().to_owned());
        }
        for section in self.sections {
            args.extend(os(&[
                "-ss",
                &section.start().to_ffmpeg_arg(),
                "-to",
                &section.end().to_ffmpeg_arg(),
                "-i",
            ]));
            args.push(file_url(self.source));
        }
        args
    }

    /// The video chain plus, for VAAPI encoders, the upload of the frames to the device in the encoder's pixel format.
    fn encoder_chain(&self, chain: &str, spec: &JobSpec) -> String {
        if self.vaapi.is_none() {
            return chain.to_owned();
        }
        let encoder = spec.profile.video.encoder.spec();
        let format = match spec.profile.video.bit_depth {
            BitDepth::Eight => encoder.pix_fmt_8,
            BitDepth::Ten => encoder.pix_fmt_10.unwrap_or(encoder.pix_fmt_8),
        };
        let upload = format!("format={format},hwupload");
        if chain.is_empty() {
            upload
        } else {
            format!("{chain},{upload}")
        }
    }

    /// A pass whose output is thrown away: video only, `extra` encoder arguments, `-f null -`.
    fn discarding(&self, chain: &str, extra: &[String]) -> Vec<OsString> {
        let mut args = self.head();
        let graph = concat_graph(
            ConcatInput {
                sections: self.sections.len(),
                audio_tracks: 0,
            },
            chain,
        );
        if let Some(graph) = graph {
            args.extend(os(&[
                "-filter_complex",
                &graph.graph,
                "-map",
                &format!("[{}]", graph.video),
            ]));
        }
        args.extend(os(&FPS_MODE));
        args.extend(extra.iter().map(OsString::from));
        args.extend(os(&["-an", "-f", "null", "-"]));
        args
    }

    /// The pass that writes `temp`: every audio track, global metadata, track languages and titles, no subtitles or
    /// chapters (ADR-8), the profile's encoder and muxer arguments.
    fn encoding(
        &self,
        chain: &str,
        audio_tracks: usize,
        source: &MediaInfo,
        encode: &[String],
        temp: &Path,
    ) -> Vec<OsString> {
        let mut args = self.head();
        let graph = concat_graph(
            ConcatInput {
                sections: self.sections.len(),
                audio_tracks,
            },
            chain,
        );
        if let Some(graph) = graph {
            args.extend(os(&[
                "-filter_complex",
                &graph.graph,
                "-map",
                &format!("[{}]", graph.video),
            ]));
            for label in &graph.audio {
                args.extend(os(&["-map", &format!("[{label}]")]));
            }
        }
        args.extend(os(&FPS_MODE));
        args.extend(os(&["-map_metadata", "0", "-map_chapters", "-1"]));
        let tracks = source.streams.iter().filter(|s| matches!(s.kind, StreamKind::Audio(_)));
        for (index, stream) in tracks.enumerate() {
            let fields = [("language", &stream.language), ("title", &stream.title)];
            for (key, value) in fields {
                if let Some(value) = value {
                    args.push(format!("-metadata:s:a:{index}").into());
                    args.push(format!("{key}={value}").into());
                }
            }
        }
        args.extend(encode.iter().map(OsString::from));
        args.push(file_url(temp));
        args
    }
}

/// `file:<path>`: ffmpeg never reads a path as a protocol (`concat:`, `pipe:`) or an option.
fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path.as_os_str());
    url
}

fn os(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

/// The file name of a job's output (for logs): `stem.ext`.
pub fn target_file_name(spec: &JobSpec) -> String {
    file_name(&spec.target.stem, spec.profile.container)
}
