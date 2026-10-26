use proptest::prelude::*;

use super::*;
use crate::edit::{Crop, ResizeAlgorithm};

fn chain_with(filters: &[Filter]) -> FilterChain {
    let mut chain = FilterChain::default();
    for &f in filters {
        chain.set(f);
        chain.set_enabled(f.kind(), true);
    }
    chain
}

#[test]
fn default_chain_has_every_kind_disabled() {
    let chain = FilterChain::default();
    assert_eq!(chain.order().collect::<Vec<_>>(), FilterKind::DEFAULT_ORDER.to_vec());
    assert_eq!(chain.active().count(), 0);
    assert!(!chain.needs_two_passes());
}

#[test]
fn output_size_follows_crop_rotate_resize() {
    let chain = chain_with(&[
        Filter::Crop(Crop::new(10, 20, 10, 20)),
        Filter::Orientation(Orientation::CLOCKWISE.into()),
        Filter::Resize {
            width: None,
            height: Some(720),
            algorithm: ResizeAlgorithm::Lanczos,
        },
    ]);
    // 1920x1080 -> crop 1880x1060 -> rotate 1060x1880 -> height 720 keeps the aspect: 406x720 (405.96 rounded to even)
    assert_eq!(chain.output_size(Size::new(1920, 1080)).unwrap(), Size::new(406, 720));
}

#[test]
fn a_crop_larger_than_the_frame_is_an_error() {
    let chain = chain_with(&[Filter::Crop(Crop::new(0, 100, 0, 100))]);
    assert!(matches!(
        chain.output_size(Size::new(200, 100)),
        Err(ChainError::CropTooLarge { width: 0, .. })
    ));
}

#[test]
fn disabled_filters_do_not_count() {
    let mut chain = chain_with(&[Filter::Orientation(Orientation::CLOCKWISE.into())]);
    chain.set_enabled(FilterKind::Orientation, false);
    assert_eq!(chain.output_size(Size::new(640, 480)).unwrap(), Size::new(640, 480));
}

#[test]
fn reading_repairs_the_invariant() {
    let json =
        r#"[{"enabled":true,"filter":{"kind":"resize","width":640}},{"enabled":false,"filter":{"kind":"resize"}}]"#;
    let chain: FilterChain = serde_json::from_str(json).unwrap();
    assert_eq!(chain.slots().len(), FilterKind::DEFAULT_ORDER.len());
    assert_eq!(chain.order().next(), Some(FilterKind::Resize));
    assert!(chain.is_enabled(FilterKind::Resize));
}

#[test]
fn reorder_moves_one_step() {
    let mut chain = FilterChain::default();
    assert!(!chain.move_up(FilterKind::Deinterlace));
    assert!(chain.move_up(FilterKind::Crop));
    assert_eq!(
        chain.order().take(2).collect::<Vec<_>>(),
        vec![FilterKind::Crop, FilterKind::Deinterlace]
    );
    assert!(!chain.move_down(FilterKind::Resize));
}

#[test]
fn a_drop_moves_a_filter_before_another() {
    use FilterKind::{Crop, Deinterlace, Deshake, Orientation, Resize};
    let mut chain = FilterChain::default();
    // Deinterlace, Crop, Deshake, Orientation, Resize: Resize before Crop.
    assert!(chain.move_to(Resize, 1));
    assert_eq!(
        chain.order().collect::<Vec<_>>(),
        vec![Deinterlace, Resize, Crop, Deshake, Orientation]
    );
    // Deinterlace to the end (the count), then before itself and right after itself: no change.
    assert!(chain.move_to(Deinterlace, 5));
    assert_eq!(
        chain.order().collect::<Vec<_>>(),
        vec![Resize, Crop, Deshake, Orientation, Deinterlace]
    );
    assert!(!chain.move_to(Crop, 1));
    assert!(!chain.move_to(Crop, 2));
    // Past the end counts as the end.
    assert!(chain.move_to(Resize, 99));
    assert_eq!(chain.order().last(), Some(Resize));
}

#[test]
fn what_reaches_the_crop_and_how_the_output_shows_it() {
    let mut chain = chain_with(&[
        Filter::Resize {
            width: Some(960),
            height: None,
            algorithm: ResizeAlgorithm::Bicubic,
        },
        Filter::Orientation(Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL).into()),
    ]);
    // Default order: deinterlace, crop, deshake, orientation, resize.
    assert_eq!(
        chain.size_before(FilterKind::Crop, Size::new(1920, 1080)).unwrap(),
        Size::new(1920, 1080)
    );
    assert_eq!(
        chain.orientation_after(FilterKind::Crop),
        Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL)
    );
    assert!((chain.sample_aspect_before(FilterKind::Crop, 1.5) - 1.5).abs() < 1e-12);
    // Resize and orientation moved in front of the crop: the crop sees a turned, smaller, square-pixel frame.
    while chain.move_up(FilterKind::Orientation) {}
    while chain.move_up(FilterKind::Resize) {}
    assert_eq!(
        chain.order().take(3).collect::<Vec<_>>(),
        vec![FilterKind::Resize, FilterKind::Orientation, FilterKind::Deinterlace]
    );
    assert_eq!(
        chain.size_before(FilterKind::Crop, Size::new(1920, 1080)).unwrap(),
        Size::new(540, 960)
    );
    assert!((chain.sample_aspect_before(FilterKind::Crop, 1.5) - 1.0).abs() < 1e-12);
    assert_eq!(chain.orientation_after(FilterKind::Crop), Orientation::IDENTITY);
}

#[test]
fn crop_editing_shows_the_whole_frame_turned_like_the_output() {
    let chain = chain_with(&[
        Filter::Crop(Crop::new(10, 0, 10, 0)),
        Filter::Orientation(Orientation::COUNTER_CLOCKWISE.into()),
        Filter::Resize {
            width: Some(640),
            height: Some(640),
            algorithm: ResizeAlgorithm::Bicubic,
        },
    ]);
    let editing = chain.for_crop_editing();
    assert_eq!(
        editing.active().map(Filter::kind).collect::<Vec<_>>(),
        vec![FilterKind::Orientation]
    );
    // The crop keeps its values (only switched off).
    assert_eq!(
        editing.get(FilterKind::Crop).map(|s| s.filter),
        Some(Filter::Crop(Crop::new(10, 0, 10, 0)))
    );
}

/// A chain as 0.0.47 wrote it (every kind once, Rotate and Mirror in their own slots).
fn old_chain(slots: &[(bool, &str)]) -> FilterChain {
    let body = slots
        .iter()
        .map(|(on, filter)| format!(r#"{{"enabled":{on},"filter":{filter}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    serde_json::from_str(&format!("[{body}]")).unwrap()
}

const DEINTERLACE: &str = r#"{"kind":"deinterlace","deinterlacer":"bwdif","rate":"frame"}"#;
const DESHAKE: &str = r#"{"kind":"deshake","shakiness":5,"accuracy":10,"stepsize":12,"smoothing":10,"zoom":0,"optzoom":0,"crop":"black","interpolation":"bicubic","tripod":false}"#;
const RESIZE: &str = r#"{"kind":"resize","width":1280,"height":null,"algorithm":"bicubic"}"#;
const ROTATE_CW: &str = r#"{"kind":"rotate","rotation":"clockwise"}"#;
const MIRROR_H: &str = r#"{"kind":"mirror","horizontal":true,"vertical":false}"#;
const MIRROR_V: &str = r#"{"kind":"mirror","horizontal":false,"vertical":true}"#;

fn crop_json(c: Crop) -> String {
    format!(
        r#"{{"kind":"crop","top":{},"right":{},"bottom":{},"left":{}}}"#,
        c.top, c.right, c.bottom, c.left
    )
}

#[test]
fn an_old_rotate_and_mirror_become_one_orientation() {
    let crop = crop_json(Crop::new(8, 0, 8, 0));
    // Both on, side by side (the default order): one Orientation at the Rotate's place.
    let chain = old_chain(&[
        (false, DEINTERLACE),
        (true, &crop),
        (false, DESHAKE),
        (true, ROTATE_CW),
        (true, MIRROR_H),
        (true, RESIZE),
    ]);
    assert_eq!(chain.order().collect::<Vec<_>>(), FilterKind::DEFAULT_ORDER.to_vec());
    assert_eq!(
        chain.get(FilterKind::Orientation),
        Some(&FilterSlot {
            enabled: true,
            filter: Filter::Orientation(Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL).into()),
        })
    );
    assert_eq!(chain.output_size(Size::new(1920, 1080)).unwrap(), Size::new(1280, 2310));
    // Only the mirror on: it decides the place and the value; a Rotate that was off counts as upright.
    let chain = old_chain(&[
        (false, ROTATE_CW),
        (true, DEINTERLACE),
        (true, MIRROR_V),
        (false, &crop),
        (false, DESHAKE),
        (false, RESIZE),
    ]);
    assert_eq!(
        chain.order().take(2).collect::<Vec<_>>(),
        vec![FilterKind::Deinterlace, FilterKind::Orientation]
    );
    assert_eq!(
        chain.get(FilterKind::Orientation).map(|s| (s.enabled, s.filter)),
        Some((
            true,
            Filter::Orientation(OrientationParams {
                turns: 0,
                mirror_horizontal: false,
                mirror_vertical: true,
            })
        ))
    );
    // Both off: an upright Orientation that is off, at the Rotate's place.
    let chain = old_chain(&[(false, DEINTERLACE), (false, ROTATE_CW), (false, MIRROR_H)]);
    assert_eq!(
        chain.slots().get(1),
        Some(&FilterSlot {
            enabled: false,
            filter: Filter::Orientation(OrientationParams::UPRIGHT),
        })
    );
    assert_eq!(chain.slots().len(), FilterKind::DEFAULT_ORDER.len());
}

#[test]
fn filters_between_an_old_rotate_and_mirror_keep_their_pixels() {
    let crop = Crop::new(1, 2, 3, 4);
    let source = Size::new(1920, 1080);
    // Rotate, then crop and resize, then mirror top to bottom (a reordered chain of 0.0.47).
    let chain = old_chain(&[
        (true, ROTATE_CW),
        (true, &crop_json(crop)),
        (
            true,
            r#"{"kind":"resize","width":600,"height":null,"algorithm":"lanczos"}"#,
        ),
        (true, MIRROR_V),
    ]);
    assert_eq!(
        chain.active().copied().collect::<Vec<_>>(),
        vec![
            // The old values as set: a quarter right, mirrored top to bottom.
            Filter::Orientation(OrientationParams {
                turns: 1,
                mirror_horizontal: false,
                mirror_vertical: true,
            }),
            // The crop's top and bottom trade places, the resize stays (the mirror keeps the sides).
            Filter::Crop(Crop::new(3, 2, 1, 4)),
            Filter::Resize {
                width: Some(600),
                height: None,
                algorithm: ResizeAlgorithm::Lanczos,
            },
        ]
    );
    // What 0.0.47 rendered: 1080x1920 turned, 1074x1916 cropped, 600 wide (1070 high, even), mirrored.
    assert_eq!(chain.output_size(source).unwrap(), Size::new(600, 1070));
    // A turn as the second one moves a resize's sides.
    let chain = old_chain(&[
        (true, MIRROR_H),
        (
            true,
            r#"{"kind":"resize","width":600,"height":null,"algorithm":"lanczos"}"#,
        ),
        (true, ROTATE_CW),
    ]);
    assert_eq!(chain.output_size(source).unwrap(), Size::new(338, 600));
    assert_eq!(
        chain.get(FilterKind::Resize).map(|s| s.filter),
        Some(Filter::Resize {
            width: None,
            height: Some(600),
            algorithm: ResizeAlgorithm::Lanczos,
        })
    );
}

#[test]
fn a_new_chain_round_trips() {
    let chain = chain_with(&[
        Filter::Orientation(OrientationParams {
            turns: 3,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
        Filter::Crop(Crop::new(2, 0, 2, 0)),
    ]);
    let json = serde_json::to_string(&chain).unwrap();
    assert!(
        json.contains(r#"{"kind":"orientation","turns":3,"mirror_horizontal":true,"mirror_vertical":false}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<FilterChain>(&json).unwrap(), chain);
}

proptest! {
    #[test]
    fn resize_with_one_side_gives_an_even_other_side(w in 2u32..8000, h in 2u32..8000, target in 2u32..8000) {
        let out = resized(Size::new(w, h), Some(target), None);
        prop_assert_eq!(out.width, target);
        prop_assert!(out.height.is_multiple_of(2));
    }
}
