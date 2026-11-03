//! The check of a finished render before it replaces anything (ADR-9): the temp file exists, is not empty, ffprobe reads
//! it, and it has the expected video, audio tracks, size and a plausible duration.

use std::path::Path;

use super::Expectation;
use crate::edit::Size;
use crate::media::{MediaInfo, ProbeOptions, probe};
use crate::time::{Precision, Timestamp};

/// The smallest allowed difference between the expected and the probed duration (frame and audio packet granularity at
/// each section's edges, encoder delay).
pub const MIN_DURATION_TOLERANCE: Timestamp = Timestamp::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("the rendered file is missing")]
    Missing,
    #[error("the rendered file is empty")]
    Empty,
    #[error("the rendered file is not readable: {0}")]
    Unreadable(String),
    #[error("the rendered file has no video")]
    NoVideo,
    #[error("the rendered file has {found} audio tracks instead of {expected}")]
    AudioTracks { expected: usize, found: usize },
    #[error("the rendered video is {found} instead of {expected}")]
    Size { expected: String, found: String },
    #[error("the rendered file reports no duration")]
    NoDuration,
    #[error("the rendered file is {found} long instead of {expected}")]
    Duration { expected: String, found: String },
}

/// How far the probed duration may be from the expected one: 2 % of it, at least [`MIN_DURATION_TOLERANCE`].
pub fn duration_tolerance(expected: Timestamp) -> Timestamp {
    Timestamp::from_micros(expected.as_micros() / 50).max(MIN_DURATION_TOLERANCE)
}

/// Whether a file of `found` length is plausible: shorter by at most the tolerance, longer by at most the tolerance
/// plus the sections' edge frames ([`Expectation::edge_slack`]).
fn plausible_duration(found: Timestamp, expected: &Expectation) -> bool {
    let tolerance = duration_tolerance(expected.duration);
    found >= expected.duration.saturating_sub(tolerance)
        && found
            <= expected
                .duration
                .saturating_add(tolerance)
                .saturating_add(expected.edge_slack)
}

/// Checks what ffprobe found in a rendered file of `bytes` bytes against the plan.
pub fn check(info: &MediaInfo, bytes: u64, expected: &Expectation) -> Result<(), VerifyError> {
    if bytes == 0 {
        return Err(VerifyError::Empty);
    }
    let (_, video) = info.primary_video().ok_or(VerifyError::NoVideo)?;
    let found = info.audio_streams().count();
    if found != expected.audio_streams {
        return Err(VerifyError::AudioTracks {
            expected: expected.audio_streams,
            found,
        });
    }
    if let Some((width, height)) = video.oriented_size() {
        let size = Size::new(width, height);
        if size != expected.size {
            let text = |s: Size| format!("{}x{}", s.width, s.height);
            return Err(VerifyError::Size {
                expected: text(expected.size),
                found: text(size),
            });
        }
    }
    let duration = info.duration().ok_or(VerifyError::NoDuration)?;
    if !plausible_duration(duration, expected) {
        return Err(VerifyError::Duration {
            expected: expected.duration.format(Precision::Millis),
            found: duration.format(Precision::Millis),
        });
    }
    Ok(())
}

/// Probes `file` and [`check`]s it; returns its size in bytes.
pub async fn verify_file(file: &Path, expected: &Expectation, options: &ProbeOptions) -> Result<u64, VerifyError> {
    let bytes = match tokio::fs::metadata(file).await {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VerifyError::Missing),
        Err(e) => return Err(VerifyError::Unreadable(e.to_string())),
    };
    if bytes == 0 {
        return Err(VerifyError::Empty);
    }
    let info = probe(file, options)
        .await
        .map_err(|e| VerifyError::Unreadable(e.to_string()))?;
    check(&info, bytes, expected)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AudioStream, Container, Stream, StreamKind, VideoStream};

    fn stream(index: u32, kind: StreamKind) -> Stream {
        Stream {
            index,
            kind,
            codec: None,
            codec_long: None,
            profile: None,
            bit_rate: None,
            duration: None,
            start_time: None,
            language: None,
            title: None,
            default: false,
        }
    }

    fn rendered(width: u32, height: u32, audio: u32, millis: Option<i64>) -> MediaInfo {
        let mut streams = vec![stream(
            0,
            StreamKind::Video(VideoStream {
                width: Some(width),
                height: Some(height),
                ..VideoStream::default()
            }),
        )];
        streams.extend((1..=audio).map(|i| stream(i, StreamKind::Audio(AudioStream::default()))));
        MediaInfo {
            container: Container {
                duration: millis.map(Timestamp::from_millis),
                ..Container::default()
            },
            streams,
            frame_timing: None,
        }
    }

    const EXPECTED: Expectation = Expectation {
        duration: Timestamp::from_secs(10),
        edge_slack: Timestamp::ZERO,
        audio_streams: 2,
        size: Size::new(1280, 720),
    };

    #[test]
    fn a_good_render_passes() {
        assert_eq!(check(&rendered(1280, 720, 2, Some(10_000)), 5, &EXPECTED), Ok(()));
        // Within the 500 ms minimum tolerance.
        assert_eq!(check(&rendered(1280, 720, 2, Some(10_480)), 5, &EXPECTED), Ok(()));
        assert_eq!(duration_tolerance(Timestamp::from_secs(100)), Timestamp::from_secs(2));
    }

    #[test]
    fn many_sections_may_be_a_frame_longer_at_each_edge() {
        // 40 sections of 0.25 s of a 25 fps video came out 11.2 s long (user report 2026-11-03).
        let expected = Expectation {
            duration: Timestamp::from_secs(10),
            edge_slack: Timestamp::from_millis(40 * 2 * 40),
            ..EXPECTED
        };
        assert_eq!(check(&rendered(1280, 720, 2, Some(11_200)), 5, &expected), Ok(()));
        assert_eq!(check(&rendered(1280, 720, 2, Some(13_700)), 5, &expected), Ok(()));
        assert!(check(&rendered(1280, 720, 2, Some(13_800)), 5, &expected).is_err());
        // The slack only allows longer files: a short one is still a defect.
        assert!(check(&rendered(1280, 720, 2, Some(9_400)), 5, &expected).is_err());
    }

    #[test]
    fn every_defect_has_its_reason() {
        assert_eq!(
            check(&rendered(1280, 720, 2, Some(10_000)), 0, &EXPECTED),
            Err(VerifyError::Empty)
        );
        let mut no_video = rendered(1280, 720, 2, Some(10_000));
        no_video.streams.remove(0);
        assert_eq!(check(&no_video, 5, &EXPECTED), Err(VerifyError::NoVideo));
        assert_eq!(
            check(&rendered(1280, 720, 1, Some(10_000)), 5, &EXPECTED),
            Err(VerifyError::AudioTracks { expected: 2, found: 1 })
        );
        assert_eq!(
            check(&rendered(1280, 718, 2, Some(10_000)), 5, &EXPECTED),
            Err(VerifyError::Size {
                expected: "1280x720".into(),
                found: "1280x718".into()
            })
        );
        assert_eq!(
            check(&rendered(1280, 720, 2, None), 5, &EXPECTED),
            Err(VerifyError::NoDuration)
        );
        assert_eq!(
            check(&rendered(1280, 720, 2, Some(4_200)), 5, &EXPECTED),
            Err(VerifyError::Duration {
                expected: "0:00:10.000".into(),
                found: "0:00:04.200".into()
            })
        );
    }
}
