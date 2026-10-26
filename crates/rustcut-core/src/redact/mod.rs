//! The shareable copy of a log (P9.17, the review's general note; the usual terms are pseudonymization or log
//! redaction): file names, folders and paths, tag names, user and host names, MD5 sums and the videos' titles and
//! metadata become placeholders that stay the same within a run (`<file-3>.mkv`, `<dir-2>`, `<tag-4>`, `<user>`,
//! `<host>`, `<md5-1>`, `<meta-1>`), so a line still tells what happened to which file without telling which file.
//!
//! One place decides every replacement: the app log's shareable file, the Logs panel's shareable view and a copied log
//! all go through one [`Redactor`]. Each kind can be switched off (Settings > Advanced & Logs; every kind is on by
//! default, user decision Q-P9). What the app knows is taught with `learn_*` (the opened files, the folders, the tag
//! names, the videos' titles), so these are found also where a line names them without a path or with spaces in them;
//! absolute paths and MD5 sums are also found by their shape.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Which kinds of information the shareable copy replaces (`[advanced.redact]` in `config.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactKinds {
    /// File names, folders and paths.
    pub paths: bool,
    pub tags: bool,
    /// The user's login name and the computer's host name.
    pub user: bool,
    pub md5: bool,
    /// The videos' titles and other metadata.
    pub metadata: bool,
}

impl Default for RedactKinds {
    fn default() -> Self {
        Self {
            paths: true,
            tags: true,
            user: true,
            md5: true,
            metadata: true,
        }
    }
}

/// What a placeholder stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    File,
    Dir,
    Tag,
    Md5,
    Meta,
}

impl Kind {
    const fn prefix(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::Tag => "tag",
            Self::Md5 => "md5",
            Self::Meta => "meta",
        }
    }
}

/// A taught text and how it is replaced.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Known {
    /// A whole path: its folder and its file.
    Path {
        dir: String,
        file: String,
    },
    Dir(String),
    /// A file name, or a name without its extension (the job names RustCut logs).
    File(String),
    Tag(String),
    Meta(String),
}

impl Known {
    fn text(&self) -> String {
        match self {
            Self::Path { dir, file } => format!("{dir}/{file}"),
            Self::Dir(t) | Self::File(t) | Self::Tag(t) | Self::Meta(t) => t.clone(),
        }
    }
}

/// An absolute path (`/a/b`, at least two levels) or one in the home folder (`~/a`), starting where a value starts (not
/// inside a word: `MB/s`, `x264/x265` stay; not after a colon: URLs stay); it ends at whitespace or a character that ends
/// a value, and a trailing period or colon is not part of it.
static PATH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\s="'(\[,])((?:~|/[^/\s"'`,;()\[\]{}<>|]+)/[^\s"'`,;()\[\]{}<>|]*[^\s"'`,;()\[\]{}<>|.:])"#)
        .ok()
});
static MD5: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{32}\b").ok());

/// Names shorter than this are not taught: they would replace parts of ordinary words.
const MIN_NAME: usize = 4;

/// Replaces what the kinds switched on name, with placeholders that stay the same while it lives (one per run).
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    kinds: RedactKinds,
    user: Option<String>,
    host: Option<String>,
    /// The taught texts, longest first, and the same as a set (teaching a known text again costs nothing).
    known: Vec<Known>,
    taught: HashSet<Known>,
    numbers: HashMap<(Kind, String), usize>,
    counts: HashMap<Kind, usize>,
}

impl Redactor {
    /// A redactor for this user and computer (`USER`/`LOGNAME`, the kernel's host name).
    pub fn new(kinds: RedactKinds) -> Self {
        let user = std::env::var("USER").or_else(|_unset| std::env::var("LOGNAME")).ok();
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|h| h.trim().to_owned())
            .or_else(|| std::env::var("HOSTNAME").ok());
        Self::with_identity(kinds, user, host)
    }

    /// A redactor for the given user and host names (tests, another identity).
    pub fn with_identity(kinds: RedactKinds, user: Option<String>, host: Option<String>) -> Self {
        let keep = |name: Option<String>| name.filter(|n| n.chars().count() >= 3);
        Self {
            kinds,
            user: keep(user),
            host: keep(host),
            ..Self::default()
        }
    }

    pub const fn kinds(&self) -> RedactKinds {
        self.kinds
    }

    pub const fn set_kinds(&mut self, kinds: RedactKinds) {
        self.kinds = kinds;
    }

    fn teach(&mut self, known: Known) {
        if self.taught.insert(known.clone()) {
            // Longest first: a whole path before its folder, a file name before its stem.
            let len = known.text().chars().count();
            let at = self.known.partition_point(|k| k.text().chars().count() >= len);
            self.known.insert(at, known);
        }
    }

    /// A file or folder the app works with (an opened video, a target, a folder of RustCut): its whole path, its
    /// folder, its name and its name without the extension.
    pub fn learn_path(&mut self, path: &Path) {
        let text = path.to_string_lossy().trim_end_matches('/').to_owned();
        let Some((dir, file)) = text.rsplit_once('/') else {
            return;
        };
        if file.is_empty() {
            return;
        }
        if !dir.is_empty() {
            self.teach(Known::Path {
                dir: dir.to_owned(),
                file: file.to_owned(),
            });
            self.teach(Known::Dir(dir.to_owned()));
        }
        if file.chars().count() >= MIN_NAME {
            self.teach(Known::File(file.to_owned()));
        }
        if let Some((stem, _ext)) = file.rsplit_once('.')
            && stem.chars().count() >= MIN_NAME
        {
            self.teach(Known::File(stem.to_owned()));
        }
    }

    /// A folder the app works with (a target folder, RustCut's own folders).
    pub fn learn_dir(&mut self, path: &Path) {
        let text = path.to_string_lossy().trim_end_matches('/').to_owned();
        if text.contains('/') && text.chars().count() >= MIN_NAME {
            self.teach(Known::Dir(text));
        }
    }

    /// A tag name of RustView's tree (replaced as a whole word).
    pub fn learn_tag(&mut self, name: &str) {
        let name = name.trim();
        if name.chars().count() >= MIN_NAME {
            self.teach(Known::Tag(name.to_owned()));
        }
    }

    /// A title or other metadata of a video.
    pub fn learn_meta(&mut self, text: &str) {
        let text = text.trim();
        if text.chars().count() >= MIN_NAME {
            self.teach(Known::Meta(text.to_owned()));
        }
    }

    fn number(&mut self, kind: Kind, text: &str) -> usize {
        if let Some(n) = self.numbers.get(&(kind, text.to_owned())) {
            return *n;
        }
        let count = self.counts.entry(kind).or_insert(0);
        *count += 1;
        let n = *count;
        self.numbers.insert((kind, text.to_owned()), n);
        n
    }

    fn placeholder(&mut self, kind: Kind, text: &str) -> String {
        format!("<{}-{}>", kind.prefix(), self.number(kind, text))
    }

    /// A file name as a placeholder: the extension stays (it says what kind of file), the stem gets one number for the
    /// name with and without its extension.
    fn file_placeholder(&mut self, name: &str) -> String {
        match name.rsplit_once('.') {
            Some((stem, ext))
                if !stem.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
            {
                format!("{}.{ext}", self.placeholder(Kind::File, stem))
            }
            _ => self.placeholder(Kind::File, name),
        }
    }

    fn path_placeholder(&mut self, dir: &str, file: &str) -> String {
        let dir = if dir.is_empty() {
            String::new()
        } else {
            self.placeholder(Kind::Dir, dir)
        };
        format!("{dir}/{}", self.file_placeholder(file))
    }

    fn known_placeholder(&mut self, known: &Known) -> Option<String> {
        let k = self.kinds;
        match known {
            Known::Path { dir, file } if k.paths => Some(self.path_placeholder(dir, file)),
            Known::Dir(dir) if k.paths => Some(self.placeholder(Kind::Dir, dir)),
            Known::File(name) if k.paths => Some(self.file_placeholder(name)),
            Known::Tag(name) if k.tags => Some(self.placeholder(Kind::Tag, name)),
            Known::Meta(text) if k.metadata => Some(self.placeholder(Kind::Meta, text)),
            _ => None,
        }
    }

    /// The line with every switched-on kind replaced.
    pub fn redact(&mut self, line: &str) -> String {
        let mut text = line.to_owned();
        // What the app taught: whole words for names, anywhere for paths.
        for known in self.known.clone() {
            let needle = known.text();
            if !text.contains(&needle) {
                continue;
            }
            let Some(placeholder) = self.known_placeholder(&known) else {
                continue;
            };
            text = match known {
                Known::Tag(_) | Known::File(_) | Known::Meta(_) => replace_words(&text, &needle, &placeholder),
                Known::Path { .. } | Known::Dir(_) => text.replace(&needle, &placeholder),
            };
        }
        if self.kinds.paths {
            text = self.replace_paths(&text);
        }
        if self.kinds.md5
            && let Some(md5) = MD5.as_ref()
        {
            let found: Vec<String> = md5.find_iter(&text).map(|m| m.as_str().to_owned()).collect();
            for sum in found {
                let placeholder = self.placeholder(Kind::Md5, &sum);
                text = text.replace(&sum, &placeholder);
            }
        }
        if self.kinds.user {
            for (name, placeholder) in [(self.user.clone(), "<user>"), (self.host.clone(), "<host>")] {
                if let Some(name) = name {
                    text = replace_words(&text, &name, placeholder);
                }
            }
        }
        text
    }

    /// Paths found by their shape (not taught): the folder and the file each by a placeholder.
    fn replace_paths(&mut self, text: &str) -> String {
        let Some(path) = PATH.as_ref() else {
            return text.to_owned();
        };
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let found: Vec<(usize, usize, String)> = path
            .captures_iter(text)
            .filter_map(|c| c.get(1))
            // A placeholder made from a taught path is no path any more.
            .filter(|m| !m.as_str().contains('<'))
            .map(|m| (m.start(), m.end(), m.as_str().to_owned()))
            .collect();
        for (start, end, found) in found {
            out.push_str(text.get(last..start).unwrap_or_default());
            let (dir, file) = found.rsplit_once('/').unwrap_or(("", found.as_str()));
            out.push_str(&self.path_placeholder(dir, file));
            last = end;
        }
        out.push_str(text.get(last..).unwrap_or_default());
        out
    }
}

/// `text` with every occurrence of `word` that stands alone (not inside a longer word) replaced.
fn replace_words(text: &str, word: &str, with: &str) -> String {
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(word) {
        let before = rest.get(..at).and_then(|b| b.chars().last());
        let after_at = at + word.len();
        let after = rest.get(after_at..).and_then(|a| a.chars().next());
        out.push_str(rest.get(..at).unwrap_or_default());
        // Not inside a longer word, not the name inside a placeholder (`<file-1>`), not a field's name (`file=`).
        if is_word(before) || is_word(after) || before == Some('<') || after == Some('=') {
            out.push_str(word);
        } else {
            out.push_str(with);
        }
        rest = rest.get(after_at..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
