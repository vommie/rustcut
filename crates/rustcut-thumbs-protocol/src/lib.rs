//! What RustCut and its thumbnail helper `rustcut-thumbs` say to each other (P12 block 6). One helper serves one video:
//! RustCut writes a [`Start`] and then [`Command`]s as JSON lines to its stdin; the helper answers on stdout with framed
//! [`Reply`]s, a [`Reply::Thumb`] followed by its WebP picture. The end of stdin ends the helper, so it never outlives RustCut.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// A reply larger than this is a broken stream, not a picture.
const MAX_FRAME: u32 = 64 * 1024 * 1024;

/// Which video and how. RustCut decides everything from the probe, so the helper never reads settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Start {
    pub path: String,
    /// Length of a picture's longer side in pixels.
    pub long_side: u32,
    /// WebP quality, 0 to 100.
    pub quality: u8,
    pub workers: u32,
    /// Niceness of every worker. It can only go up later, so RustCut starts another helper to lower it.
    pub nice: i32,
}

/// One piece of work. Times are milliseconds on the player's clock, which starts at 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Job {
    /// The keyframe at or before the time.
    Key { ms: u64 },
    /// The frame at the time.
    Exact { ms: u64 },
    /// Decode on from the keyframe before `start_ms` and keep the frame at each target (sorted).
    Span { start_ms: u64, targets: Vec<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Appends to the queue.
    Add(Vec<Job>),
    /// Goes first and replaces the last `Front` that has not started: only the newest place of the pointer counts.
    Front(Vec<Job>),
    /// Forgets everything waiting.
    Clear,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    /// The video opened; the queue runs.
    Ready { duration_ms: u64 },
    /// A picture; `len` bytes of WebP follow. `time_ms` is where it really is.
    Thumb {
        target_ms: u64,
        time_ms: u64,
        exact: bool,
        width: u32,
        height: u32,
    },
    /// The keyframe for the target was already made.
    Known { target_ms: u64, time_ms: u64 },
    /// No picture for the target (a seek that does not work, a damaged part, no keyframe near).
    Missing { target_ms: u64 },
    /// Nothing is waiting or running.
    Idle,
    /// The helper cannot go on.
    Failed { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("the stream ended")]
    Closed,
    #[error("a frame of {0} bytes is not believable")]
    TooLarge(u32),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Writes a reply and its picture as one frame: both lengths, the JSON, the bytes.
pub fn write_reply(out: &mut impl Write, reply: &Reply, picture: &[u8]) -> Result<(), FrameError> {
    let head = serde_json::to_vec(reply)?;
    let lengths = |n: usize| u32::try_from(n).map_err(|_too_long| FrameError::TooLarge(u32::MAX));
    out.write_all(&lengths(head.len())?.to_le_bytes())?;
    out.write_all(&lengths(picture.len())?.to_le_bytes())?;
    out.write_all(&head)?;
    out.write_all(picture)?;
    out.flush()?;
    Ok(())
}

/// Reads one frame. [`FrameError::Closed`] = the helper is gone.
pub fn read_reply(input: &mut impl Read) -> Result<(Reply, Vec<u8>), FrameError> {
    let mut lengths = [0_u8; 8];
    if let Err(e) = input.read_exact(&mut lengths) {
        return Err(match e.kind() {
            io::ErrorKind::UnexpectedEof => FrameError::Closed,
            _ => e.into(),
        });
    }
    let [a, b, c, d, e, f, g, h] = lengths;
    let (head, picture) = (u32::from_le_bytes([a, b, c, d]), u32::from_le_bytes([e, f, g, h]));
    for length in [head, picture] {
        if length > MAX_FRAME {
            return Err(FrameError::TooLarge(length));
        }
    }
    let mut buffer = vec![0_u8; usize::try_from(head).unwrap_or(0)];
    input.read_exact(&mut buffer)?;
    let reply = serde_json::from_slice(&buffer)?;
    let mut blob = vec![0_u8; usize::try_from(picture).unwrap_or(0)];
    input.read_exact(&mut blob)?;
    Ok((reply, blob))
}

/// The picture's size: the shown shape with the longer side at `long_side`, in even pixels.
pub fn fit(width: u32, height: u32, aspect: (i32, i32), long_side: u32) -> (u32, u32) {
    let (n, d) = aspect;
    let shown_width = if n > 0 && d > 0 {
        f64::from(width) * f64::from(n) / f64::from(d)
    } else {
        f64::from(width)
    };
    let shown_height = f64::from(height).max(1.0);
    let long = f64::from(long_side.max(2));
    let scale = long / shown_width.max(shown_height).max(1.0);
    let even = |value: f64| {
        let rounded = (value * scale / 2.0).round().clamp(1.0, 8192.0);
        // The clamp keeps the value in range, so nothing is lost.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 1..=8192 above")]
        let half = rounded as u32;
        half * 2
    };
    (even(shown_width), even(shown_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_keeps_the_shown_shape_in_even_pixels() {
        assert_eq!(fit(1920, 1080, (1, 1), 320), (320, 180));
        assert_eq!(fit(720, 1280, (1, 1), 320), (180, 320));
        assert_eq!(fit(853, 479, (1, 1), 320), (320, 180));
    }

    #[test]
    fn a_pixel_aspect_widens_the_picture() {
        assert_eq!(fit(720, 576, (16, 15), 320), (320, 240));
        assert_eq!(fit(720, 576, (0, 1), 320), (320, 256));
    }

    #[test]
    fn replies_survive_the_round_trip() {
        let thumb = Reply::Thumb {
            target_ms: 1000,
            time_ms: 960,
            exact: false,
            width: 320,
            height: 180,
        };
        let mut wire = Vec::new();
        write_reply(&mut wire, &thumb, b"webp").unwrap();
        write_reply(&mut wire, &Reply::Idle, &[]).unwrap();
        let mut input = wire.as_slice();
        assert_eq!(read_reply(&mut input).unwrap(), (thumb, b"webp".to_vec()));
        assert_eq!(read_reply(&mut input).unwrap().0, Reply::Idle);
        assert!(matches!(read_reply(&mut input), Err(FrameError::Closed)));
    }

    #[test]
    fn a_broken_stream_is_refused_before_it_allocates() {
        let mut wire = u32::MAX.to_le_bytes().to_vec();
        wire.extend_from_slice(&0_u32.to_le_bytes());
        assert!(matches!(read_reply(&mut wire.as_slice()), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn commands_are_single_json_lines() {
        let command = Command::Front(vec![Job::Span {
            start_ms: 5,
            targets: vec![10, 20],
        }]);
        let line = serde_json::to_string(&command).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(serde_json::from_str::<Command>(&line).unwrap(), command);
    }
}
