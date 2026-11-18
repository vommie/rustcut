//! What the folder navigator's search keeps (Q-NF3, pure): a text without wildcards is found anywhere in the name,
//! `*`, `?` and `[...]` work like Dolphin's filter (the whole name must match), the `.*` toggle makes it a regular
//! expression. Case never matters. Every kind tells where in the name it matched, for the highlight.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

/// The largest compiled pattern (like the tag filter's).
const SIZE_LIMIT: usize = 1 << 20;

/// A search that compiled.
#[derive(Debug, Clone)]
pub struct Pattern {
    regex: Regex,
    /// A wildcard pattern highlights its literal parts (the capture groups), the others every match.
    groups: bool,
}

/// Why a search does not compile: a short title, and the reason with an example of a right one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    pub message: String,
    pub example: String,
}

impl Pattern {
    /// `None` for an empty search (everything counts).
    pub fn parse(text: &str, regex: bool) -> Result<Option<Self>, PatternError> {
        if text.is_empty() {
            return Ok(None);
        }
        if regex {
            return compile(text, false).map(Some).map_err(|err| PatternError {
                message: "Not a valid regular expression".to_owned(),
                example: format!(
                    "{}. Example: (harbor|beach) or ^2026-07-1[0-4]",
                    capitalized(&syntax_reason(&err))
                ),
            });
        }
        if !text.contains(['*', '?', '[']) {
            return compile(&regex::escape(text), false).map(Some).map_err(too_long);
        }
        let translated = wildcards(text)?;
        compile(&translated, true).map(Some).map_err(too_long)
    }

    pub fn is_match(&self, name: &str) -> bool {
        self.regex.is_match(name)
    }

    /// The byte ranges of `name` to highlight, in order, never empty or overlapping.
    pub fn spans(&self, name: &str) -> Vec<Range<usize>> {
        if self.groups {
            return self
                .regex
                .captures(name)
                .map(|caps| {
                    caps.iter()
                        .skip(1)
                        .flatten()
                        .map(|m| m.range())
                        .filter(|r| !r.is_empty())
                        .collect()
                })
                .unwrap_or_default();
        }
        self.regex
            .find_iter(name)
            .map(|m| m.range())
            .filter(|r| !r.is_empty())
            .collect()
    }
}

fn compile(source: &str, groups: bool) -> Result<Pattern, regex::Error> {
    RegexBuilder::new(source)
        .case_insensitive(true)
        .size_limit(SIZE_LIMIT)
        .build()
        .map(|regex| Pattern { regex, groups })
}

/// A text or wildcard search only fails to compile when it is too long.
fn too_long(err: regex::Error) -> PatternError {
    tracing::debug!("[NextInFolder] The search does not compile: {err}");
    PatternError {
        message: "The search is too long".to_owned(),
        example: "Example: harbor".to_owned(),
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The last line of the regex crate's report ("error: unclosed group" -> "unclosed group").
fn syntax_reason(err: &regex::Error) -> String {
    match err {
        regex::Error::Syntax(text) => text
            .lines()
            .rev()
            .find_map(|line| line.trim().strip_prefix("error: "))
            .unwrap_or("it cannot be read")
            .to_owned(),
        regex::Error::CompiledTooBig(_) => "it is too long".to_owned(),
        _ => "it cannot be read".to_owned(),
    }
}

/// A wildcard pattern as an anchored regex: literal runs become groups (they are highlighted), `*` any text, `?` one
/// character, `[...]` a class (`[!...]` negated, like Dolphin).
fn wildcards(text: &str) -> Result<String, PatternError> {
    let mut out = String::from("^");
    let mut literal = String::new();
    let flush = |literal: &mut String, out: &mut String| {
        if !literal.is_empty() {
            out.push('(');
            out.push_str(&regex::escape(literal));
            out.push(')');
            literal.clear();
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                flush(&mut literal, &mut out);
                out.push_str(".*?");
            }
            '?' => {
                flush(&mut literal, &mut out);
                out.push('.');
            }
            '[' => {
                flush(&mut literal, &mut out);
                let mut class = String::new();
                let mut closed = false;
                if chars.peek() == Some(&'!') {
                    chars.next();
                    class.push('^');
                }
                for c in chars.by_ref() {
                    if c == ']' && !class.is_empty() && class != "^" {
                        closed = true;
                        break;
                    }
                    if matches!(c, '\\' | '[' | ']' | '&' | '~') {
                        class.push('\\');
                    }
                    class.push(c);
                }
                if !closed {
                    return Err(PatternError {
                        message: "Not a complete pattern".to_owned(),
                        example: "A “]” is missing. Example: clip 0[1-5]*".to_owned(),
                    });
                }
                out.push('[');
                out.push_str(&class);
                out.push(']');
            }
            c => literal.push(c),
        }
    }
    flush(&mut literal, &mut out);
    out.push('$');
    Ok(out)
}

#[cfg(test)]
#[path = "pattern_tests.rs"]
mod tests;
