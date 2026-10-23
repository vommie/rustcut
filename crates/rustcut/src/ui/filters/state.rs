//! The filters of the open video (pure, tested): the chain with every operation of the Filters card, what mpv's preview
//! gets, and the crop session on the video (the edit mode with handles, or the quick mode while Ctrl is held).

use rustcut_core::edit::{
    Crop, CropAspect, DeinterlaceRate, Deinterlacer, DeshakeParams, Filter, FilterChain, FilterKind, Mirror,
    ResizeAlgorithm, Rotation, Size,
};
use rustcut_core::media::{IdetCounts, Interlacing};
use rustcut_core::settings::FilterDefaults;
use rustcut_core::time::Timestamp;
use rustcut_player::VideoFilter;

use super::geometry::CropView;
pub use super::session::{CropMode, CropSession};

/// Largest width or height a resize accepts.
pub const MAX_RESIZE: u32 = 16_384;

/// What the probe found about the open video's picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    /// Width and height as shown (after the display rotation, as ffmpeg's filters get them).
    pub size: Size,
    /// Pixel aspect (width / height) as shown.
    pub sar: f64,
    /// The display rotation in degrees clockwise (mpv's).
    pub rotation: u16,
    pub duration: Option<Timestamp>,
}

/// An edge of the crop as seen in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    pub const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Top),
            1 => Some(Self::Right),
            2 => Some(Self::Bottom),
            3 => Some(Self::Left),
            _ => None,
        }
    }

    const fn get(self, c: Crop) -> u32 {
        match self {
            Self::Top => c.top,
            Self::Right => c.right,
            Self::Bottom => c.bottom,
            Self::Left => c.left,
        }
    }

    const fn set(self, c: Crop, value: u32) -> Crop {
        let mut c = c;
        match self {
            Self::Top => c.top = value,
            Self::Right => c.right = value,
            Self::Bottom => c.bottom = value,
            Self::Left => c.left = value,
        }
        c
    }
}

/// What the interlace check knows about the open video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterlaceHint {
    /// No video, or the probe is not done yet.
    None,
    Checking,
    Known(Interlacing, IdetCounts),
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FiltersState {
    pub(super) chain: FilterChain,
    defaults: FilterDefaults,
    source: Option<Source>,
    pub preview: bool,
    pub keep: bool,
    pub aspect: CropAspect,
    pub interlace: InterlaceHint,
    pub auto_crop_running: bool,
    pub(super) crop: Option<CropSession>,
    /// The video area in logical pixels.
    view: (f32, f32),
}

/// A fresh chain with the configured defaults (deinterlacer, deshake parameters), every filter off.
pub fn default_chain(defaults: &FilterDefaults) -> FilterChain {
    let mut chain = FilterChain::default();
    chain.set(Filter::Deinterlace {
        deinterlacer: defaults.deinterlacer,
        rate: DeinterlaceRate::Frame,
    });
    chain.set(Filter::Deshake(defaults.deshake.sanitized()));
    chain
}

impl FiltersState {
    /// The state at startup: the kept chain when "keep" was on, else the defaults.
    pub fn new(
        defaults: FilterDefaults,
        preview: bool,
        keep: bool,
        kept: Option<FilterChain>,
        aspect: CropAspect,
    ) -> Self {
        let chain = kept.filter(|_| keep).unwrap_or_else(|| default_chain(&defaults));
        Self {
            chain,
            defaults,
            source: None,
            preview,
            keep,
            aspect,
            interlace: InterlaceHint::None,
            auto_crop_running: false,
            crop: None,
            view: (0.0, 0.0),
        }
    }

    pub const fn chain(&self) -> &FilterChain {
        &self.chain
    }

    pub const fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub const fn crop_session(&self) -> Option<&CropSession> {
        self.crop.as_ref()
    }

    /// The chain to keep for the next start (only while "keep" is on).
    pub fn kept_chain(&self) -> Option<FilterChain> {
        self.keep.then(|| self.chain.clone())
    }

    /// Another video is opening: a crop session ends (kept), the picture checks start over, and the filters go back to
    /// the defaults unless "keep" is on.
    pub fn file_opened(&mut self) {
        self.crop = None;
        self.source = None;
        self.interlace = InterlaceHint::Checking;
        self.auto_crop_running = false;
        if !self.keep {
            self.chain = default_chain(&self.defaults);
        }
    }

    pub const fn source_known(&mut self, source: Source) {
        self.source = Some(source);
    }

    /// The video area changed size.
    pub const fn set_view(&mut self, width: f32, height: f32) {
        self.view = (width, height);
    }

    // --- The card ----------------------------------------------------------------------------------------------------

    /// Switches a filter on or off (a mirror without an axis gets the horizontal one).
    pub fn toggle(&mut self, kind: FilterKind) {
        let on = !self.chain.is_enabled(kind);
        if on
            && kind == FilterKind::Mirror
            && let Some(Filter::Mirror(m)) = self.filter(kind)
            && !m.horizontal
            && !m.vertical
        {
            self.chain.set(Filter::Mirror(Mirror {
                horizontal: true,
                vertical: false,
            }));
        }
        self.chain.set_enabled(kind, on);
        if kind == FilterKind::Crop && !on {
            self.crop = None;
        }
    }

    pub fn move_filter(&mut self, kind: FilterKind, up: bool) -> bool {
        if up {
            self.chain.move_up(kind)
        } else {
            self.chain.move_down(kind)
        }
    }

    /// Every filter back to the defaults and off, in the default order.
    pub fn reset(&mut self) {
        self.chain = default_chain(&self.defaults);
        self.crop = None;
    }

    pub fn filter(&self, kind: FilterKind) -> Option<Filter> {
        self.chain.get(kind).map(|slot| slot.filter)
    }

    /// Sets a filter's parameters and switches it on.
    fn set_on(&mut self, filter: Filter) {
        self.chain.set(filter);
        self.chain.set_enabled(filter.kind(), true);
    }

    pub fn set_deinterlacer(&mut self, deinterlacer: Deinterlacer) {
        let rate = self.deinterlace_rate();
        self.set_on(Filter::Deinterlace { deinterlacer, rate });
    }

    pub fn set_field_rate(&mut self, field: bool) {
        let deinterlacer = self.deinterlacer();
        let rate = if field {
            DeinterlaceRate::Field
        } else {
            DeinterlaceRate::Frame
        };
        self.set_on(Filter::Deinterlace { deinterlacer, rate });
    }

    pub fn deinterlacer(&self) -> Deinterlacer {
        match self.filter(FilterKind::Deinterlace) {
            Some(Filter::Deinterlace { deinterlacer, .. }) => deinterlacer,
            _ => self.defaults.deinterlacer,
        }
    }

    pub fn deinterlace_rate(&self) -> DeinterlaceRate {
        match self.filter(FilterKind::Deinterlace) {
            Some(Filter::Deinterlace { rate, .. }) => rate,
            _ => DeinterlaceRate::Frame,
        }
    }

    /// The crop in the crop filter's orientation.
    pub fn crop(&self) -> Crop {
        match self.filter(FilterKind::Crop) {
            Some(Filter::Crop(c)) => c,
            _ => Crop::default(),
        }
    }

    /// The frame the crop filter gets (known once the probe is done).
    pub fn crop_frame(&self) -> Option<Size> {
        let source = self.source?;
        self.chain.size_before(FilterKind::Crop, source.size).ok()
    }

    /// The crop as seen in the output (after the rotations and mirrors that follow it): the card's fields.
    pub fn shown_crop(&self) -> Crop {
        self.chain.orientation_after(FilterKind::Crop).map_crop(self.crop())
    }

    /// Sets a crop (in the crop filter's orientation), limited to the frame; a crop that removes something switches
    /// the filter on.
    pub fn set_crop(&mut self, crop: Crop) {
        let crop = self.crop_frame().map_or(crop, |frame| crop.clamped(frame, 2));
        self.chain.set(Filter::Crop(crop));
        if !crop.is_identity() {
            self.chain.set_enabled(FilterKind::Crop, true);
        }
    }

    /// Sets one edge as seen in the output.
    pub fn set_shown_edge(&mut self, edge: Edge, value: u32) {
        let orientation = self.chain.orientation_after(FilterKind::Crop);
        let shown = edge.set(orientation.map_crop(self.crop()), value);
        let frame_crop = orientation.inverse().map_crop(shown);
        // Limit only the typed edge: the others keep their values.
        let limited = self.crop_frame().map_or(frame_crop, |frame| {
            let shown_frame = orientation.size(frame);
            let other = match edge {
                Edge::Top => shown.bottom,
                Edge::Bottom => shown.top,
                Edge::Left => shown.right,
                Edge::Right => shown.left,
            };
            let side = match edge {
                Edge::Top | Edge::Bottom => shown_frame.height,
                Edge::Left | Edge::Right => shown_frame.width,
            };
            let max = side.saturating_sub(other).saturating_sub(2);
            orientation.inverse().map_crop(edge.set(shown, value.min(max)))
        });
        self.set_crop(limited);
    }

    /// Changes one edge as seen in the output by `delta` pixels (the mouse wheel over a field).
    pub fn step_shown_edge(&mut self, edge: Edge, delta: i32) {
        let current = edge.get(self.shown_crop());
        let value = current.saturating_add_signed(delta);
        self.set_shown_edge(edge, value);
    }

    /// The rotation as quarter turns clockwise (0 when the filter is off).
    pub fn quarter_turns(&self) -> u8 {
        match (
            self.chain.is_enabled(FilterKind::Rotate),
            self.filter(FilterKind::Rotate),
        ) {
            (true, Some(Filter::Rotate { rotation })) => match rotation {
                Rotation::Clockwise => 1,
                Rotation::UpsideDown => 2,
                Rotation::CounterClockwise => 3,
            },
            _ => 0,
        }
    }

    /// Turns the output a quarter further (clockwise or not); a full turn switches the filter off.
    pub fn rotate(&mut self, clockwise: bool) {
        let turns = (self.quarter_turns() + if clockwise { 1 } else { 3 }) % 4;
        let rotation = match turns {
            1 => Some(Rotation::Clockwise),
            2 => Some(Rotation::UpsideDown),
            3 => Some(Rotation::CounterClockwise),
            _ => None,
        };
        match rotation {
            Some(rotation) => self.set_on(Filter::Rotate { rotation }),
            None => self.chain.set_enabled(FilterKind::Rotate, false),
        }
    }

    pub fn mirror(&self) -> Mirror {
        match (
            self.chain.is_enabled(FilterKind::Mirror),
            self.filter(FilterKind::Mirror),
        ) {
            (true, Some(Filter::Mirror(m))) => m,
            _ => Mirror::default(),
        }
    }

    /// Flips one mirror axis; without an axis the filter is off.
    pub fn toggle_mirror(&mut self, horizontal: bool) {
        let mut m = self.mirror();
        if horizontal {
            m.horizontal = !m.horizontal;
        } else {
            m.vertical = !m.vertical;
        }
        self.chain.set(Filter::Mirror(m));
        self.chain.set_enabled(FilterKind::Mirror, m.horizontal || m.vertical);
    }

    /// The resize's width, height and algorithm.
    pub fn resize(&self) -> (Option<u32>, Option<u32>, ResizeAlgorithm) {
        match self.filter(FilterKind::Resize) {
            Some(Filter::Resize {
                width,
                height,
                algorithm,
            }) => (width, height, algorithm),
            _ => (None, None, ResizeAlgorithm::default()),
        }
    }

    /// Sets the width (`which` = 0) or the height (1); `None` = follow the other side's aspect ratio. Sizes are limited
    /// to 2..=16384.
    pub fn set_resize_side(&mut self, which: u8, value: Option<u32>) {
        let (mut width, mut height, algorithm) = self.resize();
        let value = value.map(|v| v.clamp(2, MAX_RESIZE));
        if which == 0 {
            width = value;
        } else {
            height = value;
        }
        self.set_on(Filter::Resize {
            width,
            height,
            algorithm,
        });
    }

    /// The frame size that reaches the resize.
    pub fn resize_input(&self) -> Option<Size> {
        let source = self.source?;
        self.chain.size_before(FilterKind::Resize, source.size).ok()
    }

    /// The 16:9 / 4:3 helpers (PyCutRv): the height from the width (the frame's width when none is set), rounded up to
    /// an even number.
    pub fn resize_to_ratio(&mut self, ratio: f64) {
        let (width, _, algorithm) = self.resize();
        let Some(width) = width.or_else(|| self.resize_input().map(|s| s.width)) else {
            return;
        };
        let height = (f64::from(width) / ratio / 2.0).ceil() * 2.0;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the resize limits"
        )]
        let height = height.clamp(2.0, f64::from(MAX_RESIZE)) as u32;
        self.set_on(Filter::Resize {
            width: Some(width),
            height: Some(height),
            algorithm,
        });
    }

    pub fn set_resize_algorithm(&mut self, algorithm: ResizeAlgorithm) {
        let (width, height, _) = self.resize();
        self.chain.set(Filter::Resize {
            width,
            height,
            algorithm,
        });
    }

    pub fn deshake(&self) -> DeshakeParams {
        match self.filter(FilterKind::Deshake) {
            Some(Filter::Deshake(p)) => p,
            _ => self.defaults.deshake,
        }
    }

    pub fn set_deshake(&mut self, params: DeshakeParams) {
        self.chain.set(Filter::Deshake(params.sanitized()));
    }

    /// Chooses the aspect lock; while the crop is edited, the crop takes that shape at once.
    pub fn set_aspect(&mut self, aspect: CropAspect) {
        self.aspect = aspect;
        if self.crop.is_some()
            && let Some(view) = self.crop_view()
            && let Some(ratio) = aspect.ratio(view.shown_aspect())
        {
            let fitted = view.fit_aspect(self.crop(), ratio);
            self.chain.set(Filter::Crop(fitted));
        }
    }

    // --- What the player and the card show ---------------------------------------------------------------------------

    /// mpv's video filters: the editing view while the crop is edited, the chain when the preview is on, else none.
    /// Nothing before the probe tells the source's rotation.
    pub fn preview_filter(&self) -> VideoFilter {
        let Some(source) = self.source else {
            return VideoFilter::default();
        };
        let chain = if self.crop.is_some() {
            self.chain.for_crop_editing()
        } else if self.preview {
            self.chain.clone()
        } else {
            return VideoFilter::default();
        };
        match chain.preview_filter(source.rotation) {
            Ok(p) => VideoFilter {
                lavfi: p.lavfi,
                rotates_source: p.rotates_source,
            },
            Err(e) => {
                tracing::debug!(error = %e, "[Filters] No preview for this chain");
                VideoFilter::default()
            }
        }
    }

    /// The video area's geometry for the crop (known once the probe is done and the area has a size).
    pub fn crop_view(&self) -> Option<CropView> {
        let source = self.source?;
        let frame = self.crop_frame()?;
        let sar = self.chain.sample_aspect_before(FilterKind::Crop, source.sar);
        CropView::new(
            frame,
            sar,
            self.chain.orientation_after(FilterKind::Crop),
            self.view.0,
            self.view.1,
        )
    }
}
