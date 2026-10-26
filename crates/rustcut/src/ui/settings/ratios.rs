//! Settings > Filters > Crop ratios (P9.14, RustView's chip editor): the list of the crop's aspect ratios as chips, a
//! field that adds one (a ratio the list has, or that is no ratio, is refused with a bubble), a held × per chip and a
//! held reset. The list rules are pure and tested; every change goes through `SettingsWriter::update` at once
//! (`[filters] crop_ratios`), and the Filters bridge follows through the live listener.

use std::rc::Weak;
use std::time::Duration;

use rustcut_core::edit::{DEFAULT_RATIOS, MAX_RATIOS, RatioPreset, RatioSet, RatioShape};
use rustcut_core::settings::Settings;
use slint::ComponentHandle as _;

use super::SettingsBridge;
use super::handlers::{bubble_key, post_refused};
use crate::ui::notices::{self, TYPING};
use crate::util::model::sync;
use crate::{RatioChipView, SettingsUi, SettingsWindow};

/// The option's key: the bubble's anchor (`settings:filters.crop_ratios`) and the jump mark.
const KEY: &str = "filters.crop_ratios";

/// The chips of `entries` (the valid ones, in the user's order): label and shape (0 landscape, 1 portrait, 2 square).
pub fn chips(entries: &[String]) -> Vec<(String, i32)> {
    RatioSet::parse(entries)
        .0
        .presets
        .into_iter()
        .map(|p| {
            let kind = match p.ratio.shape() {
                RatioShape::Landscape => 0,
                RatioShape::Portrait => 1,
                RatioShape::Square => 2,
            };
            (p.label, kind)
        })
        .collect()
}

/// `entries` with `text` added at the end, or why it cannot be added.
pub fn added(entries: &[String], text: &str) -> Result<Vec<String>, String> {
    let preset = RatioPreset::parse(text).map_err(|e| capitalized(&e.to_string()))?;
    let set = RatioSet::parse(entries).0;
    if let Some(same) = set.presets.iter().find(|p| p.ratio == preset.ratio) {
        return Err(format!("{} is the same ratio as {}.", preset.label, same.label));
    }
    if set.presets.len() >= MAX_RATIOS {
        return Err(format!("At most {MAX_RATIOS} ratios: remove one first."));
    }
    let mut list = set.entries();
    list.push(preset.label);
    Ok(list)
}

/// `entries` without the chip at `index` (an index of the chips, which leave broken entries out).
pub fn removed(entries: &[String], index: usize) -> Vec<String> {
    let mut list = RatioSet::parse(entries).0.entries();
    if index < list.len() {
        list.remove(index);
    }
    list
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl SettingsBridge {
    pub(super) fn publish_ratios(&self, settings: &Settings) {
        let rows = chips(&settings.filters.crop_ratios)
            .into_iter()
            .map(|(label, kind)| RatioChipView {
                label: label.into(),
                kind,
            })
            .collect();
        sync(&self.models.ratios, rows);
    }

    /// The field's text: added, or refused with a bubble at the field (the field stays open).
    fn add_ratio(&self, text: &str) -> bool {
        match added(&self.writer.current().filters.crop_ratios, text) {
            Ok(list) => {
                tracing::info!(ratio = text.trim(), "[Settings] Crop ratio added");
                self.update(|s| s.filters.crop_ratios = list);
                notices::clear(&bubble_key(KEY));
                self.publish();
                true
            }
            Err(reason) => {
                post_refused(KEY, &reason, Duration::ZERO);
                false
            }
        }
    }

    /// The field with a refused text was typed in: checked again after the typing grace time.
    fn ratio_typed(&self, text: &str) {
        match added(&self.writer.current().filters.crop_ratios, text) {
            Ok(_) => notices::clear_soon(&bubble_key(KEY), TYPING),
            Err(reason) => post_refused(KEY, &reason, TYPING),
        }
    }

    fn remove_ratio(&self, index: usize) {
        self.update(|s| s.filters.crop_ratios = removed(&s.filters.crop_ratios, index));
        tracing::info!(index, "[Settings] Crop ratio removed");
        self.publish();
    }

    fn reset_ratios(&self) {
        self.update(|s| s.filters.crop_ratios = DEFAULT_RATIOS.iter().map(ToString::to_string).collect());
        tracing::info!("[Settings] Crop ratios back to the defaults");
        self.publish();
    }
}

/// Calls `f` with the bridge while it lives.
fn with<R: Default>(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge) -> R) -> R {
    this.upgrade().map(|b| f(&b)).unwrap_or_default()
}

pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    let ui = window.global::<SettingsUi<'_>>();
    let t = this.clone();
    ui.on_add_ratio(move |text| with(&t, |b| b.add_ratio(&text)));
    let t = this.clone();
    ui.on_ratio_typed(move |text| with(&t, |b| b.ratio_typed(&text)));
    ui.on_ratio_canceled(|| notices::clear(&bubble_key(KEY)));
    let t = this.clone();
    ui.on_remove_ratio(move |i| {
        if let Ok(i) = usize::try_from(i) {
            with(&t, |b| b.remove_ratio(i));
        }
    });
    let t = this.clone();
    ui.on_reset_ratios(move || with(&t, SettingsBridge::reset_ratios));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn chips_show_the_valid_ratios_with_their_shape() {
        assert_eq!(
            chips(&list(&["16:9", "x", "4:5", "1:1"])),
            vec![("16:9".into(), 0), ("4:5".into(), 1), ("1:1".into(), 2)]
        );
    }

    #[test]
    fn a_ratio_is_added_once_and_a_wrong_one_is_refused_with_a_reason() {
        let start = list(&["16:9"]);
        assert_eq!(added(&start, " 2.39:1 ").unwrap(), list(&["16:9", "2.39:1"]));
        assert_eq!(added(&start, "32:18").unwrap_err(), "32:18 is the same ratio as 16:9.");
        assert!(added(&start, "abc").unwrap_err().starts_with("'abc' is not a ratio"));
        let full: Vec<String> = (1..=32).map(|n| format!("{n}:100")).collect();
        assert!(added(&full, "7:3").unwrap_err().starts_with("At most 32"));
    }

    #[test]
    fn removing_takes_the_chip_at_its_place() {
        assert_eq!(removed(&list(&["16:9", "x", "4:5"]), 1), list(&["16:9"]));
        assert_eq!(removed(&list(&["16:9"]), 5), list(&["16:9"]));
    }
}
