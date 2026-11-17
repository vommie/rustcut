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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
        match self.choice {
            SortingChoice::Natural => {
                let a: Vec<char> = a.chars().collect();
                let b: Vec<char> = b.chars().collect();
                let (base_a, ext_a) = split_extension(&a);
                let (base_b, ext_b) = split_extension(&b);
                let result = self.decimal_aware(base_a, base_b);
                if result != Ordering::Equal || (ext_a.is_empty() && ext_b.is_empty()) {
                    return result;
                }
                self.decimal_aware(ext_a, ext_b)
            }
            SortingChoice::CaseInsensitive => compare_folded(a, b).then_with(|| a.cmp(b)),
            SortingChoice::CaseSensitive => a.cmp(b),
        }
    }

    fn collate(&self, a: &[char], b: &[char]) -> Ordering {
        let a: String = a.iter().collect();
        let b: String = b.iter().collect();
        match &self.collator {
            Some(collator) => collator.compare(&a, &b),
            None => compare_folded(&a, &b),
        }
    }

    /// Dolphin's `decimalAwareNaturalCompare`.
    fn decimal_aware(&self, a: &[char], b: &[char]) -> Ordering {
        let (mut ia, mut ib) = (0, 0);
        let mut numbers = false;
        while ia < a.len() && ib < b.len() {
            let digits = is_digit(a.get(ia)) && is_digit(b.get(ib));
            if digits {
                numbers = true;
                let chain_a = numeric_chain(a, ia);
                let chain_b = numeric_chain(b, ib);
                let result = compare_chains(&chain_a, &chain_b);
                ia = chain_a.end;
                ib = chain_b.end;
                if result != Ordering::Equal {
                    return result;
                }
            } else {
                let end_a = text_run_end(a, ia);
                let end_b = text_run_end(b, ib);
                let run = self.collate(
                    a.get(ia..end_a).unwrap_or_default(),
                    b.get(ib..end_b).unwrap_or_default(),
                );
                if run != Ordering::Equal {
                    return self.collate(a.get(ia..).unwrap_or_default(), b.get(ib..).unwrap_or_default());
                }
                ia = end_a;
                ib = end_b;
            }
        }
        let rest = self.collate(a.get(ia..).unwrap_or_default(), b.get(ib..).unwrap_or_default());
        if rest != Ordering::Equal || !numbers {
            return rest;
        }
        let a: String = a.iter().collect();
        let b: String = b.iter().collect();
        compare_folded(&a, &b).then_with(|| a.cmp(&b))
    }
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

fn is_digit(c: Option<&char>) -> bool {
    c.is_some_and(char::is_ascii_digit)
}

/// Dolphin's `findExtensionSeparator`: the base name and the extension (with its dot).
fn split_extension(name: &[char]) -> (&[char], &[char]) {
    for index in (1..name.len()).rev() {
        let Some(&c) = name.get(index) else { continue };
        if c.is_whitespace() {
            break;
        }
        if c != '.' {
            continue;
        }
        let between_digits = index
            .checked_sub(1)
            .is_some_and(|before| is_digit(name.get(before)) && is_digit(name.get(index + 1)));
        if between_digits {
            continue;
        }
        return (
            name.get(..index).unwrap_or_default(),
            name.get(index..).unwrap_or_default(),
        );
    }
    (name, &[])
}

fn text_run_end(text: &[char], start: usize) -> usize {
    (start..text.len())
        .find(|&i| is_digit(text.get(i)))
        .unwrap_or(text.len())
}

/// A chain of digit runs joined by single dots (`12`, `1.5`, `1.2.3`), as digit strings.
#[derive(Debug)]
struct Chain {
    segments: Vec<String>,
    end: usize,
}

fn numeric_chain(text: &[char], start: usize) -> Chain {
    let mut segments = Vec::new();
    let mut index = start;
    loop {
        let run_start = index;
        while is_digit(text.get(index)) {
            index += 1;
        }
        segments.push(text.get(run_start..index).unwrap_or_default().iter().collect());
        if text.get(index) == Some(&'.') && is_digit(text.get(index + 1)) {
            index += 1;
        } else {
            break;
        }
    }
    Chain { segments, end: index }
}

/// Dolphin's `compareNumericChains`.
fn compare_chains(a: &Chain, b: &Chain) -> Ordering {
    if let ([int_a, frac_a], [int_b, frac_b]) = (a.segments.as_slice(), b.segments.as_slice()) {
        return compare_digits(int_a, int_b).then_with(|| compare_fraction(frac_a, frac_b));
    }
    let mut left = a.segments.iter();
    let mut right = b.segments.iter();
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
    let width = a.len().max(b.len());
    format!("{a:0<width$}").cmp(&format!("{b:0<width$}"))
}

#[cfg(test)]
#[path = "natural_tests.rs"]
mod tests;
