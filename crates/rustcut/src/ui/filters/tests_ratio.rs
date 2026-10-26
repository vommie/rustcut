//! Tests of the crop's ratio button, Shift and the double click (P9.14, RustView's crop tool).

#![allow(clippy::unwrap_used, reason = "test code")]

use rustcut_core::edit::{Crop, FilterKind, RatioChoice, Size};
use rustcut_core::settings::FilterDefaults;

use super::state::{CropMode, FiltersState, Source};

/// A 1920x1080 video shown 1:1 in a 960x540 area (a view pixel = two frame pixels).
fn editing(ratios: &[&str]) -> FiltersState {
    let defaults = FilterDefaults {
        crop_ratios: ratios.iter().map(ToString::to_string).collect(),
        ..FilterDefaults::default()
    };
    let mut s = FiltersState::new(defaults, true, false, (None, None), None);
    s.file_opened();
    s.source_known(Source {
        size: Size::new(1920, 1080),
        sar: 1.0,
        rotation: 0,
        duration: None,
    });
    s.set_view(960.0, 540.0);
    assert!(s.begin_crop(CropMode::Edit));
    s
}

fn kept(s: &FiltersState) -> (u32, u32) {
    let c = s.crop();
    (1920 - c.left - c.right, 1080 - c.top - c.bottom)
}

#[test]
fn the_ratio_button_goes_through_the_presets_and_keeps_the_area() {
    let mut s = editing(&["1:1", "16:9", "4:5", "9:16"]);
    assert_eq!(s.ratio_label(), "O", "the whole 16:9 frame has the source's ratio");
    assert!(s.cycle_ratio(true));
    assert_eq!(s.ratio_label(), "1:1");
    let (w, h) = kept(&s);
    assert_eq!(w, h);
    assert_eq!(h, 1080, "the largest square the frame holds, in the middle");
    assert_eq!(s.crop().left, s.crop().right);
    assert!(s.cycle_ratio(true));
    assert_eq!((s.ratio_label(), kept(&s)), ("16:9", (1920, 1080)));
    // A right click goes to the portrait ones.
    assert!(s.cycle_ratio(false));
    assert_eq!(s.ratio_label(), "1:1");
    assert!(s.cycle_ratio(false));
    assert_eq!(s.ratio_label(), "4:5");
    let (w, h) = kept(&s);
    assert!((f64::from(w) / f64::from(h) - 0.8).abs() < 0.01, "{w}x{h}");
    assert!(s.chain().is_enabled(FilterKind::Crop));
}

#[test]
fn a_free_drag_leaves_the_ratio_and_shift_keeps_it() {
    let mut s = editing(&["4:3"]);
    s.cycle_ratio(true);
    assert_eq!(s.ratio_label(), "4:3");
    // The 4:3 crop is 1440x1080 in the middle: its right edge is at view x 840. A free drag changes its shape.
    assert!(s.crop_pointer_down(840.0, 270.0));
    s.crop_pointer_moved(800.0, 270.0, false);
    s.crop_pointer_up();
    assert_eq!(s.ratio_label(), "Free");
    // Shift keeps the crop's own shape now; a corner drag scales it.
    let before = kept(&s);
    let ratio = f64::from(before.0) / f64::from(before.1);
    let c = s.crop();
    let (right, bottom) = (960.0 - f64::from(c.right) / 2.0, 540.0 - f64::from(c.bottom) / 2.0);
    #[expect(clippy::cast_possible_truncation, reason = "view coordinates")]
    let (x, y) = (right as f32, bottom as f32);
    assert!(s.crop_pointer_down(x, y));
    s.crop_pointer_moved(x - 60.0, y - 20.0, true);
    s.crop_pointer_up();
    let after = kept(&s);
    assert_ne!(after, before);
    assert!(
        (f64::from(after.0) / f64::from(after.1) - ratio).abs() < 0.02,
        "{before:?} -> {after:?}"
    );
    // The button places the chosen ratio again: 4:3 with the area of the crop just made.
    s.cycle_ratio(true);
    assert_eq!(s.ratio_label(), "O");
    s.cycle_ratio(true);
    assert_eq!(s.ratio_label(), "4:3");
}

#[test]
fn a_double_click_inside_keeps_the_crop() {
    let mut s = editing(&["4:3"]);
    s.cycle_ratio(true);
    assert!(s.inside_crop(480.0, 270.0));
    assert!(!s.inside_crop(20.0, 270.0), "outside the 4:3 crop");
    s.end_crop(true);
    assert!(!s.inside_crop(480.0, 270.0), "no session");
    assert_eq!(s.crop(), Crop::new(0, 240, 0, 240));
}

#[test]
fn the_choice_survives_a_change_of_the_presets_by_its_label() {
    let mut s = editing(&["16:9", "4:3"]);
    s.cycle_ratio(true);
    s.cycle_ratio(true);
    assert_eq!(s.ratio_choice_label(), "4:3");
    s.set_defaults(FilterDefaults {
        crop_ratios: vec!["4:3".into()],
        ..FilterDefaults::default()
    });
    assert_eq!((s.ratio, s.ratio_choice_label()), (RatioChoice::Preset(0), "4:3"));
    s.set_defaults(FilterDefaults {
        crop_ratios: vec!["1:1".into()],
        ..FilterDefaults::default()
    });
    assert_eq!(
        s.ratio_choice_label(),
        "O",
        "a removed ratio falls back to the source's"
    );
    let kept = FiltersState::new(FilterDefaults::default(), true, false, (None, None), Some("21:9"));
    assert_eq!(kept.ratio_choice_label(), "21:9");
}
