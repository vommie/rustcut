//! Tests of the options menus, the rows' context menus and drag & drop of the Filters card (P9.13 steps 2 and 3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code"
)]

use rustcut_core::edit::{
    DeinterlaceRate, Deinterlacer, DeshakeParams, FieldParity, Filter, FilterKind, ResizeAlgorithm, Size,
};
use rustcut_core::media::CropSides;
use rustcut_core::settings::FilterDefaults;

use super::input::typed_deshake;
use super::menu::row_items;
use super::state::{FiltersState, Kept, Source};
use super::view;
use crate::icons::Icon;
use crate::ui::menu::MenuItem;

fn state_with(defaults: FilterDefaults) -> FiltersState {
    let mut s = FiltersState::new(defaults, true, false, Kept::default(), None);
    s.file_opened();
    s.source_known(Source {
        size: Size::new(1920, 1080),
        sar: 1.0,
        rotation: 0,
        duration: None,
    });
    s
}

fn order(s: &FiltersState) -> Vec<FilterKind> {
    s.chain().order().collect()
}

#[test]
fn a_menu_resets_compares_and_saves_only_its_own_options() {
    let mut s = state_with(FilterDefaults::default());
    for kind in FilterKind::DEFAULT_ORDER {
        assert!(s.options_are_default(kind), "{kind:?} starts at the defaults");
    }
    s.set_deinterlacer(Deinterlacer::Yadif);
    s.set_field_order(FieldParity::BottomFirst);
    s.set_search(|search| search.limit = 60);
    s.set_shown_edge(super::state::Edge::Top, 40);
    s.set_resize_side(0, Some(1280));
    s.set_resize_algorithm(ResizeAlgorithm::Bicubic);
    assert!(!s.options_are_default(FilterKind::Deinterlace));
    assert!(!s.options_are_default(FilterKind::Crop));
    assert!(!s.options_are_default(FilterKind::Resize));
    assert!(s.options_are_default(FilterKind::Deshake));

    // Save as default takes only the menu's options of that filter.
    let saved = s.options_as_defaults(FilterKind::Deinterlace);
    assert_eq!(
        (saved.deinterlacer, saved.field_order),
        (Deinterlacer::Yadif, FieldParity::BottomFirst)
    );
    assert_eq!(saved.crop_search.limit, 24, "the crop's search is the Crop menu's");
    assert_eq!(s.options_as_defaults(FilterKind::Crop).crop_search.limit, 60);
    assert_eq!(
        s.options_as_defaults(FilterKind::Resize).resize_algorithm,
        ResizeAlgorithm::Bicubic
    );

    // Reset: the options go back, the row's values (the edge, the size) and the on/off state stay.
    s.switch_on(FilterKind::Crop);
    s.reset_options(FilterKind::Crop);
    assert_eq!(s.search.limit, 24);
    assert_eq!(s.shown_crop().top, 40);
    assert!(s.chain().is_enabled(FilterKind::Crop));
    s.reset_options(FilterKind::Resize);
    assert_eq!(s.resize(), (Some(1280), None, ResizeAlgorithm::Lanczos));
    s.reset_options(FilterKind::Deinterlace);
    assert!(s.options_are_default(FilterKind::Deinterlace));

    // Defaults that changed (saved from a menu) count from then on.
    s.set_deinterlacer(Deinterlacer::W3fdif);
    let defaults = s.options_as_defaults(FilterKind::Deinterlace);
    s.set_defaults(defaults);
    assert!(s.options_are_default(FilterKind::Deinterlace));
}

#[test]
fn the_crop_search_is_kept_inside_its_limits() {
    let mut s = state_with(FilterDefaults::default());
    s.set_search(|search| search.samples = 0);
    assert_eq!(s.search.samples, 1);
    s.set_search(|search| search.samples = 99);
    assert_eq!(s.search.samples, 32);
    s.set_search(|search| search.sides = CropSides::TopBottom);
    let card = view::card(&s);
    assert_eq!((card.search_sides, card.search_samples), (1, 32));
}

#[test]
fn a_dragged_filter_lands_before_the_row_it_was_dropped_on() {
    let mut s = state_with(FilterDefaults::default());
    assert!(s.drop_filter(FilterKind::Resize, 0));
    assert_eq!(order(&s)[0], FilterKind::Resize);
    assert!(!s.drop_filter(FilterKind::Resize, 1), "right after itself: no change");
    assert!(s.drop_filter(FilterKind::Resize, 5));
    assert_eq!(order(&s)[4], FilterKind::Resize);
}

#[test]
fn a_rows_menu_offers_what_the_row_can_do() {
    let s = state_with(FilterDefaults::default());
    let rows = view::card(&s).rows;
    let ids = |items: Vec<MenuItem>| {
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Action(a) => Some((a.id, a.enabled, a.hold)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let first = ids(row_items(&rows[0]));
    assert_eq!(first[0].0, "toggle");
    assert!(
        first.contains(&("up".into(), false, false)),
        "the first row cannot move up"
    );
    assert!(first.contains(&("reset".into(), true, true)), "reset is held");
    let orientation = rows.iter().find(|r| r.kind == FilterKind::Orientation).unwrap();
    let entries = ids(row_items(orientation));
    assert!(
        entries.iter().all(|(id, ..)| id != "options" && id != "reset"),
        "Orientation has no menu"
    );
}

#[test]
fn every_entry_of_a_rows_menu_has_an_icon() {
    let mut s = state_with(FilterDefaults::default());
    let off = view::card(&s).rows;
    s.switch_on(FilterKind::Crop);
    let on = view::card(&s).rows;
    for row in off.iter().chain(on.iter()) {
        for item in row_items(row) {
            if let MenuItem::Action(a) = item {
                assert!(a.icon.is_some(), "{:?}: {} has no icon (Q-P9n)", row.kind, a.title);
            }
        }
    }
    let crop = on.iter().find(|r| r.kind == FilterKind::Crop).unwrap();
    let first = row_items(crop).into_iter().next();
    assert!(
        matches!(first, Some(MenuItem::Action(a)) if a.icon == Some(Icon::FilterSwitchOff)),
        "a filter that is on offers Switch Off"
    );
}

#[test]
fn typed_deshake_values_take_limits_and_hundredths() {
    let d = DeshakeParams::default();
    assert_eq!(typed_deshake(d, 5, "40 px").unwrap().max_shift, Some(40));
    assert_eq!(typed_deshake(d, 5, "").unwrap().max_shift, None, "empty = no limit");
    assert!(typed_deshake(d, 5, "501").is_none());
    assert_eq!(typed_deshake(d, 6, "3°").unwrap().max_angle, Some(3));
    assert!(typed_deshake(d, 6, "91").is_none());
    assert_eq!(typed_deshake(d, 7, "0.5").unwrap().zoom_speed, 50);
    assert!(typed_deshake(d, 7, "6").is_none());
    assert_eq!(typed_deshake(d, 3, "20").unwrap().smoothing, 20);
    assert!(typed_deshake(d, 0, "x").is_none());
}

#[test]
fn the_row_texts_follow_the_values() {
    let mut s = state_with(FilterDefaults::default());
    let card = view::card(&s);
    assert_eq!(card.deshake_summary, "shakiness 5 · smoothing 10");
    assert_eq!(card.deshake_values[5], "", "no limit");
    assert_eq!(card.deshake_values[7], "0.25");
    let mut d = s.deshake();
    d.tripod = true;
    d.optzoom = 2;
    d.zoom = -5;
    s.set_deshake(d);
    assert_eq!(
        view::card(&s).deshake_summary,
        "shakiness 5 · smoothing 10 · tripod · adaptive zoom · zoom -5%"
    );
    s.set_deinterlacer(Deinterlacer::Kerndeint);
    let card = view::card(&s);
    assert!(!card.field_options, "kerndeint has no field options");
    s.set_field_rate(true);
    assert_eq!(s.deinterlace_rate(), DeinterlaceRate::Field);
    assert_eq!(
        s.chain().get(FilterKind::Deinterlace).map(|slot| slot.enabled),
        Some(false),
        "a menu value keeps the filter off (Q-P9.13 C)"
    );
    assert!(matches!(s.deinterlace(), Filter::Deinterlace { .. }));
}
