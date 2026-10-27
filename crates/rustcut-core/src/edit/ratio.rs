//! Aspect ratios of the crop (pure; P9.14, the review's note 13): the presets the user defines in Settings > Filters, whether
//! each is landscape, portrait or square, and the two cycles of the crop's ratio button (left click: landscape, right
//! click: portrait). The source's own ratio ("O") is always the first entry of both cycles.
//!
//! Ported from RustView `crates/rustview-core/src/image_edit/ratio.rs` (RustView is read-only for RustCut): the same
//! parsing, cycles and labels, so both apps behave alike. RustCut's changes: `RatioShape` (RustCut has an `Orientation`
//! of its own), the source's ratio as a display ratio (`value`, a video's pixels need not be square), the default list
//! and `choice_of` (the choice is kept in `ui-state.toml` by its label).

use thiserror::Error;

/// The label of the original ratio of the picture.
pub const ORIGINAL_LABEL: &str = "O";

/// The label of a rectangle whose proportions no longer follow the chosen ratio (it was resized without Shift).
pub const FREE_LABEL: &str = "Free";

/// A preset may not be more extreme than this (100:1 or 1:100); a crop of such a shape is never what was meant.
const MAX_RATIO: u64 = 100;

/// Each side of a preset may have at most this many decimals ("2.39:1").
const MAX_DECIMALS: usize = 3;

/// The ratios a fresh Settings > Filters offers (RustView's list without 3:2, 16:10, 21:9 and 2:3: the user's review 2;
/// the migration `crop-ratios-trimmed` updates an untouched list).
pub const DEFAULT_RATIOS: &[&str] = &["1:1", "5:4", "4:3", "16:9", "4:5", "3:4", "9:16"];

/// At most this many presets are kept.
pub const MAX_RATIOS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatioShape {
    Landscape,
    Portrait,
    Square,
}

/// A ratio of two whole numbers, reduced to lowest terms (so 8:6 and 4:3 are the same ratio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

impl AspectRatio {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let divisor = gcd(width, height);
        Some(Self {
            width: width / divisor,
            height: height / divisor,
        })
    }

    pub fn shape(self) -> RatioShape {
        match self.width.cmp(&self.height) {
            std::cmp::Ordering::Greater => RatioShape::Landscape,
            std::cmp::Ordering::Less => RatioShape::Portrait,
            std::cmp::Ordering::Equal => RatioShape::Square,
        }
    }

    /// Width divided by height.
    pub fn value(self) -> f64 {
        f64::from(self.width) / f64::from(self.height)
    }

    /// The same shape turned by a quarter.
    pub fn flipped(self) -> Self {
        Self {
            width: self.height,
            height: self.width,
        }
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RatioError {
    #[error("'{0}' is not a ratio; write it as width:height, for example 16:9")]
    Syntax(String),
    #[error("'{0}': both sides must be greater than zero")]
    Zero(String),
    #[error("'{0}' is too extreme; a ratio may be at most 100:1")]
    Extreme(String),
    #[error("'{0}' is the same ratio as '{1}'")]
    Duplicate(String, String),
}

/// One ratio the user defined: the ratio and the label it is shown with ("16:9", "2.39:1").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatioPreset {
    pub ratio: AspectRatio,
    pub label: String,
}

impl RatioPreset {
    /// Reads "16:9", "16 : 9", "16x9", "16/9", "2.39:1" or a single number ("1.85" = 1.85:1).
    pub fn parse(text: &str) -> Result<Self, RatioError> {
        let trimmed = text.trim();
        let syntax = || RatioError::Syntax(trimmed.to_string());
        let mut parts = trimmed.split([':', 'x', 'X', '/']);
        let first = parts.next().ok_or_else(syntax)?.trim();
        let second = parts.next().map_or("1", str::trim);
        if parts.next().is_some() || first.is_empty() || second.is_empty() {
            return Err(syntax());
        }
        let (w, w_scale) = parse_decimal(first).ok_or_else(syntax)?;
        let (h, h_scale) = parse_decimal(second).ok_or_else(syntax)?;
        // Bring both sides to the same number of decimals: 2.39:1 -> 239:100.
        let scale = w_scale.max(h_scale);
        let w = w * 10u64.pow(scale - w_scale);
        let h = h * 10u64.pow(scale - h_scale);
        if w == 0 || h == 0 {
            return Err(RatioError::Zero(trimmed.to_string()));
        }
        if w > h * MAX_RATIO || h > w * MAX_RATIO {
            return Err(RatioError::Extreme(trimmed.to_string()));
        }
        let narrow = |v: u64| u32::try_from(v).map_err(|_too_large| RatioError::Extreme(trimmed.to_string()));
        let ratio = AspectRatio::new(narrow(w)?, narrow(h)?).ok_or_else(|| RatioError::Zero(trimmed.to_string()))?;
        Ok(Self {
            ratio,
            label: format!("{first}:{second}"),
        })
    }
}

/// A non-negative number with at most `MAX_DECIMALS` decimals as an integer and its number of decimals ("2.39" -> (239, 2)).
fn parse_decimal(text: &str) -> Option<(u64, u32)> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.chars().chain(fraction.chars()).all(|c| c.is_ascii_digit()) || fraction.len() > MAX_DECIMALS {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let value = digits.parse::<u64>().ok()?;
    if value > 1_000_000_000 {
        return None;
    }
    Some((value, u32::try_from(fraction.len()).ok()?))
}

/// The presets of the settings, in the user's order, and what was wrong with the entries that are not used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RatioSet {
    pub presets: Vec<RatioPreset>,
}

impl RatioSet {
    /// Reads the entries of the settings. Invalid entries and repetitions of a ratio are left out and reported.
    pub fn parse(entries: &[String]) -> (Self, Vec<RatioError>) {
        let mut presets: Vec<RatioPreset> = Vec::with_capacity(entries.len());
        let mut errors = Vec::new();
        for entry in entries {
            match RatioPreset::parse(entry) {
                Ok(preset) => {
                    if let Some(same) = presets.iter().find(|p| p.ratio == preset.ratio) {
                        errors.push(RatioError::Duplicate(preset.label, same.label.clone()));
                    } else {
                        presets.push(preset);
                    }
                }
                Err(e) => errors.push(e),
            }
        }
        (Self { presets }, errors)
    }

    /// The cycle of the left (landscape) or right (portrait) click: the original ratio first, then the presets of that
    /// orientation and the square ones, in the user's order.
    pub fn cycle(&self, landscape: bool) -> Vec<RatioChoice> {
        let wanted = if landscape {
            RatioShape::Landscape
        } else {
            RatioShape::Portrait
        };
        std::iter::once(RatioChoice::Original)
            .chain(
                self.presets
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| matches!(p.ratio.shape(), o if o == wanted || o == RatioShape::Square))
                    .map(|(index, _)| RatioChoice::Preset(index)),
            )
            .collect()
    }

    /// The choice after `current` for a click of the ratio button. A choice of the other cycle goes to the first preset of
    /// this one (the original ratio when there is none), so one click always switches between landscape and portrait.
    pub fn next(&self, current: RatioChoice, landscape: bool) -> RatioChoice {
        let cycle = self.cycle(landscape);
        match cycle.iter().position(|c| *c == current) {
            Some(index) => cycle
                .get((index + 1) % cycle.len())
                .copied()
                .unwrap_or(RatioChoice::Original),
            None => cycle.get(1).copied().unwrap_or(RatioChoice::Original),
        }
    }

    /// The display ratio (width / height) of a choice; the original ratio is the source's (`original`, its display
    /// width / height). `None` for a preset that does not exist or an unusable original.
    pub fn value(&self, choice: RatioChoice, original: f64) -> Option<f64> {
        match choice {
            RatioChoice::Original => (original.is_finite() && original > 0.0).then_some(original),
            RatioChoice::Preset(index) => self.presets.get(index).map(|p| p.ratio.value()),
        }
    }

    /// The choice shown as `label` ("O" or a preset's label, as `label` gives it); `None` for an unknown label.
    pub fn choice_of(&self, label: &str) -> Option<RatioChoice> {
        if label == ORIGINAL_LABEL {
            return Some(RatioChoice::Original);
        }
        self.presets
            .iter()
            .position(|p| p.label == label)
            .map(RatioChoice::Preset)
    }

    /// The entries to keep in the settings: the valid ones in the user's order, each ratio once, at most
    /// [`MAX_RATIOS`].
    pub fn entries(&self) -> Vec<String> {
        self.presets.iter().take(MAX_RATIOS).map(|p| p.label.clone()).collect()
    }

    pub fn label(&self, choice: RatioChoice) -> &str {
        match choice {
            RatioChoice::Original => ORIGINAL_LABEL,
            RatioChoice::Preset(index) => self.presets.get(index).map_or(ORIGINAL_LABEL, |p| p.label.as_str()),
        }
    }
}

/// A ratio the user chose with the ratio button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RatioChoice {
    #[default]
    Original,
    /// Index into `RatioSet::presets`.
    Preset(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(entries: &[&str]) -> RatioSet {
        let (set, errors) = RatioSet::parse(&entries.iter().map(ToString::to_string).collect::<Vec<_>>());
        assert!(errors.is_empty(), "{errors:?}");
        set
    }

    #[test]
    fn presets_are_read_in_many_spellings_and_reduced() {
        for (text, w, h, label) in [
            ("16:9", 16, 9, "16:9"),
            (" 4 : 3 ", 4, 3, "4:3"),
            ("3x2", 3, 2, "3:2"),
            ("9/16", 9, 16, "9:16"),
            ("2.39:1", 239, 100, "2.39:1"),
            ("1.85", 37, 20, "1.85:1"),
            ("8:6", 4, 3, "8:6"),
            ("1:1", 1, 1, "1:1"),
        ] {
            let preset = RatioPreset::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!((preset.ratio.width, preset.ratio.height), (w, h), "{text}");
            assert_eq!(preset.label, label);
        }
    }

    #[test]
    fn invalid_presets_are_refused_with_a_reason() {
        assert!(matches!(RatioPreset::parse(""), Err(RatioError::Syntax(_))));
        assert!(matches!(RatioPreset::parse("abc"), Err(RatioError::Syntax(_))));
        assert!(matches!(RatioPreset::parse("16:9:1"), Err(RatioError::Syntax(_))));
        assert!(matches!(RatioPreset::parse("-4:3"), Err(RatioError::Syntax(_))));
        assert!(matches!(RatioPreset::parse("1.2345:1"), Err(RatioError::Syntax(_))));
        assert!(matches!(RatioPreset::parse("0:9"), Err(RatioError::Zero(_))));
        assert!(matches!(RatioPreset::parse("101:1"), Err(RatioError::Extreme(_))));
        assert!(RatioPreset::parse("100:1").is_ok());
    }

    #[test]
    fn repeated_ratios_are_left_out() {
        let (set, errors) = RatioSet::parse(&["4:3".to_string(), "8:6".to_string(), "x".to_string()]);
        assert_eq!(set.presets.len(), 1);
        assert_eq!(errors.len(), 2);
        assert!(matches!(&errors[0], RatioError::Duplicate(a, b) if a == "8:6" && b == "4:3"));
    }

    #[test]
    fn the_shape_is_detected_from_the_numbers() {
        assert_eq!(
            RatioPreset::parse("16:9").map(|p| p.ratio.shape()),
            Ok(RatioShape::Landscape)
        );
        assert_eq!(
            RatioPreset::parse("4:5").map(|p| p.ratio.shape()),
            Ok(RatioShape::Portrait)
        );
        assert_eq!(
            RatioPreset::parse("2:2").map(|p| p.ratio.shape()),
            Ok(RatioShape::Square)
        );
    }

    #[test]
    fn left_click_cycles_landscape_and_right_click_portrait_with_the_original_first() {
        let set = set(&["1:1", "16:9", "4:5", "4:3", "9:16"]);
        assert_eq!(
            set.cycle(true),
            vec![
                RatioChoice::Original,
                RatioChoice::Preset(0),
                RatioChoice::Preset(1),
                RatioChoice::Preset(3)
            ]
        );
        assert_eq!(
            set.cycle(false),
            vec![
                RatioChoice::Original,
                RatioChoice::Preset(0),
                RatioChoice::Preset(2),
                RatioChoice::Preset(4)
            ]
        );
        let mut choice = RatioChoice::Original;
        let mut labels = Vec::new();
        for _ in 0..5 {
            choice = set.next(choice, true);
            labels.push(set.label(choice).to_string());
        }
        assert_eq!(labels, ["1:1", "16:9", "4:3", "O", "1:1"]);
    }

    #[test]
    fn switching_between_the_cycles_goes_to_the_first_preset_of_the_other_one() {
        let set = set(&["16:9", "4:5", "21:9", "9:16"]);
        let landscape = set.next(RatioChoice::Original, true);
        assert_eq!(set.label(landscape), "16:9");
        let portrait = set.next(landscape, false);
        assert_eq!(set.label(portrait), "4:5");
        assert_eq!(set.label(set.next(portrait, true)), "16:9");
        // Without presets of an orientation the button stays on the original ratio.
        let only_landscape = self::set(&["16:9"]);
        assert_eq!(only_landscape.next(RatioChoice::Original, false), RatioChoice::Original);
    }

    #[test]
    fn the_original_choice_is_the_sources_display_ratio() {
        let set = set(&["4:5"]);
        // Anamorphic PAL 16:9: 720x576 pixels of 64:45 show as 16:9.
        assert_eq!(
            set.value(RatioChoice::Original, 720.0 * 64.0 / 45.0 / 576.0),
            Some(16.0 / 9.0)
        );
        assert_eq!(set.value(RatioChoice::Preset(0), 1.0), Some(0.8));
        assert_eq!(set.value(RatioChoice::Preset(9), 1.0), None);
        assert_eq!(set.value(RatioChoice::Original, 0.0), None);
    }

    #[test]
    fn a_choice_is_kept_by_its_label() {
        let set = set(&["16:9", "2.39:1"]);
        assert_eq!(set.choice_of("O"), Some(RatioChoice::Original));
        assert_eq!(set.choice_of("2.39:1"), Some(RatioChoice::Preset(1)));
        assert_eq!(set.choice_of("4:3"), None);
        assert_eq!(set.label(RatioChoice::Preset(1)), "2.39:1");
        let (defaults, errors) = RatioSet::parse(&DEFAULT_RATIOS.iter().map(ToString::to_string).collect::<Vec<_>>());
        assert!(errors.is_empty());
        assert_eq!(defaults.entries().len(), DEFAULT_RATIOS.len());
    }
}
