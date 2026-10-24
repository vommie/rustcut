//! What keeps the job itself (not its target) from being saved: an error in the render settings, or audio that cannot be
//! copied for this job (user decision Q-N6). Each problem names the option that solves it, for the card's jump mark.

use rustcut_core::media::MediaInfo;
use rustcut_core::render::{AudioCodec, RenderProfile, Severity, audio_copy_problem};

use crate::ui::jump::JumpTarget;
use crate::ui::render_settings::rows::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobProblem {
    /// The card's line.
    pub text: String,
    /// The whole explanation (the line's tooltip).
    pub tooltip: String,
    /// The option that solves it.
    pub jump: JumpTarget,
}

/// The first problem of a job made with `profile` from `sections` sections of `source` (`None` while the source is not
/// probed yet: the audio check waits for it).
pub fn job_problem(profile: &RenderProfile, sections: usize, source: Option<&MediaInfo>) -> Option<JobProblem> {
    if let Some(issue) = profile.validate().into_iter().find(|i| i.severity == Severity::Error) {
        return Some(JobProblem {
            text: "The render settings have an error".to_owned(),
            tooltip: sentence(&issue.message),
            jump: JumpTarget::Codecs(Row::for_field(issue.field).unwrap_or(Row::Encoder)),
        });
    }
    if profile.audio.codec == AudioCodec::Copy
        && let Some(problem) = source.and_then(|s| audio_copy_problem(profile.container, sections, s))
    {
        return Some(JobProblem {
            text: problem.short(),
            tooltip: sentence(&problem.to_string()),
            jump: JumpTarget::Codecs(Row::AudioCodec),
        });
    }
    None
}

/// `message` with a capital letter first.
fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use rustcut_core::media::{AudioStream, Stream, StreamKind};
    use rustcut_core::render::{Container, builtin_profiles};

    use super::*;

    fn source(codec: &str) -> MediaInfo {
        MediaInfo {
            streams: vec![Stream {
                index: 1,
                kind: StreamKind::Audio(AudioStream::default()),
                codec: Some(codec.to_owned()),
                codec_long: None,
                profile: None,
                bit_rate: None,
                duration: None,
                start_time: None,
                language: None,
                title: None,
                default: false,
            }],
            ..MediaInfo::default()
        }
    }

    #[test]
    fn copied_audio_needs_one_section_and_leads_to_the_audio_codec() {
        let mut profile = builtin_profiles().into_iter().next().unwrap();
        assert_eq!(job_problem(&profile, 3, Some(&source("aac"))), None);
        profile.audio.codec = AudioCodec::Copy;
        assert_eq!(job_problem(&profile, 1, Some(&source("aac"))), None);
        assert_eq!(job_problem(&profile, 2, None), None, "the check waits for the probe");
        let problem = job_problem(&profile, 2, Some(&source("aac"))).unwrap();
        assert_eq!(problem.text, "Copied audio needs one section");
        assert!(
            problem
                .tooltip
                .starts_with("Audio can be copied only for a job with one section (this one has 2)")
        );
        assert_eq!(problem.jump, JumpTarget::Codecs(Row::AudioCodec));
        profile.container = Container::Webm;
        let problem = job_problem(&profile, 1, Some(&source("aac"))).unwrap();
        assert_eq!(problem.text, "WebM (.webm) cannot take the aac audio unchanged");
    }

    #[test]
    fn a_render_settings_error_leads_to_its_row() {
        let mut profile = builtin_profiles().into_iter().next().unwrap();
        profile.container = Container::Mov;
        let problem = job_problem(&profile, 1, None).unwrap();
        assert_eq!(problem.text, "The render settings have an error");
        assert_eq!(
            problem.tooltip,
            "QuickTime (.mov) cannot carry Opus audio; use MP4 or MKV"
        );
        assert_eq!(problem.jump, JumpTarget::Codecs(Row::Container));
    }
}
