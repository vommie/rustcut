//! When the source's audio can be copied unchanged (`AudioCodec::Copy`, user decision Q-N6): only into a job with one
//! section (more sections are joined by a filter graph, which needs decoded audio), and only into a container that
//! takes the source's audio codec. The command planner refuses other jobs, and the Job Finalization card says so
//! before the job is saved.

use super::Container;
use crate::media::MediaInfo;

/// Why the source's audio cannot be copied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioCopyProblem {
    #[error(
        "audio can be copied only for a job with one section (this one has {0}); choose an audio codec in the render profile"
    )]
    Sections(usize),
    #[error(
        "{container} cannot take the source's {codec} audio unchanged; choose an audio codec in the render profile"
    )]
    Container { container: &'static str, codec: String },
}

impl AudioCopyProblem {
    /// The problem in a few words (the Job Finalization card's line).
    pub fn short(&self) -> String {
        match self {
            Self::Sections(_) => "Copied audio needs one section".to_owned(),
            Self::Container { container, codec } => format!("{container} cannot take the {codec} audio unchanged"),
        }
    }
}

/// Why the audio of `source` cannot be copied into a job of `sections` sections written as `container`; `None` when it
/// can, or when the source has no audio.
pub fn audio_copy_problem(container: Container, sections: usize, source: &MediaInfo) -> Option<AudioCopyProblem> {
    let mut tracks = source.audio_streams().peekable();
    tracks.peek()?;
    if sections > 1 {
        return Some(AudioCopyProblem::Sections(sections));
    }
    tracks.find_map(|(stream, _)| {
        let codec = stream.codec.as_deref().unwrap_or("unknown");
        (!container.takes_copied_audio(codec)).then(|| AudioCopyProblem::Container {
            container: container.display_name(),
            codec: codec.to_owned(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AudioStream, Stream, StreamKind};

    fn source(codecs: &[&str]) -> MediaInfo {
        let streams = codecs
            .iter()
            .zip(1..)
            .map(|(codec, index)| Stream {
                index,
                kind: StreamKind::Audio(AudioStream::default()),
                codec: Some((*codec).to_owned()),
                codec_long: None,
                profile: None,
                bit_rate: None,
                duration: None,
                start_time: None,
                language: None,
                title: None,
                default: false,
            })
            .collect();
        MediaInfo {
            streams,
            ..MediaInfo::default()
        }
    }

    #[test]
    fn copying_needs_one_section_and_a_container_that_takes_the_codec() {
        assert_eq!(
            audio_copy_problem(Container::Mkv, 3, &source(&[])),
            None,
            "no audio, nothing to copy"
        );
        assert_eq!(
            audio_copy_problem(Container::Mkv, 2, &source(&["aac"])),
            Some(AudioCopyProblem::Sections(2))
        );
        assert_eq!(audio_copy_problem(Container::Mkv, 1, &source(&["aac", "opus"])), None);
        assert_eq!(audio_copy_problem(Container::Mp4, 1, &source(&["opus"])), None);
        assert_eq!(
            audio_copy_problem(Container::Webm, 1, &source(&["opus", "vorbis"])),
            None
        );
        let webm = audio_copy_problem(Container::Webm, 1, &source(&["opus", "aac"])).unwrap();
        assert_eq!(webm.short(), "WebM (.webm) cannot take the aac audio unchanged");
        assert!(audio_copy_problem(Container::Mov, 1, &source(&["opus"])).is_some());
        assert!(audio_copy_problem(Container::Mp4, 1, &source(&["vorbis"])).is_some());
    }
}
