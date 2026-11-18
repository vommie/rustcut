//! Dolphin's name order, ported from KDE Dolphin's `KFileItemModel::stringCompare` and its decimal-aware natural
//! compare, so "the next file" is the one Dolphin shows below the current one.
//!
//! Natural mode (Dolphin's default): the extension is split off (the last `.` that is not between two digits, never
//! when the suffix holds whitespace), base names are compared run by run (digit runs as numbers, `N.M` as a decimal
//! fraction so `1.10` < `1.9`, longer chains segment by segment; text runs with the locale's collator, case
//! insensitive), then the extensions; equal names with numbers fall back to a plain string compare. The two other
//! modes compare code points (case sensitive or not).

use std::cmp::Ordering;

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed};
use icu_locale_core::Locale;

/// Dolphin's `[General] SortingChoice` in `dolphinrc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortingChoice {
    /// Numbers by value, case insensitive, locale-aware (Dolphin's default).
    #[default]
    Natural,
    /// Code points, case insensitive.
    CaseInsensitive,
    /// Code points, case sensitive.
    CaseSensitive,
}

impl SortingChoice {
    /// Reads the value Dolphin stores (`0`/`NaturalSorting`, `1`/`CaseInsensitiveSorting`, `2`/`CaseSensitiveSorting`).
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        match value.trim() {
            "1" | "CaseInsensitiveSorting" => Self::CaseInsensitive,
            "2" | "CaseSensitiveSorting" => Self::CaseSensitive,
            _ => Self::Natural,
        }
    }
}

/// Compares file names like Dolphin.
pub struct NameOrder {
    choice: SortingChoice,
    collator: Option<CollatorBorrowed<'static>>,
}

impl std::fmt::Debug for NameOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NameOrder")
            .field("choice", &self.choice)
            .finish_non_exhaustive()
    }
}

impl NameOrder {
    /// A comparer for `locale` (a POSIX name like `de_DE.UTF-8` or a BCP 47 tag; unknown ones use the root order).
    #[must_use]
    pub fn new(choice: SortingChoice, locale: &str) -> Self {
        let mut options = CollatorOptions::default();
        options.strength = Some(Strength::Secondary);
        let locale = parse_locale(locale);
        let collator = Collator::try_new(locale.into(), options)
            .or_else(|_| Collator::try_new(Locale::UNKNOWN.into(), options))
            .ok();
        Self { choice, collator }
    }

    /// The comparer for the process's locale (`LC_ALL`, `LC_COLLATE`, `LANG`).
    #[must_use]
    pub fn for_environment(choice: SortingChoice) -> Self {
        let locale = ["LC_ALL", "LC_COLLATE", "LANG"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::new(choice, &locale)
    }

    /// Dolphin's `stringCompare`.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        self.compare_keys(&NameKey::new(a), &NameKey::new(b))
    }

    /// A name split once for [`NameOrder::compare_keys`]. (ICU sort keys of every run and rest were measured on
    /// 100 000 names: building them took longer than the comparisons they saved, 2026-11-18.)
    #[must_use]
    pub fn key(&self, name: &str) -> NameKey {
        NameKey::new(name)
    }

    /// Compares names split once with [`NameKey::new`] (sorting a large folder splits every name once, not per
    /// comparison).
    #[must_use]
    pub fn compare_keys(&self, a: &NameKey, b: &NameKey) -> Ordering {
        match self.choice {
            SortingChoice::Natural => {
                let result = self.decimal_aware(a.base(), &a.base_tokens, b.base(), &b.base_tokens);
                if result != Ordering::Equal || (a.ext().is_empty() && b.ext().is_empty()) {
                    return result;
                }
                self.decimal_aware(a.ext(), &a.ext_tokens, b.ext(), &b.ext_tokens)
            }
            SortingChoice::CaseInsensitive => compare_folded(&a.text, &b.text).then_with(|| a.text.cmp(&b.text)),
            SortingChoice::CaseSensitive => a.text.cmp(&b.text),
        }
    }

    fn collate(&self, a: &str, b: &str) -> Ordering {
        // Equal bytes collate equal: most runs of a folder's names are the same words ("Clip ", " [take ").
        if a == b {
            return Ordering::Equal;
        }
        match &self.collator {
            Some(collator) => collator.compare(a, b),
            None => compare_folded(a, b),
        }
    }

    /// Dolphin's `decimalAwareNaturalCompare`, token by token: a text run on one side and a number on the other
    /// compare the run with nothing; differing runs decide by the whole rest; equal numbers go on.
    fn decimal_aware(&self, a: &str, ta: &[Token], b: &str, tb: &[Token]) -> Ordering {
        let (mut i, mut j) = (0, 0);
        let mut numbers = false;
        while let (Some(x), Some(y)) = (ta.get(i), tb.get(j)) {
            if x.number && y.number {
                numbers = true;
                let result = compare_chains(x.text(a), y.text(b));
                if result != Ordering::Equal {
                    return result;
                }
                i += 1;
                j += 1;
            } else {
                let run_a = if x.number { "" } else { x.text(a) };
                let run_b = if y.number { "" } else { y.text(b) };
                if self.collate(run_a, run_b) != Ordering::Equal {
                    return self.collate(x.rest(a), y.rest(b));
                }
                i += usize::from(!x.number);
                j += usize::from(!y.number);
            }
        }
        let rest_a = ta.get(i).map_or("", |t| t.rest(a));
        let rest_b = tb.get(j).map_or("", |t| t.rest(b));
        let rest = self.collate(rest_a, rest_b);
        if rest != Ordering::Equal || !numbers {
            return rest;
        }
        compare_folded(a, b).then_with(|| a.cmp(b))
    }
}

/// A file name split once for [`NameOrder::compare_keys`]: the base name and the extension, each as runs of text and
/// numbers (byte ranges into the name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameKey {
    text: Box<str>,
    /// Where the extension (with its dot) starts; the name's length without one.
    split: usize,
    base_tokens: Vec<Token>,
    ext_tokens: Vec<Token>,
}

impl NameKey {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let split = extension_start(name).unwrap_or(name.len());
        let base = name.get(..split).unwrap_or_default();
        let ext = name.get(split..).unwrap_or_default();
        Self {
            base_tokens: tokens(base),
            ext_tokens: tokens(ext),
            split,
            text: name.into(),
        }
    }

    /// The whole name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.text
    }

    fn base(&self) -> &str {
        self.text.get(..self.split).unwrap_or_default()
    }

    fn ext(&self) -> &str {
        self.text.get(self.split..).unwrap_or_default()
    }
}

/// A run of a name part: text up to the next digit, or a number chain (`12`, `1.5`, `1.2.3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Token {
    start: u32,
    end: u32,
    number: bool,
}

impl Token {
    fn text(self, part: &str) -> &str {
        part.get(self.start as usize..self.end as usize).unwrap_or_default()
    }

    /// The part from this token to its end.
    fn rest(self, part: &str) -> &str {
        part.get(self.start as usize..).unwrap_or_default()
    }
}

/// The runs of a part. Digits and dots are ASCII, so every boundary is a character boundary.
fn tokens(part: &str) -> Vec<Token> {
    let bytes = part.as_bytes();
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let number = digit(i);
        if number {
            loop {
                while digit(i) {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') && digit(i + 1) {
                    i += 1;
                } else {
                    break;
                }
            }
        } else {
            while i < bytes.len() && !digit(i) {
                i += 1;
            }
        }
        let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(i)) else {
            break;
        };
        out.push(Token { start, end, number });
    }
    out
}

/// `de_DE.UTF-8@euro` -> `de-DE`; `C`/`POSIX` and garbage -> unknown (root order).
fn parse_locale(posix: &str) -> Locale {
    let name = posix.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    if name.is_empty() || name == "C" || name == "POSIX" {
        return Locale::UNKNOWN;
    }
    name.parse().unwrap_or(Locale::UNKNOWN)
}

/// Case-insensitive code point order (Qt's `QString::compare` with `Qt::CaseInsensitive` folds case per character).
fn compare_folded(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// Dolphin's `findExtensionSeparator`: the byte index of the extension's dot (the last `.` that is not between two
/// digits, from the second character on); `None` when there is none or the suffix holds whitespace.
fn extension_start(name: &str) -> Option<usize> {
    let bytes = name.as_bytes();
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    for (index, c) in name.char_indices().rev() {
        if index == 0 {
            return None;
        }
        if c.is_whitespace() {
            return None;
        }
        if c == '.' && !(digit(index - 1) && digit(index + 1)) {
            return Some(index);
        }
    }
    None
}

/// Dolphin's `compareNumericChains`: `N.M` against `N.M` as decimal fractions, other chains segment by segment.
fn compare_chains(a: &str, b: &str) -> Ordering {
    let (count_a, count_b) = (a.split('.').count(), b.split('.').count());
    if count_a == 2 && count_b == 2 {
        let (int_a, frac_a) = a.split_once('.').unwrap_or((a, ""));
        let (int_b, frac_b) = b.split_once('.').unwrap_or((b, ""));
        return compare_digits(int_a, int_b).then_with(|| compare_fraction(frac_a, frac_b));
    }
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (Some(x), Some(y)) => {
                let result = compare_digits(x, y);
                if result != Ordering::Equal {
                    return result;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// Whole numbers of any length: leading zeros dropped, the shorter is smaller.
fn compare_digits(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Fractions: the shorter is padded with zeros (`.5` == `.50`, `.10` < `.9`).
fn compare_fraction(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    (0..a.len().max(b.len()))
        .map(|i| {
            a.get(i)
                .copied()
                .unwrap_or(b'0')
                .cmp(&b.get(i).copied().unwrap_or(b'0'))
        })
        .find(|o| *o != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

#[cfg(test)]
#[path = "natural_tests.rs"]
mod tests;
