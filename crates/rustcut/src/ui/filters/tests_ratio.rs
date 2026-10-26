//! Tests of the crop's ratio button, Shift and the double click (P9.14, RustView's crop tool).

#![allow(clippy::unwrap_used, reason = "test code")]

use rustcut_core::edit::{Crop, FilterKind, RatioChoice, Size};
use rustcut_core::settings::FilterDefaults;

use super::state::{CropMode, FiltersState, Kept, Source};

/// A 1920x1080 video shown 1:1 in a 960x540 area (a view pixel = two frame pixels).
fn editing(ratios: &[&str]) -> FiltersState {
    let defaults = FilterDefaults {
        crop_ratios: ratios.iter().map(ToString::to_string).collect(),
        ..FilterDefaults::default()
    };
    let mut s = FiltersState::new(defaults, true, false, Kept::default(), None);
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
    let kept = FiltersState::new(FilterDefaults::default(), true, false, Kept::default(), Some("21:9"));
    assert_eq!(kept.ratio_choice_label(), "21:9");
}

#[test]
fn a_dragged_crop_keeps_the_multiple_while_it_sets_the_output_size() {
    let mut s = editing(&["16:9"]);
    s.set_multiple(16);
    // Pull the right edge 37 view pixels (74 frame pixels) in: the width lands on a multiple of 16.
    assert!(s.crop_pointer_down(960.0, 270.0));
    s.crop_pointer_moved(923.0, 270.0, false);
    s.crop_pointer_up();
    let (w, h) = kept(&s);
    assert_eq!((w % 16, h), (0, 1072), "{w}x{h}: choosing 16 fitted the height too");
    assert_eq!(s.crop().left, 0, "only the dragged edge moves");
    // Shift moves the other axis too: both land on multiples.
    let c = s.crop();
    #[expect(clippy::cast_possible_truncation, reason = "view coordinates")]
    let right = (960.0 - f64::from(c.right) / 2.0) as f32;
    assert!(s.crop_pointer_down(right, 540.0));
    s.crop_pointer_moved(right - 30.0, 520.0, true);
    s.crop_pointer_up();
    let (w, h) = kept(&s);
    assert_eq!((w % 16, h % 16), (0, 0), "{w}x{h}");
    // A Resize after the crop sets the output: the crop is free again (even pixels).
    s.end_crop(true);
    s.set_resize_side(0, Some(1280));
    s.switch_on(FilterKind::Resize);
    assert_eq!(s.effective_multiple(), 2);
    assert!(s.resize_sets_size());
}

#[test]
fn choosing_a_multiple_fits_the_crop_and_counts_as_an_option_of_the_menu() {
    let mut s = editing(&["16:9"]);
    s.end_crop(false);
    s.set_crop(Crop::new(138, 0, 138, 0));
    assert!(s.options_are_default(FilterKind::Crop));
    s.set_multiple(8);
    assert_eq!(s.crop(), Crop::new(140, 0, 140, 0), "1920x804 becomes 1920x800");
    assert!(
        !s.chain().is_enabled(FilterKind::Crop),
        "an option keeps the filter off"
    );
    assert!(!s.options_are_default(FilterKind::Crop));
    assert_eq!(s.options_as_defaults(FilterKind::Crop).crop_multiple, 8);
    s.reset_options(FilterKind::Crop);
    assert_eq!(s.multiple, 2);
    assert_eq!(s.kept().multiple, None, "nothing is kept while keep is off");
}

#[test]
fn a_click_while_picking_names_the_pixel_of_the_crops_frame() {
    use rustcut_core::edit::Orientation;

    use super::geometry::CropView;
    let hd = Size::new(1920, 1080);
    // Shown half size: the view's (480.4, 270.9) is the frame's pixel (960, 541); outside the picture there is none.
    let v = CropView::new(hd, 1.0, Orientation::IDENTITY, 960.0, 540.0).unwrap();
    assert_eq!(v.frame_point(480.4, 270.9), Some((960, 541)));
    assert_eq!(v.frame_point(-1.0, 10.0), None);
    // Turned clockwise after the crop: the pixel maps back to the place it is shown.
    let turned = CropView::new(hd, 1.0, Orientation::CLOCKWISE, 1080.0, 1920.0).unwrap();
    let (x, y) = turned.frame_point(100.5, 50.5).unwrap();
    let one = Crop::new(y, 1920 - x - 1, 1080 - y - 1, x);
    assert_eq!(turned.shown_crop(one), Crop::new(50, 1080 - 101, 1920 - 51, 100));
}

#[test]
fn picking_shows_the_whole_frame_and_leaves_the_crop_as_it_was() {
    let mut s = editing(&["16:9"]);
    s.end_crop(false);
    assert!(s.begin_crop(CropMode::Pick));
    assert!(super::view::card(&s).picking);
    assert!(!s.crop_pointer_down(480.0, 270.0), "a click picks, it grabs nothing");
    s.end_crop(false);
    assert!(!s.chain().is_enabled(FilterKind::Crop));
    assert!(!super::view::card(&s).picking);
    // The picked color is the video's: the menu's options still equal the defaults, and a save leaves it out.
    s.set_search(|search| search.color = Some([255, 255, 255]));
    assert!(s.options_are_default(FilterKind::Crop));
    assert_eq!(s.options_as_defaults(FilterKind::Crop).crop_search.color, None);
    s.reset_options(FilterKind::Crop);
    assert_eq!(s.search.color, None);
}
