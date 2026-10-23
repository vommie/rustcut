//! The Logs panel's lines without Slint: a job's stored log (RustCut's notes and ffmpeg's stderr) with a level per line
//! (errors red, warnings yellow, RustCut's notes and the command lines set apart), filtered by a search text and an
//! "errors only" switch (both in `UiState.logs`), and the text that Copy puts on the clipboard.

use rustcut_core::store::{LogLine, LogStream};

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
}
