//! One worker's own view of the video: a demuxer and a decoder that stay open. Staying open is what makes a thumbnail cost
//! milliseconds instead of a process start. The lab's finding lives in [`Source::keyframe`]: a decoder with a reorder delay
//! keeps a frame until later frames arrive, so with `skip_frame=nokey` it would read on to the next keyframe after every
//! seek. Draining it right after the keyframe gets the frame at once and reads only that packet.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use ffmpeg_next::codec::Context;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::media::Type;
use ffmpeg_next::{Discard, Packet, Rational, Rescale, decoder, format, frame, rescale};
use parking_lot::Mutex;
use rustcut_thumbs_protocol::Reply;

use crate::picture::{Picture, Picturer};

/// How much a keyframe search may read before it counts as missing: a file without keyframe flags would read to its end.
const KEY_SEARCH_BYTES: usize = 64 * 1024 * 1024;

/// What happened to a job's target.
pub enum Outcome {
    Thumb { reply: Reply, webp: Vec<u8> },
    Plain(Reply),
}

pub struct Source {
    input: Input,
    stream: usize,
    time_base: Rational,
    start: i64,
    decoder: decoder::Video,
    picturer: Picturer,
    /// Keyframes any worker already took, by their packet time.
    seen: Arc<Mutex<HashSet<i64>>>,
    frame: frame::Video,
}

impl Source {
    pub fn open(path: &Path, long_side: u32, quality: u8, seen: Arc<Mutex<HashSet<i64>>>) -> Result<Self, ffmpeg_next::Error> {
        let input = format::input(path)?;
        let (stream, time_base, parameters, start) = {
            let video = input.streams().best(Type::Video).ok_or(ffmpeg_next::Error::StreamNotFound)?;
            let start = match video.start_time() {
                ffmpeg_next::ffi::AV_NOPTS_VALUE => 0,
                known => known,
            };
            (video.index(), video.time_base(), video.parameters(), start)
        };
        let mut context = Context::from_parameters(parameters)?;
        context.set_threading(ffmpeg_next::threading::Config::count(1));
        let mut decoder = context.decoder().video()?;
        decoder.skip_loop_filter(Discard::All);
        let shape = decoder.aspect_ratio();
        let aspect = (shape.numerator(), shape.denominator());
        Ok(Self {
            input,
            stream,
            time_base,
            start,
            decoder,
            picturer: Picturer::new(long_side, quality, aspect, time_base),
            seen,
            frame: frame::Video::empty(),
        })
    }

    /// The video's length in milliseconds, 0 when the container does not know it.
    pub fn duration_ms(&self) -> u64 {
        u64::try_from(self.input.duration() / 1000).unwrap_or(0)
    }

    fn millis(&self, pts: i64) -> u64 {
        let micro = (pts - self.start).rescale(self.time_base, rescale::TIME_BASE);
        u64::try_from(micro / 1000).unwrap_or(0)
    }

    /// Goes to the keyframe at or before the time. The container's clock has the stream's start in it.
    fn seek(&mut self, ms: u64) -> bool {
        let start = self.start.rescale(self.time_base, rescale::TIME_BASE);
        let target = i64::try_from(ms).unwrap_or(i64::MAX / 2000).saturating_mul(1000).saturating_add(start);
        self.decoder.flush();
        self.input.seek(target, ..target).is_ok()
    }

    /// The next packet of the video stream, `None` at the end.
    fn next_packet(&mut self, packet: &mut Packet) -> Option<()> {
        loop {
            if packet.read(&mut self.input).is_err() {
                return None;
            }
            if packet.stream() == self.stream {
                return Some(());
            }
        }
    }

    /// The keyframe at or before the time, each only once.
    pub fn keyframe(&mut self, target_ms: u64) -> Outcome {
        let missing = Outcome::Plain(Reply::Missing { target_ms });
        if !self.seek(target_ms) {
            return missing;
        }
        self.decoder.skip_frame(Discard::NonKey);
        let mut packet = Packet::empty();
        let mut budget = KEY_SEARCH_BYTES;
        loop {
            if self.next_packet(&mut packet).is_none() {
                return missing;
            }
            if packet.is_key() {
                break;
            }
            budget = budget.saturating_sub(packet.size());
            if budget == 0 {
                return missing;
            }
        }
        let pts = packet.pts().or(packet.dts()).unwrap_or(self.start);
        let time_ms = self.millis(pts);
        if !self.seen.lock().insert(pts) {
            return Outcome::Plain(Reply::Known { target_ms, time_ms });
        }
        if self.decoder.send_packet(&packet).is_err() || self.decoder.send_eof().is_err() {
            return missing;
        }
        if self.decoder.receive_frame(&mut self.frame).is_err() {
            return missing;
        }
        self.picture(target_ms, time_ms, false).unwrap_or(missing)
    }

    /// The frame at the time.
    pub fn exact(&mut self, target_ms: u64) -> Outcome {
        let mut found = None;
        self.span(target_ms, &[target_ms], &mut |outcome| found = Some(outcome), &|| false);
        found.unwrap_or(Outcome::Plain(Reply::Missing { target_ms }))
    }

    /// Decodes on from the keyframe before `start_ms`; `emit` gets each target (sorted). Returns the targets left when
    /// `stop` asked to let go. Frames nothing refers to are skipped: a target lands at most a frame or two off.
    pub fn span(
        &mut self,
        start_ms: u64,
        targets: &[u64],
        emit: &mut dyn FnMut(Outcome),
        stop: &dyn Fn() -> bool,
    ) -> Vec<u64> {
        let mut left = targets;
        if !self.seek(start_ms) {
            for &target_ms in left {
                emit(Outcome::Plain(Reply::Missing { target_ms }));
            }
            return Vec::new();
        }
        self.decoder.skip_frame(Discard::NonReference);
        let mut packet = Packet::empty();
        let mut ended = false;
        while !left.is_empty() {
            if self.next_packet(&mut packet).is_some() {
                if self.decoder.send_packet(&packet).is_err() {
                    continue;
                }
            } else if !ended {
                ended = true;
                if self.decoder.send_eof().is_err() {
                    break;
                }
            } else {
                break;
            }
            while let Some((&target_ms, rest)) = left.split_first() {
                if self.decoder.receive_frame(&mut self.frame).is_err() {
                    break;
                }
                let Some(pts) = self.frame.timestamp() else { continue };
                let time_ms = self.millis(pts);
                let mut remaining = rest;
                let mut hit = None;
                if time_ms >= target_ms {
                    hit = Some(target_ms);
                    // Targets the frame has passed as well: nothing nearer will come.
                    while let Some((&next, tail)) = remaining.split_first() {
                        if next > time_ms {
                            break;
                        }
                        emit(Outcome::Plain(Reply::Known { target_ms: next, time_ms }));
                        remaining = tail;
                    }
                }
                if let Some(target_ms) = hit {
                    if let Some(outcome) = self.picture(target_ms, time_ms, true) {
                        emit(outcome);
                    }
                    left = remaining;
                    if !left.is_empty() && stop() {
                        return left.to_vec();
                    }
                }
            }
        }
        for &target_ms in left {
            emit(Outcome::Plain(Reply::Missing { target_ms }));
        }
        Vec::new()
    }

    fn picture(&mut self, target_ms: u64, time_ms: u64, exact: bool) -> Option<Outcome> {
        let Picture { webp, width, height } = self.picturer.make(&self.frame)?;
        Some(Outcome::Thumb {
            reply: Reply::Thumb {
                target_ms,
                time_ms,
                exact,
                width,
                height,
            },
            webp,
        })
    }
}
