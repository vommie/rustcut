//! The folder navigator's state (Q-NF3, pure): the folder's files, the search's hits, the page and the highlighted row,
//! and what a page shows. The bridge feeds it listings and hits (filtered on the runtime) and turns its view into the
//! Slint models.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use super::pattern::Pattern;

/// A slot of the pager: a page (counted from 0) or the gap of left-out pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Page(usize),
    Gap,
}

/// A piece of a name: highlighted or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub hit: bool,
    /// This piece gives way when the name does not fit (cut at its end); the others keep their width.
    pub shrink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// The file's place in the folder's list.
    pub index: usize,
    pub segments: Vec<Segment>,
    pub current: bool,
    pub highlighted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageView {
    pub rows: Vec<RowView>,
    pub slots: Vec<Slot>,
    pub page: usize,
    /// The page the open video is on (with the search's hits), if it is a hit.
    pub current_page: Option<usize>,
    /// "46–60 of 120", "1–9 of 9 found", "0 found".
    pub range: String,
}

#[derive(Debug, Default)]
pub struct Navigator {
    files: Arc<Vec<PathBuf>>,
    names: Arc<Vec<String>>,
    current: Option<usize>,
    /// The places in `files` the search keeps, in order; every file without a search.
    hits: Vec<usize>,
    searching: bool,
    rows: usize,
    page: usize,
    /// A position in `hits`.
    highlight: usize,
}

impl Navigator {
    pub fn new(rows: usize) -> Self {
        Self {
            rows: rows.max(1),
            ..Self::default()
        }
    }

    pub fn set_rows(&mut self, rows: usize) {
        self.rows = rows.max(1);
        self.page = self.highlight / self.rows;
    }

    /// A new listing of the folder: the hits come again (`set_hits`); until then every file counts.
    pub fn set_files(&mut self, files: Arc<Vec<PathBuf>>, current: Option<usize>) {
        self.names = Arc::new(files.iter().map(|p| file_name(p)).collect());
        self.files = files;
        self.current = current;
        self.searching = false;
        self.hits = (0..self.files.len()).collect();
        self.clamp();
    }

    pub fn files(&self) -> &Arc<Vec<PathBuf>> {
        &self.files
    }

    pub fn names(&self) -> &Arc<Vec<String>> {
        &self.names
    }

    /// What the search kept (`None`: no search, every file).
    pub fn set_hits(&mut self, hits: Option<Vec<usize>>) {
        self.searching = hits.is_some();
        self.hits = hits.unwrap_or_else(|| (0..self.files.len()).collect());
        self.clamp();
    }

    /// The bubble opens (or the user asks for the open video): its page, the video highlighted.
    pub fn show_current(&mut self) {
        match self.current.and_then(|c| self.hits.iter().position(|&h| h == c)) {
            Some(at) => self.highlight = at,
            None => self.highlight = 0,
        }
        self.page = self.highlight / self.rows;
    }

    /// How many files the search kept (all without one).
    pub fn found(&self) -> usize {
        self.hits.len()
    }

    /// How many rows the list is high when the bubble opens: a page, or fewer when the folder has fewer files (at
    /// least one, for "No files"). Paging and searching keep it.
    pub fn list_rows(&self) -> usize {
        self.files.len().clamp(1, self.rows)
    }

    pub fn pages(&self) -> usize {
        self.hits.len().div_ceil(self.rows).max(1)
    }

    pub fn go_to_page(&mut self, page: usize) {
        self.page = page.min(self.pages() - 1);
        self.highlight = (self.page * self.rows).min(self.hits.len().saturating_sub(1));
    }

    /// Up/Down (±1), PgUp/PgDn (± a page); the page follows the highlight.
    pub fn step(&mut self, delta: isize) {
        let last = self.hits.len().saturating_sub(1);
        let at = isize::try_from(self.highlight)
            .unwrap_or(isize::MAX)
            .saturating_add(delta);
        self.highlight = usize::try_from(at.max(0)).unwrap_or(0).min(last);
        self.page = self.highlight / self.rows;
    }

    pub fn home(&mut self) {
        self.highlight = 0;
        self.page = 0;
    }

    pub fn end(&mut self) {
        self.highlight = self.hits.len().saturating_sub(1);
        self.page = self.highlight / self.rows;
    }

    /// The pointer rests on a row of the page (counted from the page's first row).
    pub fn hover(&mut self, row: usize) {
        let at = self.page * self.rows + row;
        if at < self.hits.len() {
            self.highlight = at;
        }
    }

    pub fn highlighted(&self) -> Option<&PathBuf> {
        self.hits.get(self.highlight).and_then(|&i| self.files.get(i))
    }

    /// The paths of the shown page, in order.
    pub fn page_paths(&self) -> Vec<PathBuf> {
        self.page_range()
            .filter_map(|i| self.hits.get(i))
            .filter_map(|&i| self.files.get(i).cloned())
            .collect()
    }

    fn page_range(&self) -> Range<usize> {
        let start = self.page * self.rows;
        start..(start + self.rows).min(self.hits.len())
    }

    fn clamp(&mut self) {
        self.highlight = self.highlight.min(self.hits.len().saturating_sub(1));
        self.page = self.page.min(self.pages() - 1);
    }

    pub fn view(&self, pattern: Option<&Pattern>) -> PageView {
        let rows = self
            .page_range()
            .filter_map(|at| self.hits.get(at).map(|&index| (at, index)))
            .map(|(at, index)| {
                let name = self.names.get(index).map_or("", String::as_str);
                RowView {
                    index,
                    segments: segments(name, &pattern.map(|p| p.spans(name)).unwrap_or_default()),
                    current: Some(index) == self.current,
                    highlighted: at == self.highlight,
                }
            })
            .collect();
        let found = self.hits.len();
        let shown = self.page_range();
        let range = match (found, self.searching) {
            (0, true) => "0 found".to_owned(),
            (0, false) => "No files".to_owned(),
            (_, true) => format!("{}–{} of {found} found", shown.start + 1, shown.end),
            (_, false) => format!("{}–{} of {found}", shown.start + 1, shown.end),
        };
        PageView {
            rows,
            slots: slots(self.pages(), self.page),
            page: self.page,
            current_page: self
                .current
                .and_then(|c| self.hits.iter().position(|&h| h == c))
                .map(|at| at / self.rows),
            range,
        }
    }
}

/// The places of `names` a search keeps.
pub fn filter(names: &[String], pattern: &Pattern) -> Vec<usize> {
    names
        .iter()
        .enumerate()
        .filter(|(_, n)| pattern.is_match(n))
        .map(|(i, _)| i)
        .collect()
}

/// Seven slots once there are more than seven pages, so the pager never changes its width while paging:
/// `1 2 3 4 5 … N`, `1 … a b c … N`, `1 … N-4 N-3 N-2 N-1 N`.
pub fn slots(pages: usize, at: usize) -> Vec<Slot> {
    if pages <= 7 {
        return (0..pages).map(Slot::Page).collect();
    }
    let last = pages - 1;
    if at <= 3 {
        let mut out: Vec<Slot> = (0..5).map(Slot::Page).collect();
        out.extend([Slot::Gap, Slot::Page(last)]);
        out
    } else if at >= last - 3 {
        let mut out = vec![Slot::Page(0), Slot::Gap];
        out.extend((last - 4..=last).map(Slot::Page));
        out
    } else {
        vec![
            Slot::Page(0),
            Slot::Gap,
            Slot::Page(at - 1),
            Slot::Page(at),
            Slot::Page(at + 1),
            Slot::Gap,
            Slot::Page(last),
        ]
    }
}

/// The name in pieces: the hits apart, and one piece that gives way when the name is too long. That is the last
/// piece's head when it is no hit (its end, a counter and the extension, stays, like `MiddleText`), else the longest
/// piece that is no hit.
pub fn segments(name: &str, spans: &[Range<usize>]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut at = 0;
    for span in spans {
        if let (Some(before), Some(hit)) = (name.get(at..span.start), name.get(span.clone())) {
            if !before.is_empty() {
                out.push(Segment {
                    text: before.to_owned(),
                    hit: false,
                    shrink: false,
                });
            }
            out.push(Segment {
                text: hit.to_owned(),
                hit: true,
                shrink: false,
            });
            at = span.end;
        }
    }
    if let Some(rest) = name.get(at..).filter(|r| !r.is_empty()) {
        let (head, tail) = crate::util::text::split_middle(rest);
        out.push(Segment {
            text: head,
            hit: false,
            shrink: false,
        });
        if !tail.is_empty() {
            out.push(Segment {
                text: tail,
                hit: false,
                shrink: false,
            });
        }
    }
    let ends_plain = out.last().is_some_and(|s| !s.hit);
    let chosen = if ends_plain && out.len() >= 2 && !out.get(out.len() - 2).is_some_and(|s| s.hit) {
        // The last piece was split: its head gives way.
        Some(out.len() - 2)
    } else {
        out.iter()
            .enumerate()
            .filter(|(_, s)| !s.hit)
            .max_by_key(|(_, s)| s.text.chars().count())
            .map(|(i, _)| i)
    };
    if let Some(piece) = out.get_mut(chosen.unwrap_or(0)) {
        piece.shrink = true;
    }
    out
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
