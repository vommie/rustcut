//! The log of one render: RustCut's notes and ffmpeg's stderr, capped so a chatty ffmpeg never fills the memory or the
//! database. The first lines (command, stream mapping) and the last ones (where it went wrong) are kept.

use std::collections::VecDeque;
use std::fmt::Write as _;

use crate::store::{LogLine, LogStream};

/// Lines kept from the start of the log.
pub const HEAD_LINES: usize = 200;
/// Lines kept from the end of the log.
pub const TAIL_LINES: usize = 2_000;
/// Longer lines are cut (in characters).
pub const MAX_LINE_CHARS: usize = 2_000;

/// A capped log: the first [`HEAD_LINES`], then the latest [`TAIL_LINES`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobLog {
    head: Vec<LogLine>,
    tail: VecDeque<LogLine>,
    dropped: usize,
}

impl JobLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, stream: LogStream, line: &str) {
        let line = match line.char_indices().nth(MAX_LINE_CHARS) {
            Some((end, _)) => format!("{} [...]", line.get(..end).unwrap_or(line)),
            None => line.to_owned(),
        };
        let entry = LogLine {
            at: chrono::Utc::now().timestamp_millis(),
            stream,
            line,
        };
        if self.head.len() < HEAD_LINES {
            self.head.push(entry);
            return;
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.tail.push_back(entry);
    }

    /// A note of RustCut itself.
    pub fn note(&mut self, line: &str) {
        self.push(LogStream::App, line);
    }

    /// Lines left out between the head and the tail.
    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_empty()
    }

    /// The kept lines in order (a gap is not marked; see [`JobLog::dropped`]).
    pub fn lines(&self) -> impl DoubleEndedIterator<Item = &LogLine> {
        self.head.iter().chain(self.tail.iter())
    }

    /// The kept lines as text, with a marker where lines were left out.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines().enumerate() {
            if i == HEAD_LINES && self.dropped > 0 {
                // Writing into a String cannot fail.
                let _ = writeln!(out, "[... {} lines left out ...]", self.dropped);
            }
            out.push_str(&line.line);
            out.push('\n');
        }
        out
    }

    /// Why ffmpeg failed, for the job's failure reason: ffmpeg 7+ follows the cause with a cascade of follow-up errors
    /// ("Task finished with error code", "Nothing was written"), so the earliest line of the last run of error lines
    /// (`-loglevel level+...` tags them `[error]`/`[fatal]`) is the cause. Without tagged lines, ffmpeg's last line.
    pub fn last_error(&self) -> Option<String> {
        let ffmpeg = || self.lines().rev().filter(|l| l.stream == LogStream::Ffmpeg);
        let cause = ffmpeg()
            .map(|l| tagged_error(&l.line))
            .skip_while(Option::is_none)
            .map_while(|tagged| tagged)
            .last();
        if let Some((context, message)) = cause {
            let component = component(context);
            return Some(if component.is_empty() {
                message.to_owned()
            } else {
                format!("{component}: {message}")
            });
        }
        ffmpeg()
            .map(|l| l.line.trim())
            .find(|l| !l.is_empty())
            .map(str::to_owned)
    }
}

/// `(context, message)` of a line tagged `[error]` or `[fatal]` (`[libx264 @ 0x55] [error] text`, `x264 [error]: text`).
fn tagged_error(line: &str) -> Option<(&str, &str)> {
    ["[fatal]", "[error]"].iter().find_map(|tag| {
        let (context, message) = line.split_once(tag)?;
        let message = message.trim_start_matches(':').trim();
        (!message.is_empty()).then_some((context.trim(), message))
    })
}

/// The component of a log context without addresses: `[vost#0:0/libx264 @ 0x5] [enc:libx264 @ 0x6]` -> `enc:libx264`,
/// `x264` -> `x264`.
pub(crate) fn component(context: &str) -> &str {
    let last = context.rsplit('[').next().unwrap_or(context);
    last.split(" @ ").next().unwrap_or(last).trim_end_matches(']').trim()
}

/// A line of ffmpeg's stderr without its leading `[context @ 0x...]` groups.
pub(crate) fn without_context(line: &str) -> &str {
    let mut rest = line.trim();
    while rest.starts_with('[')
        && let Some((_, after)) = rest.split_once("] ")
    {
        rest = after.trim_start();
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_and_tail_are_kept() {
        let mut log = JobLog::new();
        for i in 0..(HEAD_LINES + TAIL_LINES + 50) {
            log.push(LogStream::Ffmpeg, &format!("line {i}"));
        }
        assert_eq!(log.len(), HEAD_LINES + TAIL_LINES);
        assert_eq!(log.dropped(), 50);
        let lines: Vec<&str> = log.lines().map(|l| l.line.as_str()).collect();
        assert_eq!(lines[HEAD_LINES - 1], format!("line {}", HEAD_LINES - 1));
        assert_eq!(lines[HEAD_LINES], format!("line {}", HEAD_LINES + 50));
        assert!(log.to_text().contains("[... 50 lines left out ...]\nline 250\n"));
    }

    #[test]
    fn long_lines_are_cut_at_a_character() {
        let mut log = JobLog::new();
        log.note(&"ä".repeat(MAX_LINE_CHARS + 10));
        let line = &log.lines().next().unwrap().line;
        assert_eq!(line.chars().count(), MAX_LINE_CHARS + " [...]".len());
        assert_eq!(log.lines().next().unwrap().stream, LogStream::App);
    }

    #[test]
    fn contexts() {
        assert_eq!(component("[vost#0:0/libx264 @ 0x5] [enc:libx264 @ 0x6]"), "enc:libx264");
        assert_eq!(component("x264"), "x264");
        assert_eq!(component(""), "");
        assert_eq!(
            without_context("[h264_nvenc @ 0x55a8] Cannot load libcuda.so.1"),
            "Cannot load libcuda.so.1"
        );
        assert_eq!(without_context("[a @ 1] [b @ 2] text [c]"), "text [c]");
        assert_eq!(
            without_context("Device creation failed: -5."),
            "Device creation failed: -5."
        );
    }

    #[test]
    fn the_failure_reason() {
        let mut log = JobLog::new();
        assert_eq!(log.last_error(), None);
        log.note("[error] notes of RustCut do not count");
        log.push(LogStream::Ffmpeg, "[info] Stream mapping:");
        assert_eq!(log.last_error().as_deref(), Some("[info] Stream mapping:"));
        log.push(
            LogStream::Ffmpeg,
            "[libx264 @ 0x55d1c0] [error] height not divisible by 2 (853x479)",
        );
        log.push(LogStream::Ffmpeg, "[info] Conversion failed!");
        assert_eq!(
            log.last_error().as_deref(),
            Some("libx264: height not divisible by 2 (853x479)")
        );
        // A real cascade (ffmpeg 9): the cause is the first of the last run of errors.
        for line in [
            "[info] Stream mapping:",
            "x264 [error]: baseline profile doesn't support a bit depth of 10",
            "[libx264 @ 0x55d5cb6cf380] [error] Error setting profile baseline.",
            "[vost#0:0/libx264 @ 0x55d5] [enc:libx264 @ 0x55d5] [error] Error while opening encoder",
            "[vf#0:0 @ 0x55d5cb696100] [error] Task finished with error code: -22 (Invalid argument)",
            "[out#0/matroska @ 0x55d5cb696fc0] [error] Nothing was written into output file",
            "[info] Conversion failed!",
        ] {
            log.push(LogStream::Ffmpeg, line);
        }
        assert_eq!(
            log.last_error().as_deref(),
            Some("x264: baseline profile doesn't support a bit depth of 10")
        );
        log.push(
            LogStream::Ffmpeg,
            "[fatal] Error opening output files: No space left on device",
        );
        assert_eq!(
            log.last_error().as_deref(),
            Some("Error opening output files: No space left on device")
        );
    }
}
