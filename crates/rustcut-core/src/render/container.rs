//! Output containers and which codecs they can carry.

use serde::{Deserialize, Serialize};

use super::{AudioCodec, CodecFamily, VideoEncoder};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    /// Matroska carries every codec RustCut offers (PyCutRv's default).
    #[default]
    Mkv,
    Mp4,
    Webm,
    Mov,
}

/// How serious a codec/container combination problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Works, but some players or devices may not play it.
    Warning,
    /// ffmpeg refuses it or the file is not standard.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatIssue {
    pub severity: Severity,
    pub message: String,
}

impl Container {
    pub const ALL: [Self; 4] = [Self::Mkv, Self::Mp4, Self::Webm, Self::Mov];

    /// File extension without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Mov => "mov",
        }
    }

    /// ffmpeg muxer name, given explicitly with `-f` because the render writes to a temporary name (ADR-9).
    pub const fn muxer(self) -> &'static str {
        match self {
            Self::Mkv => "matroska",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Mov => "mov",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Mkv => "Matroska (.mkv)",
            Self::Mp4 => "MP4 (.mp4)",
            Self::Webm => "WebM (.webm)",
            Self::Mov => "QuickTime (.mov)",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.extension().eq_ignore_ascii_case(ext))
    }

    /// Problems of putting `video` and `audio` into this container (empty = fine).
    pub fn check(self, video: VideoEncoder, audio: AudioCodec) -> Vec<CompatIssue> {
        let mut issues = Vec::new();
        let family = video.spec().family;
        let mut push = |severity, message: String| issues.push(CompatIssue { severity, message });
        match self {
            Self::Mkv => {}
            Self::Webm => {
                if family != CodecFamily::Av1 {
                    push(
                        Severity::Error,
                        format!("WebM carries only VP9 and AV1 video, not {}", video.spec().display_name),
                    );
                }
                if !matches!(audio, AudioCodec::Opus | AudioCodec::Vorbis | AudioCodec::Copy) {
                    push(
                        Severity::Error,
                        format!(
                            "WebM carries only Opus and Vorbis audio, not {}",
                            audio.spec().display_name
                        ),
                    );
                }
                if audio == AudioCodec::Copy {
                    push(
                        Severity::Warning,
                        "WebM fails when the copied source audio is not Opus or Vorbis".to_owned(),
                    );
                }
            }
            Self::Mp4 | Self::Mov => {
                if audio == AudioCodec::Vorbis {
                    push(
                        Severity::Error,
                        format!("{} cannot carry Vorbis audio", self.display_name()),
                    );
                }
                if audio == AudioCodec::Opus {
                    push(
                        Severity::Warning,
                        format!(
                            "Opus in {} does not play on many devices; AAC is safer",
                            self.display_name()
                        ),
                    );
                }
                if audio == AudioCodec::Flac {
                    push(
                        Severity::Warning,
                        format!("FLAC in {} does not play on many devices", self.display_name()),
                    );
                }
                if self == Self::Mov && family == CodecFamily::Av1 {
                    push(
                        Severity::Warning,
                        "AV1 in QuickTime (.mov) does not play on many devices; use MP4 or MKV".to_owned(),
                    );
                }
                if audio == AudioCodec::Copy {
                    push(
                        Severity::Warning,
                        format!(
                            "{} fails when the copied source audio is not supported by it",
                            self.display_name()
                        ),
                    );
                }
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mkv_takes_everything() {
        for video in VideoEncoder::ALL {
            for audio in AudioCodec::ALL {
                assert!(Container::Mkv.check(video, audio).is_empty());
            }
        }
    }

    #[test]
    fn webm_and_mp4_rules() {
        let errors = |c: Container, v, a| c.check(v, a).iter().filter(|i| i.severity == Severity::Error).count();
        assert_eq!(errors(Container::Webm, VideoEncoder::SvtAv1, AudioCodec::Opus), 0);
        assert_eq!(errors(Container::Webm, VideoEncoder::X264, AudioCodec::Aac), 2);
        assert_eq!(errors(Container::Mp4, VideoEncoder::X264, AudioCodec::Vorbis), 1);
        assert_eq!(Container::Mp4.check(VideoEncoder::X264, AudioCodec::Aac), vec![]);
        assert_eq!(Container::Mp4.check(VideoEncoder::SvtAv1, AudioCodec::Opus).len(), 1);
    }

    #[test]
    fn extensions_round_trip() {
        for c in Container::ALL {
            assert_eq!(Container::from_extension(c.extension()), Some(c));
        }
        assert_eq!(Container::from_extension("MKV"), Some(Container::Mkv));
    }
}
