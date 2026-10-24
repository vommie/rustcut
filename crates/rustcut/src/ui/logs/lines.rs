//! The Logs panel's lines without Slint: a job's stored log (RustCut's notes and ffmpeg's stderr) with a level per line
//! (errors red, warnings yellow, RustCut's notes and the command lines set apart), filtered by a search text and an
//! "errors only" switch (both in `UiState.logs`), and the text that Copy puts on the clipboard.

use rustcut_core::settings::LogSource;
use rustcut_core::store::{LogLine, LogStream};

use crate::app::log_book::BookLine;
use crate::ui::notices::{Entry, Level as NoticeLevel};

/// How a line is drawn (the numbers are `logs_panel.slint`'s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineLevel {
    Info = 0,
    /// A note of RustCut (`[Render] ...`).
    Note = 1,
    Warning = 2,
    Error = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownLine {
    /// Position in the whole log (1-based), shown as the line number.
    pub number: usize,
    pub text: String,
    pub level: LineLevel,
}

/// The level of one stored line.
pub fn level(line: &LogLine) -> LineLevel {
    let text = line.line.as_str();
    match line.stream {
        LogStream::App | LogStream::RustView => {
            if text.contains("] Failed") || text.starts_with("[Render] No progress") {
                LineLevel::Error
            } else {
                LineLevel::Note
            }
        }
        LogStream::Ffmpeg => {
            if text.contains("[fatal]") || text.contains("[error]") || text.contains(" [error]:") {
                LineLevel::Error
            } else if text.contains("[warning]") || text.contains(" [warning]:") || text.starts_with("Svt[warn]") {
                LineLevel::Warning
            } else {
                LineLevel::Info
            }
        }
    }
}

/// The lines that pass the filter, numbered by their place in the whole log.
pub fn shown(lines: &[LogLine], search: &str, errors_only: bool) -> Vec<ShownLine> {
    let needle = search.trim().to_lowercase();
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let level = level(line);
            let keep = (!errors_only || level >= LineLevel::Warning)
                && (needle.is_empty() || line.line.to_lowercase().contains(&needle));
            keep.then(|| ShownLine {
                number: i + 1,
                text: line.line.clone(),
                level,
            })
        })
        .collect()
}

/// Whether a line of the app's log belongs to the RustView log (the link and the Tag & Rate panel).
pub fn is_rustview(line: &BookLine) -> bool {
    line.text.starts_with("[RustView]") || line.text.starts_with("[Tags]")
}

/// The lines of the app's log (`LogSource::App`) or its RustView part that pass the filter, numbered by their place
/// in this run's log; each line starts with its time and level.
pub fn book_shown(lines: &[BookLine], source: LogSource, search: &str, errors_only: bool) -> Vec<ShownLine> {
    let needle = search.trim().to_lowercase();
    lines
        .iter()
        .filter(|line| source != LogSource::RustView || is_rustview(line))
        .filter_map(|line| {
            let level = match line.level {
                tracing::Level::ERROR => LineLevel::Error,
                tracing::Level::WARN => LineLevel::Warning,
                _ => LineLevel::Info,
            };
            let text = format!("{} {:<5} {}", line.time, line.level, line.text);
            let keep = (!errors_only || level >= LineLevel::Warning)
                && (needle.is_empty() || text.to_lowercase().contains(&needle));
            keep.then(|| ShownLine {
                number: usize::try_from(line.seq).unwrap_or(usize::MAX),
                text,
                level,
            })
        })
        .collect()
}

/// The messages of this session (`LogSource::Messages`) that pass the filter: time, level, title and detail; a message
/// that showed nowhere says so.
pub fn messages_shown(entries: &[Entry], search: &str, errors_only: bool) -> Vec<ShownLine> {
    let needle = search.trim().to_lowercase();
    entries
        .iter()
        .filter_map(|entry| {
            let (level, word) = match entry.level {
                NoticeLevel::Error => (LineLevel::Error, "ERROR"),
                NoticeLevel::Warning => (LineLevel::Warning, "WARN"),
                NoticeLevel::Success => (LineLevel::Info, "OK"),
                NoticeLevel::Info => (LineLevel::Info, "INFO"),
            };
            let mut text = format!("{} {word:<5} {}", entry.time, entry.title);
            if !entry.detail.is_empty() {
                text.push_str(" · ");
                text.push_str(&entry.detail.replace('\n', " · "));
            }
            if entry.silent {
                text.push_str(" (not shown)");
            }
            let keep = (!errors_only || level >= LineLevel::Warning)
                && (needle.is_empty() || text.to_lowercase().contains(&needle));
            keep.then(|| ShownLine {
                number: usize::try_from(entry.seq).unwrap_or(usize::MAX),
                text,
                level,
            })
        })
        .collect()
}

/// The shown lines as text for the clipboard.
pub fn to_text(lines: &[ShownLine]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

/// The panel's line under the header: what is shown of how much.
pub fn summary(total: usize, shown: &[ShownLine]) -> String {
    let errors = shown.iter().filter(|l| l.level == LineLevel::Error).count();
    let warnings = shown.iter().filter(|l| l.level == LineLevel::Warning).count();
    let mut parts = vec![if shown.len() == total {
        match total {
            1 => "1 line".to_owned(),
            n => format!("{n} lines"),
        }
    } else {
        format!("{} of {total} lines", shown.len())
    }];
    if errors > 0 {
        parts.push(match errors {
            1 => "1 error".to_owned(),
            n => format!("{n} errors"),
        });
    }
    if warnings > 0 {
        parts.push(match warnings {
            1 => "1 warning".to_owned(),
            n => format!("{n} warnings"),
        });
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(stream: LogStream, text: &str) -> LogLine {
        LogLine {
            at: 0,
            stream,
            line: text.into(),
        }
    }

    #[test]
    fn levels_filters_and_text() {
        let log = vec![
            line(LogStream::App, "[Render] Job #3: /in/a.mp4 -> /out/a.mkv"),
            line(LogStream::App, "[Render] Encode: ffmpeg -i 'file:/in/a.mp4'"),
            line(LogStream::Ffmpeg, "[info] Stream mapping:"),
            line(LogStream::Ffmpeg, "x264 [warning]: VBV maxrate unspecified"),
            line(LogStream::Ffmpeg, "[libx264 @ 0x55] [error] height not divisible by 2"),
            line(LogStream::App, "[Render] Failed: ffmpeg ended with code 1"),
        ];
        let levels: Vec<LineLevel> = log.iter().map(level).collect();
        assert_eq!(
            levels,
            [
                LineLevel::Note,
                LineLevel::Note,
                LineLevel::Info,
                LineLevel::Warning,
                LineLevel::Error,
                LineLevel::Error
            ]
        );
        let all = shown(&log, "", false);
        assert_eq!(summary(log.len(), &all), "6 lines · 2 errors · 1 warning");
        let errors = shown(&log, "", true);
        assert_eq!(errors.iter().map(|l| l.number).collect::<Vec<_>>(), [4, 5, 6]);
        let found = shown(&log, "  HEIGHT ", false);
        assert_eq!(found.len(), 1);
        assert_eq!(summary(log.len(), &found), "1 of 6 lines · 1 error");
        assert_eq!(to_text(&found), "[libx264 @ 0x55] [error] height not divisible by 2\n");
        assert_eq!(summary(1, &shown(&log[..1], "", false)), "1 line");
    }

    fn book(seq: u64, level: tracing::Level, text: &str) -> BookLine {
        BookLine {
            seq,
            time: "14:05:09.123".into(),
            level,
            text: text.into(),
        }
    }

    #[test]
    fn the_messages_of_the_session() {
        use crate::ui::notices::Kind;
        let entry = |seq, level, title: &str, detail: &str, silent| Entry {
            seq,
            time: "15:42:07".into(),
            kind: Kind::JobQueued,
            level,
            title: title.into(),
            detail: detail.into(),
            silent,
            read: false,
        };
        let entries = [
            entry(1, NoticeLevel::Success, "Job 3 queued", "clip - 03.mkv", false),
            entry(2, NoticeLevel::Error, "Cannot open clip.mp4", "Invalid data", false),
            entry(3, NoticeLevel::Info, "Section 2 added", "", true),
        ];
        let all = messages_shown(&entries, "", false);
        let texts: Vec<&str> = all.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "15:42:07 OK    Job 3 queued · clip - 03.mkv",
                "15:42:07 ERROR Cannot open clip.mp4 · Invalid data",
                "15:42:07 INFO  Section 2 added (not shown)",
            ]
        );
        assert_eq!(messages_shown(&entries, "", true).len(), 1);
        assert_eq!(messages_shown(&entries, "CLIP.mp4", false).len(), 1);
    }

    #[test]
    fn the_app_log_and_its_rustview_part() {
        let log = vec![
            book(7, tracing::Level::INFO, "[Queue] Started jobs=0"),
            book(8, tracing::Level::WARN, "[RustView] Not reachable error=not installed"),
            book(9, tracing::Level::INFO, "[Tags] Tag tree loaded tags=10"),
            book(10, tracing::Level::ERROR, "[Store] The queue database cannot be used"),
        ];
        let app = book_shown(&log, LogSource::App, "", false);
        assert_eq!(app.len(), 4);
        assert_eq!(app.first().unwrap().text, "14:05:09.123 INFO  [Queue] Started jobs=0");
        assert_eq!(app.first().unwrap().number, 7);
        let rustview = book_shown(&log, LogSource::RustView, "", false);
        assert_eq!(rustview.iter().map(|l| l.number).collect::<Vec<_>>(), [8, 9]);
        assert_eq!(rustview.first().unwrap().level, LineLevel::Warning);
        let errors = book_shown(&log, LogSource::App, "", true);
        assert_eq!(errors.iter().map(|l| l.number).collect::<Vec<_>>(), [8, 10]);
        assert_eq!(
            book_shown(&log, LogSource::App, "warn", false).len(),
            1,
            "the level text is searchable"
        );
    }
}
