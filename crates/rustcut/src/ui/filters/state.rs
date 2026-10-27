//! The filters of the open video (pure, tested): the chain with every operation of the Filters card, what mpv's preview
//! gets, and the crop session on the video (the edit mode with handles, or the quick mode of the Quick crop key).

use rustcut_core::edit::{
    Crop, DeinterlaceRate, Deinterlacer, DeshakeParams, FieldParity, Filter, FilterChain, FilterKind,
    OrientationParams, RatioChoice, RatioSet, ResizeAlgorithm, Size,
};
use rustcut_core::media::{CropSearch, IdetCounts, Interlacing};
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
    /// The crop's ratio choice (the ratio button on the crop, P9.14) and the presets of Settings > Filters.
    pub ratio: RatioChoice,
    pub(super) ratios: RatioSet,
    pub interlace: InterlaceHint,
    /// How auto-crop searches (kept and reset with the chain).
    pub search: CropSearch,
    pub auto_crop_running: bool,
    pub(super) crop: Option<CropSession>,
    /// The video area in logical pixels.
    view: (f32, f32),
}

/// What "keep" carries to the next video and the next start (`ui-state.toml`): the chain and the auto-crop search
/// (the crop's size multiple of 0.0.51-0.0.59 became the final size rule of Settings > Filters, Q-P9s).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kept {
    pub chain: Option<FilterChain>,
    pub search: Option<CropSearch>,
}

/// A fresh chain with the configured defaults (Settings > Filters), every filter off.
pub fn default_chain(defaults: &FilterDefaults) -> FilterChain {
    defaults.chain()
}

impl FiltersState {
    /// The defaults a fresh chain gets from now on (the current chain stays as it is); the ratio choice keeps its label
    /// when the presets changed (the source's ratio when it is gone).
    pub fn set_defaults(&mut self, defaults: FilterDefaults) {
        let label = self.ratios.label(self.ratio).to_owned();
        self.ratios = defaults.ratios();
        self.ratio = self.ratios.choice_of(&label).unwrap_or_default();
        self.defaults = defaults;
    }

    /// The state at startup: the kept chain and search when "keep" was on, else the defaults; `ratio` = the label of
    /// the crop's ratio choice (the source's ratio when it is unknown).
    pub fn new(defaults: FilterDefaults, preview: bool, keep: bool, kept: Kept, ratio: Option<&str>) -> Self {
        let chain = kept.chain.filter(|_| keep).unwrap_or_else(|| default_chain(&defaults));
        let search = kept.search.filter(|_| keep).unwrap_or(defaults.crop_search).sanitized();
        let ratios = defaults.ratios();
        let ratio = ratio.and_then(|label| ratios.choice_of(label)).unwrap_or_default();
        Self {
            chain,
            defaults,
            source: None,
            preview,
            keep,
            ratio,
            ratios,
            interlace: InterlaceHint::None,
            search,
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

    /// What to keep for the next start (nothing while "keep" is off).
    pub fn kept(&self) -> Kept {
        if !self.keep {
            return Kept::default();
        }
        Kept {
            chain: Some(self.chain.clone()),
            search: Some(self.search),
        }
    }

    pub const fn defaults(&self) -> &FilterDefaults {
        &self.defaults
    }

    /// The crop's aspect ratio presets (Settings > Filters).
    pub const fn ratios(&self) -> &RatioSet {
        &self.ratios
    }

    /// The label of the crop's ratio choice ("O" or a preset's), kept in `ui-state.toml`.
    pub fn ratio_choice_label(&self) -> &str {
        self.ratios.label(self.ratio)
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
            self.search = self.defaults.crop_search.sanitized();
        }
    }

    /// The chain of a job loaded into the editor, or of a draft put back (a crop being edited ends first).
    pub fn load_chain(&mut self, chain: FilterChain) {
        self.crop = None;
        self.chain = chain;
    }

    pub const fn source_known(&mut self, source: Source) {
        self.source = Some(source);
    }

    /// The video area changed size.
    pub const fn set_view(&mut self, width: f32, height: f32) {
        self.view = (width, height);
    }

    // --- The card ----------------------------------------------------------------------------------------------------

    /// Switches a filter on or off; its values stay.
    pub fn toggle(&mut self, kind: FilterKind) {
        let on = !self.chain.is_enabled(kind);
        self.chain.set_enabled(kind, on);
        if kind == FilterKind::Crop && !on {
            self.crop = None;
        }
    }

    /// Switches a filter on (an action that wants to show its result: a key, auto-crop, the notice's button).
    pub fn switch_on(&mut self, kind: FilterKind) {
        self.chain.set_enabled(kind, true);
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
        self.search = self.defaults.crop_search.sanitized();
        self.crop = None;
    }

    pub fn filter(&self, kind: FilterKind) -> Option<Filter> {
        self.chain.get(kind).map(|slot| slot.filter)
    }

    /// The deinterlace filter's values (whether it is on or not).
    pub fn deinterlace(&self) -> Filter {
        match self.filter(FilterKind::Deinterlace) {
            Some(f @ Filter::Deinterlace { .. }) => f,
            _ => self.defaults.deinterlace(),
        }
    }

    /// Changes the deinterlace filter's values; a filter that is off stays off (user decision Q-P9.13).
    fn change_deinterlace(
        &mut self,
        change: impl FnOnce(&mut Deinterlacer, &mut DeinterlaceRate, &mut FieldParity, &mut bool),
    ) {
        if let Filter::Deinterlace {
            mut deinterlacer,
            mut rate,
            mut field_order,
            mut only_interlaced,
        } = self.deinterlace()
        {
            change(&mut deinterlacer, &mut rate, &mut field_order, &mut only_interlaced);
            self.chain.set(Filter::Deinterlace {
                deinterlacer,
                rate,
                field_order,
                only_interlaced,
            });
        }
    }

    pub fn set_deinterlacer(&mut self, deinterlacer: Deinterlacer) {
        self.change_deinterlace(|d, _, _, _| *d = deinterlacer);
    }

    pub fn set_field_rate(&mut self, field: bool) {
        self.change_deinterlace(|_, rate, _, _| {
            *rate = if field {
                DeinterlaceRate::Field
            } else {
                DeinterlaceRate::Frame
            };
        });
    }

    pub fn set_field_order(&mut self, order: FieldParity) {
        self.change_deinterlace(|_, _, field_order, _| *field_order = order);
    }

    pub fn set_only_interlaced(&mut self, only: bool) {
        self.change_deinterlace(|_, _, _, only_interlaced| *only_interlaced = only);
    }

    pub fn deinterlacer(&self) -> Deinterlacer {
        match self.deinterlace() {
            Filter::Deinterlace { deinterlacer, .. } => deinterlacer,
            _ => self.defaults.deinterlacer,
        }
    }

    pub fn deinterlace_rate(&self) -> DeinterlaceRate {
        match self.deinterlace() {
            Filter::Deinterlace { rate, .. } => rate,
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

    /// Sets a crop (in the crop filter's orientation), limited to the frame; a filter that is off stays off.
    pub fn set_crop(&mut self, crop: Crop) {
        let crop = self.crop_frame().map_or(crop, |frame| crop.clamped(frame, 2));
        self.chain.set(Filter::Crop(crop));
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

    /// The orientation's values (whether it is on or not).
    pub fn orientation(&self) -> OrientationParams {
        match self.filter(FilterKind::Orientation) {
            Some(Filter::Orientation(p)) => p,
            _ => OrientationParams::UPRIGHT,
        }
    }

    /// The turn buttons and keys: a quarter to the right or the left, like radio buttons (the one that is on switches
    /// off); Orientation switches on (user decision Q-P9.13b: its buttons and keys show their result at once).
    pub fn turn(&mut self, clockwise: bool) {
        self.chain
            .set(Filter::Orientation(self.orientation().pick_turn(clockwise)));
        self.switch_on(FilterKind::Orientation);
    }

    /// The mirror buttons and keys: switch the mirror left to right (`horizontal`) or top to bottom; Orientation
    /// switches on.
    pub fn flip(&mut self, horizontal: bool) {
        self.chain
            .set(Filter::Orientation(self.orientation().toggle_mirror(horizontal)));
        self.switch_on(FilterKind::Orientation);
    }

    /// The resize's width, height and algorithm.
    pub fn resize(&self) -> (Option<u32>, Option<u32>, ResizeAlgorithm) {
        match self.filter(FilterKind::Resize) {
            Some(Filter::Resize {
                width,
                height,
                algorithm,
            }) => (width, height, algorithm),
            _ => (None, None, self.defaults.resize_algorithm),
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
        self.chain.set(Filter::Resize {
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
        self.chain.set(Filter::Resize {
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
