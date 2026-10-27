#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code"
)]

use proptest::prelude::*;
use rustcut_core::edit::{Crop, FilterKind, Orientation, Size};
use rustcut_core::media::{IdetCounts, Interlacing};
use rustcut_core::settings::FilterDefaults;

use super::geometry::{CropView, Handle, MIN_KEPT};
use super::state::{CropMode, Edge, FiltersState, InterlaceHint, Kept, Source};
use super::view::{self, HintLevel};

const HD: Size = Size::new(1920, 1080);

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn the_picture_is_letterboxed_like_mpv() {
    let v = CropView::new(HD, 1.0, Orientation::IDENTITY, 1000.0, 1000.0).unwrap();
    let p = v.picture();
    assert!(near(p.x, 0.0) && near(p.width, 1000.0) && near(p.height, 562.5) && near(p.y, 218.75));
    // A quarter turn shows the frame upright: pillarboxed.
    let turned = CropView::new(HD, 1.0, Orientation::CLOCKWISE, 1000.0, 1000.0).unwrap();
    assert_eq!(turned.shown_size(), Size::new(1080, 1920));
    assert!(near(turned.picture().width, 562.5) && near(turned.picture().height, 1000.0));
    // Anamorphic PAL 16:9 (720x576, pixels 64:45) fills a 1024x576 area exactly.
    let pal = CropView::new(Size::new(720, 576), 64.0 / 45.0, Orientation::IDENTITY, 1024.0, 576.0).unwrap();
    assert!(near(pal.picture().width, 1024.0) && near(pal.picture().x, 0.0));
    assert!(CropView::new(HD, 1.0, Orientation::IDENTITY, 0.0, 10.0).is_none());
}

#[test]
fn the_kept_area_follows_the_rotation_after_the_crop() {
    // 1920x1080 shown 1:1 in a 1080x1920 area after a clockwise turn: the frame's left edge is shown at the top.
    let v = CropView::new(HD, 1.0, Orientation::CLOCKWISE, 1080.0, 1920.0).unwrap();
    let r = v.kept_rect(Crop::new(0, 0, 0, 100));
    assert!(near(r.y, 100.0) && near(r.x, 0.0) && near(r.height, 1820.0) && near(r.width, 1080.0));
    assert_eq!(v.shown_crop(Crop::new(0, 0, 0, 100)), Crop::new(100, 0, 0, 0));
}

#[test]
fn handles_are_found_near_corners_and_edges() {
    let v = CropView::new(HD, 1.0, Orientation::IDENTITY, 1920.0, 1080.0).unwrap();
    let crop = Crop::new(100, 200, 100, 200);
    assert_eq!(v.hit(crop, 203.0, 98.0), Some(Handle::TopLeft));
    assert_eq!(v.hit(crop, 1720.0, 985.0), Some(Handle::BottomRight));
    assert_eq!(v.hit(crop, 900.0, 104.0), Some(Handle::Top));
    assert_eq!(v.hit(crop, 1714.0, 500.0), Some(Handle::Right));
    assert_eq!(v.hit(crop, 900.0, 500.0), Some(Handle::Inside));
    assert_eq!(v.hit(crop, 50.0, 500.0), None);
    // The quick crop: no aiming needed, the nearest edge or corner anywhere over the picture.
    assert_eq!(v.nearest(crop, 300.0, 500.0), Some(Handle::Left));
    assert_eq!(v.nearest(crop, 900.0, 900.0), Some(Handle::Bottom));
    assert_eq!(v.nearest(crop, 1700.0, 120.0), Some(Handle::TopRight));
    assert_eq!(v.nearest(crop, 960.0, 1300.0), None);
}

#[test]
fn drags_move_edges_on_whole_pixels_and_keep_a_minimum() {
    let v = CropView::new(HD, 1.0, Orientation::IDENTITY, 960.0, 540.0).unwrap();
    // Half scale: 25 view pixels are 50 frame pixels.
    assert_eq!(
        v.drag(Crop::default(), Handle::Left, 25.0, 0.0, None),
        Crop::new(0, 0, 0, 50)
    );
    assert_eq!(
        v.drag(Crop::default(), Handle::TopRight, -10.6, 20.4, None),
        Crop::new(41, 21, 0, 0),
        "whole pixels, no 2-pixel steps (user review 2026-10-27)"
    );
    let squeezed = v.drag(Crop::default(), Handle::Left, 5000.0, 0.0, None);
    assert_eq!(squeezed.kept(HD).unwrap().width, MIN_KEPT);
    // Moving the kept area stops at the frame edges and keeps its size.
    let moved = v.drag(Crop::new(100, 100, 100, 100), Handle::Inside, -500.0, 10.0, None);
    assert_eq!(moved, Crop::new(120, 200, 80, 0));
}

#[test]
fn drags_under_a_rotation_change_the_matching_frame_edge() {
    // Clockwise after the crop: the shown top is the frame's left edge.
    let v = CropView::new(HD, 1.0, Orientation::CLOCKWISE, 1080.0, 1920.0).unwrap();
    assert_eq!(
        v.drag(Crop::default(), Handle::Top, 0.0, 64.0, None),
        Crop::new(0, 0, 0, 64)
    );
    let mirrored = CropView::new(HD, 1.0, Orientation::MIRROR_HORIZONTAL, 1920.0, 1080.0).unwrap();
    assert_eq!(
        mirrored.drag(Crop::default(), Handle::Left, 30.0, 0.0, None),
        Crop::new(0, 30, 0, 0)
    );
}

#[test]
fn a_locked_aspect_shapes_the_dragged_box() {
    let v = CropView::new(HD, 1.0, Orientation::IDENTITY, 1920.0, 1080.0).unwrap();
    let square = v.drag(Crop::default(), Handle::BottomRight, -100.0, -10.0, Some(1.0));
    let kept = square.kept(HD).unwrap();
    assert_eq!(kept.width, kept.height);
    assert_eq!(square.left, 0, "the opposite corner stays");
    // An edge drag grows the other direction around its middle.
    let wide = v.drag(Crop::new(0, 660, 0, 660), Handle::Top, 0.0, 300.0, Some(16.0 / 9.0));
    let kept = wide.kept(HD).unwrap();
    assert!(
        (f64::from(kept.width) / f64::from(kept.height) - 16.0 / 9.0).abs() < 0.02,
        "{kept:?}"
    );
    assert!(wide.left.abs_diff(wide.right) <= 2);
    // Choosing 4:3 fits the largest 4:3 box into the current kept area.
    let fitted = v.fit_aspect(Crop::default(), 4.0 / 3.0);
    assert_eq!(fitted, Crop::new(0, 240, 0, 240));
}

fn arb_orientation() -> impl Strategy<Value = Orientation> {
    prop_oneof![
        Just(Orientation::IDENTITY),
        Just(Orientation::CLOCKWISE),
        Just(Orientation::COUNTER_CLOCKWISE),
        Just(Orientation::UPSIDE_DOWN),
        Just(Orientation::MIRROR_HORIZONTAL),
        Just(Orientation::MIRROR_VERTICAL),
        Just(Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL)),
        Just(Orientation::COUNTER_CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL)),
    ]
}

fn arb_handle() -> impl Strategy<Value = Handle> {
    prop_oneof![
        Just(Handle::TopLeft),
        Just(Handle::Top),
        Just(Handle::TopRight),
        Just(Handle::Right),
        Just(Handle::BottomRight),
        Just(Handle::Bottom),
        Just(Handle::BottomLeft),
        Just(Handle::Left),
        Just(Handle::Inside),
    ]
}

proptest! {
    #[test]
    fn every_drag_gives_a_valid_crop(
        w in 40u32..4000, h in 40u32..4000, sar in 0.5f64..2.0, o in arb_orientation(),
        vw in 100.0f32..2000.0, vh in 100.0f32..2000.0, handle in arb_handle(),
        t in 0u32..2000, r in 0u32..2000, b in 0u32..2000, l in 0u32..2000,
        dx in -3000.0f32..3000.0, dy in -3000.0f32..3000.0,
        aspect in prop::option::of(0.3f64..3.0),
    ) {
        let frame = Size::new(w & !1, h & !1);
        let view = CropView::new(frame, sar, o, vw, vh).unwrap();
        let start = Crop::new(t, r, b, l).clamped(frame, 2);
        let crop = view.drag(start, handle, dx, dy, aspect);
        let kept = crop.kept(frame).expect("something stays");
        prop_assert!(kept.width >= MIN_KEPT.min(frame.width) && kept.height >= MIN_KEPT.min(frame.height));
    }

    #[test]
    fn a_dragged_edge_follows_the_pointer(w in 200u32..3000, h in 200u32..3000, o in arb_orientation(),
        vw in 200.0f32..2000.0, vh in 200.0f32..2000.0, dx in -60.0f32..60.0) {
        let frame = Size::new(w & !1, h & !1);
        let view = CropView::new(frame, 1.0, o, vw, vh).unwrap();
        let start = view.frame_crop(Crop::new(0, 0, 0, (view.shown_size().width / 4) & !1));
        let before = view.kept_rect(start);
        let after = view.kept_rect(view.drag(start, Handle::Left, dx, 0.0, None));
        // Within one pixel of the frame of where the pointer went, the picture's edge at most.
        let step = view.picture().width / view.shown_size().width as f32;
        let wanted = (before.x + dx).max(view.picture().x);
        prop_assume!(wanted + 16.0 * step < before.right());
        prop_assert!((after.x - wanted).abs() <= step + 0.01);
    }
}

fn source(size: Size, rotation: u16) -> Source {
    Source {
        size,
        sar: 1.0,
        rotation,
        duration: None,
    }
}

fn state() -> FiltersState {
    let mut s = FiltersState::new(FilterDefaults::default(), true, false, Kept::default(), None);
    s.file_opened();
    s.source_known(source(HD, 0));
    s.set_view(960.0, 540.0);
    s
}

#[test]
fn orientation_buttons_turn_in_quarters_switch_mirrors_and_switch_it_on() {
    let mut s = state();
    assert!(!s.chain().is_enabled(FilterKind::Orientation));
    s.turn(true);
    assert!(
        s.chain().is_enabled(FilterKind::Orientation),
        "a button shows its result at once"
    );
    assert_eq!(s.orientation().turns, 1);
    s.turn(false);
    assert_eq!(s.orientation().turns, 3, "left replaces right");
    s.turn(false);
    assert_eq!(s.orientation().turns, 0, "a second click takes the turn back");
    assert!(
        s.chain().is_enabled(FilterKind::Orientation),
        "it stays on (D-P9a: nothing switches itself off)"
    );
    s.flip(true);
    s.flip(false);
    assert!(s.orientation().mirror_horizontal && s.orientation().mirror_vertical);
    assert_eq!(s.orientation().orientation(), Orientation::UPSIDE_DOWN);
    s.flip(true);
    assert!(!s.orientation().mirror_horizontal && s.orientation().mirror_vertical);
    // Switched off, the values stay.
    s.toggle(FilterKind::Orientation);
    assert!(s.orientation().mirror_vertical && s.chain().active().count() == 0);
}

#[test]
fn editing_a_filter_that_is_off_keeps_it_off() {
    let mut s = state();
    s.set_shown_edge(Edge::Bottom, 90);
    s.set_resize_side(0, Some(1280));
    s.set_deinterlacer(rustcut_core::edit::Deinterlacer::Yadif);
    s.set_field_order(rustcut_core::edit::FieldParity::BottomFirst);
    s.set_only_interlaced(true);
    assert_eq!(
        s.chain().active().count(),
        0,
        "user decision Q-P9.13: the value is kept, the filter stays off"
    );
    assert_eq!(s.shown_crop().bottom, 90);
    assert_eq!(s.resize().0, Some(1280));
    s.toggle(FilterKind::Crop);
    assert_eq!(view::card(&s).output.text, "1920×990");
    s.switch_on(FilterKind::Deinterlace);
    assert_eq!(
        s.chain().get(FilterKind::Deinterlace).map(|slot| slot.filter),
        Some(rustcut_core::edit::Filter::Deinterlace {
            deinterlacer: rustcut_core::edit::Deinterlacer::Yadif,
            rate: rustcut_core::edit::DeinterlaceRate::Frame,
            field_order: rustcut_core::edit::FieldParity::BottomFirst,
            only_interlaced: true,
        })
    );
}

#[test]
fn crop_fields_are_the_edges_seen_in_the_output() {
    let mut s = state();
    s.turn(true);
    // Crop comes before the orientation by default: the shown top is the frame's left edge.
    s.set_shown_edge(Edge::Top, 120);
    assert_eq!(s.crop(), Crop::new(0, 0, 0, 120));
    assert_eq!(s.shown_crop(), Crop::new(120, 0, 0, 0));
    // Too large: limited so two pixels stay.
    s.set_shown_edge(Edge::Bottom, 5000);
    assert_eq!(s.crop().kept(HD).unwrap().width, 2);
    s.step_shown_edge(Edge::Top, -200);
    assert_eq!(s.shown_crop().top, 0);
}

#[test]
fn resize_helpers_take_the_height_from_the_width() {
    let mut s = state();
    s.resize_to_ratio(16.0 / 9.0);
    assert_eq!(s.resize().0, Some(1920));
    assert_eq!(s.resize().1, Some(1080));
    s.set_resize_side(0, Some(853));
    s.resize_to_ratio(16.0 / 9.0);
    assert_eq!((s.resize().0, s.resize().1), (Some(853), Some(480)));
    s.resize_to_ratio(4.0 / 3.0);
    assert_eq!(s.resize().1, Some(640));
    s.set_resize_side(1, Some(1));
    assert_eq!(s.resize().1, Some(2));
}

#[test]
fn the_preview_follows_the_toggle_the_source_rotation_and_crop_editing() {
    let mut s = FiltersState::new(FilterDefaults::default(), true, false, Kept::default(), None);
    s.set_crop(Crop::new(0, 0, 0, 100));
    s.switch_on(FilterKind::Crop);
    assert_eq!(s.preview_filter().lavfi, "", "nothing before the probe");
    s.source_known(source(HD, 90));
    s.set_view(960.0, 540.0);
    let p = s.preview_filter();
    assert!(
        p.rotates_source && p.lavfi.starts_with("transpose=dir=clock,crop="),
        "{p:?}"
    );
    s.preview = false;
    assert_eq!(s.preview_filter().lavfi, "");
    // Editing shows the whole frame even with the preview off (with nothing else, mpv turns it itself) ...
    assert!(s.begin_crop(CropMode::Edit));
    assert_eq!(s.preview_filter(), rustcut_player::VideoFilter::default());
    // ... and turned like the output.
    s.flip(true);
    let p = s.preview_filter();
    assert_eq!(
        (p.lavfi.as_str(), p.rotates_source),
        ("transpose=dir=clock,hflip", true)
    );
    s.end_crop(true);
    assert_eq!(s.preview_filter().lavfi, "");
}

#[test]
fn a_crop_session_drags_and_escape_restores() {
    let mut s = state();
    assert!(s.begin_crop(CropMode::Edit));
    assert!(s.chain().is_enabled(FilterKind::Crop));
    // The whole frame fills the 960x540 area: grab the left edge and pull it 50 view pixels in.
    assert!(s.crop_pointer_down(0.0, 270.0));
    assert!(s.crop_pointer_moved(50.0, 280.0, false));
    assert!(s.crop_pointer_up());
    assert_eq!(s.crop(), Crop::new(0, 0, 0, 100));
    s.end_crop(false);
    assert_eq!(s.crop(), Crop::default());
    assert!(!s.chain().is_enabled(FilterKind::Crop));
    // Quick mode grabs the nearest edge from anywhere over the picture.
    assert!(s.begin_crop(CropMode::Quick));
    assert!(s.crop_pointer_down(470.0, 30.0));
    s.crop_pointer_moved(470.0, 60.0, false);
    s.crop_pointer_up();
    s.end_crop(true);
    assert_eq!(s.crop(), Crop::new(60, 0, 0, 0));
}

#[test]
fn keep_carries_the_chain_to_the_next_video_and_start() {
    let mut s = state();
    s.turn(true);
    s.search.limit = 60;
    s.file_opened();
    assert_eq!(s.orientation().turns, 0, "without keep the next video starts clean");
    assert_eq!(s.search.limit, 24);
    s.keep = true;
    s.turn(false);
    s.search.limit = 60;
    s.file_opened();
    assert_eq!(s.orientation().turns, 3);
    let kept = s.kept();
    let restarted = FiltersState::new(FilterDefaults::default(), true, true, kept.clone(), None);
    assert_eq!((restarted.orientation().turns, restarted.search.limit), (3, 60));
    let off = FiltersState::new(FilterDefaults::default(), true, false, kept, None);
    assert_eq!((off.orientation().turns, off.search.limit), (0, 24));
}

#[test]
fn the_card_tells_the_output_size_and_the_interlace_check() {
    let mut s = state();
    let plain = view::card(&s).output;
    assert_eq!(plain.text, "1920×1080");
    assert_eq!(plain.tooltip, "The final output size after every filter.");
    assert!(
        !plain.adjusted && plain.rows.is_empty(),
        "nothing cut: no rows, no jump"
    );
    s.set_crop(Crop::new(1, 0, 0, 3));
    s.switch_on(FilterKind::Crop);
    let card = view::card(&s);
    // Q-P9s: the header shows the final size; its tooltip names both only when the rule cut something.
    assert_eq!(card.output.text, "1916×1078");
    assert!(card.output.adjusted);
    assert_eq!(
        card.output.rows,
        vec![
            ("After the filters".to_owned(), "1917×1079".to_owned()),
            ("Divisible by 2".to_owned(), "1916×1078".to_owned())
        ]
    );
    // The rule off: the odd size stays, nothing to explain or jump to.
    s.set_defaults(FilterDefaults {
        final_multiple: 0,
        ..FilterDefaults::default()
    });
    let card = view::card(&s);
    assert_eq!((card.output.text.as_str(), card.output.adjusted), ("1917×1079", false));
    s.set_defaults(FilterDefaults {
        final_multiple: 16,
        ..FilterDefaults::default()
    });
    assert_eq!(view::card(&s).output.text, "1904×1072");
    assert_eq!(card.crop_size, "1917×1079");
    s.interlace = InterlaceHint::Known(
        Interlacing::Interlaced { top_field_first: true },
        IdetCounts {
            tff: 250,
            ..IdetCounts::default()
        },
    );
    let card = view::card(&s);
    assert_eq!(card.interlace_hint, "interlaced");
    assert_eq!(card.interlace_counts, "250 interlaced · 0 progressive");
    assert_eq!(card.interlace_level, HintLevel::Attention);
    assert!(card.interlace_detail.contains("top field first"));
    assert_eq!(
        card.interlace_rows.first(),
        Some(&("Top field first".to_owned(), "250 frames".to_owned()))
    );
    assert_eq!(card.rows.len(), 5);
    assert!(!card.rows[0].can_up && card.rows[0].can_down);
    assert_eq!(
        card.rows.iter().map(view::RowView::lines).sum::<i32>(),
        6,
        "Crop has two lines"
    );
    assert!(
        card.rows
            .iter()
            .all(|r| r.has_menu() == (r.kind != FilterKind::Orientation))
    );
    // No "auto": the dim placeholder is the number an empty side comes to (13#3).
    assert_eq!(view::card(&s).resize_auto_height, "1079");
}
