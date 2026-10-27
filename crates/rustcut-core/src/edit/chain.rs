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

    /// The size rounded down to multiples of `n` (the final size rule, Q-P9s); `n` below 2 (the rule off) keeps it.
    /// A side shorter than `n` stays as it is, made even.
    pub const fn floor_to(self, n: u32) -> Self {
        const fn side(v: u32, n: u32) -> u32 {
            if n < 2 {
                v
            } else if v >= n {
                v / n * n
            } else if v >= 2 {
                v & !1
            } else {
                v
            }
        }
        Self {
            width: side(self.width, n),
            height: side(self.height, n),
        }
    }
}

/// The values of the final size rule (Settings > Filters > Final size, Q-P9s): 0 = off (the size stays as the filters
/// make it), else every render's width and height are cut down to multiples of it (2 = even, the default).
pub const FINAL_MULTIPLES: [u8; 5] = [0, 2, 4, 8, 16];

/// The final size rule's default: even sizes (the only rule every 4:2:0 encoder needs, Q-P9s research).
pub const fn even_multiple() -> u8 {
    2
}

/// `n` as one of [`FINAL_MULTIPLES`]: 0 stays off, anything else the largest multiple that does not exceed it (2 for 1).
pub fn final_multiple(n: u8) -> u8 {
    if n == 0 {
        return 0;
    }
    FINAL_MULTIPLES
        .iter()
        .rev()
        .copied()
        .find(|m| *m != 0 && *m <= n)
        .unwrap_or(2)
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

    /// Moves a filter before the filter at `before` (an index of the order before the move; the count = the end), as a
    /// drop of drag & drop does; `false` when the order stays.
    pub fn move_to(&mut self, kind: FilterKind, before: usize) -> bool {
        let Some(from) = self.position(kind) else {
            return false;
        };
        let to = if before > from { before - 1 } else { before }.min(self.slots.len().saturating_sub(1));
        if to == from {
            return false;
        }
        let slot = self.slots.remove(from);
        self.slots.insert(to, slot);
        true
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

    /// The size the render writes: [`Self::output_size`] cut down by the final size rule (`multiple`, see
    /// [`FINAL_MULTIPLES`]).
    pub fn final_size(&self, source: Size, multiple: u8) -> Result<Size, ChainError> {
        Ok(self.output_size(source)?.floor_to(u32::from(final_multiple(multiple))))
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
mod tests;
