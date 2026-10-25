//! The ordered filter chain of a job: every filter kind exactly once, each enabled or not, in a user-defined order.

use serde::{Deserialize, Serialize};

use super::filter::{Mirror, Rotation};
use super::{Filter, FilterKind, Orientation, OrientationParams};

/// Width and height of a video frame in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_even(self) -> bool {
        self.width.is_multiple_of(2) && self.height.is_multiple_of(2)
    }

    /// The size rounded down to even numbers (yuv420 encoders need them).
    pub const fn even_floor(self) -> Self {
        Self {
            width: self.width & !1,
            height: self.height & !1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChainError {
    #[error("the crop removes the whole picture ({width}x{height} left of the {source_width}x{source_height} frame)")]
    CropTooLarge {
        width: i64,
        height: i64,
        source_width: u32,
        source_height: u32,
    },
    #[error("the output would be smaller than 2x2 pixels ({0}x{1})")]
    TooSmall(u32, u32),
    #[error("deshake is enabled but the render pass has no transform file (two passes are needed)")]
    DeshakeNeedsPass,
    #[error("the path of the deshake transform file is not valid UTF-8")]
    NonUtf8Path,
}

/// One filter of the chain with its on/off state. A disabled filter keeps its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FilterSlot {
    pub enabled: bool,
    pub filter: Filter,
}

/// The filter chain of a job. Invariant: each [`FilterKind`] appears exactly once; reading an old or hand-edited list
/// repairs it (duplicates dropped, missing kinds appended disabled, the Rotate and Mirror of 0.0.47 and older merged into
/// one Orientation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<StoredSlot>", into = "Vec<FilterSlot>")]
pub struct FilterChain {
    slots: Vec<FilterSlot>,
}

impl Default for FilterChain {
    fn default() -> Self {
        Self {
            slots: FilterKind::DEFAULT_ORDER
                .iter()
                .map(|&kind| FilterSlot {
                    enabled: false,
                    filter: Filter::default_for(kind),
                })
                .collect(),
        }
    }
}

impl FilterChain {
    pub fn slots(&self) -> &[FilterSlot] {
        &self.slots
    }

    pub fn order(&self) -> impl ExactSizeIterator<Item = FilterKind> + '_ {
        self.slots.iter().map(|s| s.filter.kind())
    }

    pub fn get(&self, kind: FilterKind) -> Option<&FilterSlot> {
        self.slots.iter().find(|s| s.filter.kind() == kind)
    }

    pub fn is_enabled(&self, kind: FilterKind) -> bool {
        self.get(kind).is_some_and(|s| s.enabled)
    }

    /// Replaces the parameters of the filter's kind; position and on/off state stay.
    pub fn set(&mut self, filter: Filter) {
        if let Some(slot) = self.slot_mut(filter.kind()) {
            slot.filter = filter;
        }
    }

    pub fn set_enabled(&mut self, kind: FilterKind, enabled: bool) {
        if let Some(slot) = self.slot_mut(kind) {
            slot.enabled = enabled;
        }
    }

    /// Moves a filter one place earlier; `false` when it is first already.
    pub fn move_up(&mut self, kind: FilterKind) -> bool {
        match self.position(kind) {
            Some(i) if i > 0 => {
                self.slots.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves a filter one place later; `false` when it is last already.
    pub fn move_down(&mut self, kind: FilterKind) -> bool {
        match self.position(kind) {
            Some(i) if i + 1 < self.slots.len() => {
                self.slots.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// The enabled filters that change something, in chain order.
    pub fn active(&self) -> impl Iterator<Item = &Filter> + '_ {
        self.slots
            .iter()
            .filter(|s| s.enabled && !s.filter.is_identity())
            .map(|s| &s.filter)
    }

    /// Whether the render needs two passes (deshake enabled).
    pub fn needs_two_passes(&self) -> bool {
        self.active().any(|f| f.kind() == FilterKind::Deshake)
    }

    /// The frame size after the chain for a source of `source` pixels (as ffmpeg computes it, before the even-size guard).
    pub fn output_size(&self, source: Size) -> Result<Size, ChainError> {
        let mut size = source;
        for filter in self.active() {
            size = match *filter {
                Filter::Crop(c) => {
                    let width = i64::from(size.width) - i64::from(c.left) - i64::from(c.right);
                    let height = i64::from(size.height) - i64::from(c.top) - i64::from(c.bottom);
                    match (u32::try_from(width), u32::try_from(height)) {
                        (Ok(w), Ok(h)) if w > 0 && h > 0 => Size::new(w, h),
                        _ => {
                            return Err(ChainError::CropTooLarge {
                                width,
                                height,
                                source_width: size.width,
                                source_height: size.height,
                            });
                        }
                    }
                }
                Filter::Orientation(p) => p.orientation().size(size),
                Filter::Resize { width, height, .. } => resized(size, width, height),
                Filter::Deinterlace { .. } | Filter::Deshake(_) => size,
            };
        }
        if size.width < 2 || size.height < 2 {
            return Err(ChainError::TooSmall(size.width, size.height));
        }
        Ok(size)
    }

    /// The chain with every filter from `kind` on switched off: what reaches `kind`.
    pub fn before(&self, kind: FilterKind) -> Self {
        let cut = self.position(kind).unwrap_or(self.slots.len());
        let mut chain = self.clone();
        for slot in chain.slots.iter_mut().skip(cut) {
            slot.enabled = false;
        }
        chain
    }

    /// The size of the frame that reaches `kind` from a `source`-sized video.
    pub fn size_before(&self, kind: FilterKind, source: Size) -> Result<Size, ChainError> {
        self.before(kind).output_size(source)
    }

    /// The sample aspect ratio (pixel width / height) of the frame that reaches `kind` from a source with `source_sar`:
    /// quarter turns invert it, a resize makes the pixels square (`setsar=1`).
    pub fn sample_aspect_before(&self, kind: FilterKind, source_sar: f64) -> f64 {
        self.before(kind)
            .active()
            .fold(source_sar, |sar, filter| match *filter {
                Filter::Orientation(p) => p.orientation().sample_aspect(sar),
                Filter::Resize { .. } => 1.0,
                _ => sar,
            })
    }

    /// The enabled orientation after `kind` (how the output shows `kind`'s frame).
    pub fn orientation_after(&self, kind: FilterKind) -> Orientation {
        let start = self.position(kind).map_or(self.slots.len(), |i| i + 1);
        self.slots
            .iter()
            .skip(start)
            .filter(|s| s.enabled && !s.filter.is_identity())
            .fold(Orientation::IDENTITY, |o, slot| match slot.filter {
                Filter::Orientation(p) => o.then(p.orientation()),
                _ => o,
            })
    }

    /// What the preview shows while the crop is edited on the video: the whole frame that reaches the crop, turned and
    /// mirrored like the output. The crop is off; after it only rotations, mirrors and deinterlacing stay (a resize would
    /// distort the uncropped frame).
    pub fn for_crop_editing(&self) -> Self {
        let crop_at = self.position(FilterKind::Crop).unwrap_or(0);
        let mut chain = self.clone();
        for (i, slot) in chain.slots.iter_mut().enumerate() {
            let kind = slot.filter.kind();
            let dropped =
                kind == FilterKind::Crop || (i > crop_at && matches!(kind, FilterKind::Resize | FilterKind::Deshake));
            if dropped {
                slot.enabled = false;
            }
        }
        chain
    }

    fn position(&self, kind: FilterKind) -> Option<usize> {
        self.slots.iter().position(|s| s.filter.kind() == kind)
    }

    fn slot_mut(&mut self, kind: FilterKind) -> Option<&mut FilterSlot> {
        self.slots.iter_mut().find(|s| s.filter.kind() == kind)
    }
}

/// ffmpeg's `scale=w:-2` arithmetic: the missing side keeps the aspect ratio, rounded to the nearest multiple of 2.
fn resized(size: Size, width: Option<u32>, height: Option<u32>) -> Size {
    let rescale_even = |value: u32, num: u32, den: u32| -> u32 {
        let den = u64::from(den.max(1)) * 2;
        let scaled = (u64::from(value) * u64::from(num) + den / 2) / den * 2;
        u32::try_from(scaled).unwrap_or(u32::MAX)
    };
    match (width, height) {
        (Some(w), Some(h)) => Size::new(w, h),
        (Some(w), None) => Size::new(w, rescale_even(w, size.height, size.width)),
        (None, Some(h)) => Size::new(rescale_even(h, size.width, size.height), h),
        (None, None) => size,
    }
}

/// A slot as stored: a current filter or the Rotate or Mirror of 0.0.47 and older.
#[derive(Debug, Clone, Copy, Deserialize)]
struct StoredSlot {
    enabled: bool,
    filter: StoredFilter,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
enum StoredFilter {
    Current(Filter),
    Legacy(LegacyFilter),
}

/// The two filters Orientation replaced (0.0.48).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LegacyFilter {
    Rotate { rotation: Rotation },
    Mirror(Mirror),
}

impl LegacyFilter {
    fn params(self) -> OrientationParams {
        match self {
            Self::Rotate { rotation } => Orientation::from_rotation(rotation).into(),
            Self::Mirror(mirror) => OrientationParams {
                turns: 0,
                mirror_horizontal: mirror.horizontal,
                mirror_vertical: mirror.vertical,
            },
        }
    }
}

impl From<Vec<FilterSlot>> for FilterChain {
    fn from(slots: Vec<FilterSlot>) -> Self {
        slots
            .into_iter()
            .map(|slot| StoredSlot {
                enabled: slot.enabled,
                filter: StoredFilter::Current(slot.filter),
            })
            .collect::<Vec<_>>()
            .into()
    }
}

impl From<Vec<StoredSlot>> for FilterChain {
    fn from(stored: Vec<StoredSlot>) -> Self {
        let mut repaired: Vec<FilterSlot> = Vec::with_capacity(FilterKind::DEFAULT_ORDER.len());
        // Where the Orientation made from an old Rotate or Mirror stands.
        let mut legacy_at: Option<usize> = None;
        for slot in stored {
            match slot.filter {
                StoredFilter::Current(filter) => {
                    if !repaired.iter().any(|s| s.filter.kind() == filter.kind()) {
                        repaired.push(FilterSlot {
                            enabled: slot.enabled,
                            filter,
                        });
                    }
                }
                StoredFilter::Legacy(old) => merge_legacy(&mut repaired, &mut legacy_at, slot.enabled, old.params()),
            }
        }
        for kind in FilterKind::DEFAULT_ORDER {
            if !repaired.iter().any(|s| s.filter.kind() == kind) {
                repaired.push(FilterSlot {
                    enabled: false,
                    filter: Filter::default_for(kind),
                });
            }
        }
        Self { slots: repaired }
    }
}

/// Adds an old Rotate or Mirror (`turn`, on or off) to the chain being repaired. The first one becomes the Orientation at
/// its place (an old filter that was off was upright: its value only meant something while on). When both were on, the
/// second joins the first, and the filters between them are moved behind it ([`Filter::seen_through`]), so the chain
/// renders what it rendered before.
fn merge_legacy(repaired: &mut Vec<FilterSlot>, legacy_at: &mut Option<usize>, enabled: bool, turn: OrientationParams) {
    let Some(at) = *legacy_at else {
        if repaired.iter().all(|s| s.filter.kind() != FilterKind::Orientation) {
            *legacy_at = Some(repaired.len());
            repaired.push(FilterSlot {
                enabled,
                filter: Filter::Orientation(if enabled { turn } else { OrientationParams::UPRIGHT }),
            });
        }
        return;
    };
    let Some(&FilterSlot {
        enabled: first_on,
        filter: Filter::Orientation(first),
    }) = repaired.get(at)
    else {
        return;
    };
    if !enabled {
        return;
    }
    if first_on {
        for slot in repaired.iter_mut().skip(at + 1) {
            slot.filter = slot.filter.seen_through(turn.orientation());
        }
        if let Some(slot) = repaired.get_mut(at) {
            slot.filter = Filter::Orientation(first.then(turn));
        }
    } else {
        // The first was off: the one that was on decides the place.
        repaired.remove(at);
        *legacy_at = Some(repaired.len());
        repaired.push(FilterSlot {
            enabled: true,
            filter: Filter::Orientation(turn),
        });
    }
}

impl From<FilterChain> for Vec<FilterSlot> {
    fn from(chain: FilterChain) -> Self {
        chain.slots
    }
}

#[cfg(test)]
mod tests {
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
}
