//! What the Filters card, its header and the options menus show, computed from the state (pure, tested); the bridge
//! copies it into the `Filters` global.

use rustcut_core::edit::{
    ChainError, Crop, CropAspect, DeinterlaceRate, Deinterlacer, DeshakeParams, FieldParity, Filter, FilterKind,
    OrientationParams, ResizeAlgorithm, Size,
};
use rustcut_core::media::{CropSides, Interlacing};

use super::state::{CropMode, FiltersState, InterlaceHint};
use crate::util::amount::hundredths;

/// The deinterlacers in the order of the card's list.
pub const DEINTERLACERS: [Deinterlacer; 4] = Deinterlacer::ALL;
/// The resize algorithms in the order of the card's list.
pub const ALGORITHMS: [ResizeAlgorithm; 4] = [
    ResizeAlgorithm::Lanczos,
    ResizeAlgorithm::Bicubic,
    ResizeAlgorithm::Spline,
    ResizeAlgorithm::Bilinear,
];
/// The field orders in the order of the menu's segments.
pub const FIELD_ORDERS: [FieldParity; 3] = [FieldParity::Auto, FieldParity::TopFirst, FieldParity::BottomFirst];
/// The sides auto-crop searches, in the order of the menu's segments.
pub const SIDES: [CropSides; 3] = [CropSides::All, CropSides::TopBottom, CropSides::LeftRight];

/// How the interlace hint is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HintLevel {
    #[default]
    Hidden = 0,
    /// Plain information (progressive, checking, unclear).
    Muted = 1,
    /// Worth acting on: the video is interlaced.
    Attention = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub kind: FilterKind,
    pub enabled: bool,
    pub can_up: bool,
    pub can_down: bool,
}

impl RowView {
    /// Lines of the row: Crop has its tools and its edges (Q-P9.13: fixed row heights).
    pub const fn lines(&self) -> i32 {
        if matches!(self.kind, FilterKind::Crop) { 2 } else { 1 }
    }

    /// Orientation's values are all in its row; every other filter has an options menu.
    pub const fn has_menu(&self) -> bool {
        has_menu(self.kind)
    }
}

/// Whether `kind` has an options menu.
pub const fn has_menu(kind: FilterKind) -> bool {
    !matches!(kind, FilterKind::Orientation)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardView {
    pub has_media: bool,
    pub rows: Vec<RowView>,
    /// The header's output size ("1280×540"; empty without a probed video), what it means, and whether it is a problem.
    pub output: String,
    pub output_tooltip: String,
    pub output_warning: bool,
    pub deinterlacer: usize,
    pub field_rate: bool,
    /// The deinterlacer knows fields, the field order and "only interlaced frames".
    pub field_options: bool,
    pub field_order: usize,
    pub only_interlaced: bool,
    pub interlace_hint: String,
    pub interlace_level: HintLevel,
    pub interlace_detail: String,
    /// The tooltip's frame counts (label, value).
    pub interlace_rows: Vec<(String, String)>,
    /// The menu's line of counts ("1450 interlaced · 12 progressive").
    pub interlace_counts: String,
    pub interlace_checking: bool,
    /// The crop as seen in the output.
    pub crop: Crop,
    /// What the crop keeps as seen in the output ("1280×540"); empty before the probe.
    pub crop_size: String,
    pub crop_editing: bool,
    pub crop_quick: bool,
    pub can_edit_crop: bool,
    pub auto_crop_running: bool,
    pub aspect: usize,
    pub search_limit: u8,
    pub search_sides: usize,
    pub search_samples: u8,
    pub search_sections_only: bool,
    pub orientation: OrientationParams,
    /// Typed sizes ("" = automatic) and what "automatic" comes to (the fields' dim placeholders).
    pub resize_width: String,
    pub resize_height: String,
    pub resize_auto_width: String,
    pub resize_auto_height: String,
    pub resize_algorithm: usize,
    pub deshake: DeshakeParams,
    /// The menu's texts: shakiness, accuracy, search step, smoothing, zoom, max shift, max angle, zoom speed.
    pub deshake_values: [String; 8],
    /// The Deshake row's dim line.
    pub deshake_summary: String,
    pub preview: bool,
    pub keep: bool,
}

/// The card's view of `state`.
pub fn card(state: &FiltersState) -> CardView {
    let chain = state.chain();
    let count = chain.slots().len();
    let rows = chain
        .slots()
        .iter()
        .enumerate()
        .map(|(i, slot)| RowView {
            kind: slot.filter.kind(),
            enabled: slot.enabled,
            can_up: i > 0,
            can_down: i + 1 < count,
        })
        .collect();
    let (output, output_tooltip, output_warning) = output_text(state);
    let (interlace_hint, interlace_level, interlace_detail) = interlace(state.interlace);
    let (width, height, algorithm) = state.resize();
    let (auto_w, auto_h) = resize_auto(state, width, height);
    let session = state.crop_session();
    let (field_order, only_interlaced) = match state.deinterlace() {
        Filter::Deinterlace {
            field_order,
            only_interlaced,
            ..
        } => (field_order, only_interlaced),
        _ => (FieldParity::Auto, false),
    };
    let deshake = state.deshake();
    CardView {
        has_media: state.source().is_some(),
        rows,
        output,
        output_tooltip,
        output_warning,
        deinterlacer: DEINTERLACERS
            .iter()
            .position(|d| *d == state.deinterlacer())
            .unwrap_or(0),
        field_rate: state.deinterlace_rate() == DeinterlaceRate::Field,
        field_options: state.deinterlacer().has_field_options(),
        field_order: FIELD_ORDERS.iter().position(|o| *o == field_order).unwrap_or(0),
        only_interlaced,
        interlace_hint,
        interlace_level,
        interlace_detail,
        interlace_rows: interlace_rows(state.interlace),
        interlace_counts: interlace_counts(state.interlace),
        interlace_checking: state.interlace == InterlaceHint::Checking,
        crop: state.shown_crop(),
        crop_size: crop_size(state),
        crop_editing: session.is_some_and(|s| s.mode == CropMode::Edit),
        crop_quick: session.is_some_and(|s| s.mode == CropMode::Quick),
        can_edit_crop: state.crop_view().is_some(),
        auto_crop_running: state.auto_crop_running,
        aspect: CropAspect::ALL.iter().position(|a| *a == state.aspect).unwrap_or(0),
        search_limit: state.search.limit,
        search_sides: SIDES.iter().position(|s| *s == state.search.sides).unwrap_or(0),
        search_samples: state.search.samples,
        search_sections_only: state.search.sections_only,
        orientation: state.orientation(),
        resize_width: width.map(|w| w.to_string()).unwrap_or_default(),
        resize_height: height.map(|h| h.to_string()).unwrap_or_default(),
        resize_auto_width: auto_w,
        resize_auto_height: auto_h,
        resize_algorithm: ALGORITHMS.iter().position(|a| *a == algorithm).unwrap_or(0),
        deshake_values: deshake_values(deshake),
        deshake_summary: deshake_summary(deshake),
        deshake,
        preview: state.preview,
        keep: state.keep,
    }
}

/// The header's size after every filter, its tooltip, and whether it is a problem (the chain leaves no picture).
fn output_text(state: &FiltersState) -> (String, String, bool) {
    let Some(source) = state.source() else {
        return (String::new(), String::new(), false);
    };
    match state.chain().output_size(source.size) {
        Ok(size) if size.is_even() => (
            dims(size),
            format!("The output after every filter: {}", dims(size)),
            false,
        ),
        Ok(size) => (
            dims(size),
            format!(
                "The output after every filter: {}; the render makes it {} (even sizes)",
                dims(size),
                dims(size.even_floor())
            ),
            false,
        ),
        Err(ChainError::CropTooLarge { .. }) => {
            ("No picture".into(), "The crop removes the whole picture.".into(), true)
        }
        Err(e) => ("No picture".into(), capitalized(&e.to_string()), true),
    }
}

fn dims(size: Size) -> String {
    format!("{}×{}", size.width, size.height)
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What the crop keeps as seen in the output.
fn crop_size(state: &FiltersState) -> String {
    let Some(frame) = state.crop_frame() else {
        return String::new();
    };
    let shown = state.chain().orientation_after(FilterKind::Crop).size(frame);
    state.shown_crop().kept(shown).map(dims).unwrap_or_default()
}

/// The short hint next to the deinterlacer, how it is drawn, and its tooltip.
fn interlace(hint: InterlaceHint) -> (String, HintLevel, String) {
    match hint {
        InterlaceHint::None => (String::new(), HintLevel::Hidden, String::new()),
        InterlaceHint::Checking => (
            "checking…".into(),
            HintLevel::Muted,
            "Looking at a few hundred frames to tell whether the video is interlaced.".into(),
        ),
        InterlaceHint::Failed => (
            String::new(),
            HintLevel::Hidden,
            "The interlace check could not run.".into(),
        ),
        InterlaceHint::Known(verdict, _) => match verdict {
            Interlacing::Interlaced { top_field_first } => (
                "interlaced".into(),
                HintLevel::Attention,
                format!(
                    "The video is interlaced ({} field first): switch Deinterlace on, or the output shows combing \
                         on movement.",
                    if top_field_first { "top" } else { "bottom" }
                ),
            ),
            Interlacing::Progressive => (
                "progressive".into(),
                HintLevel::Muted,
                "The video has whole frames: Deinterlace is not needed.".into(),
            ),
            Interlacing::Unknown => (
                "unclear".into(),
                HintLevel::Muted,
                "The frames do not tell clearly: look at movement in the video.".into(),
            ),
        },
    }
}

/// The frames ffmpeg's idet counted (the hint's tooltip, Q-N10).
fn interlace_rows(hint: InterlaceHint) -> Vec<(String, String)> {
    let InterlaceHint::Known(_, c) = hint else {
        return Vec::new();
    };
    [
        ("Top field first", c.tff),
        ("Bottom field first", c.bff),
        ("Progressive", c.progressive),
        ("Undetermined", c.undetermined),
    ]
    .into_iter()
    .map(|(label, n)| (label.to_owned(), format!("{n} frames")))
    .collect()
}

/// The menu's line: interlaced (both field orders) and progressive frames.
fn interlace_counts(hint: InterlaceHint) -> String {
    match hint {
        InterlaceHint::None => String::new(),
        InterlaceHint::Checking => "Checking…".into(),
        InterlaceHint::Failed => "The check could not run".into(),
        InterlaceHint::Known(_, c) => format!(
            "{} interlaced · {} progressive",
            c.tff.saturating_add(c.bff),
            c.progressive
        ),
    }
}

/// What an empty width or height comes to (ffmpeg keeps the aspect ratio): the field's dim placeholder (13#3).
fn resize_auto(state: &FiltersState, width: Option<u32>, height: Option<u32>) -> (String, String) {
    let Some(input) = state.resize_input() else {
        return (String::new(), String::new());
    };
    let mut probe = rustcut_core::edit::FilterChain::default();
    probe.set(Filter::Resize {
        width,
        height,
        algorithm: ResizeAlgorithm::default(),
    });
    probe.set_enabled(FilterKind::Resize, true);
    let out = probe.output_size(input).unwrap_or(input);
    (out.width.to_string(), out.height.to_string())
}

/// The menu's deshake texts (empty = no limit).
fn deshake_values(d: DeshakeParams) -> [String; 8] {
    [
        d.shakiness.to_string(),
        d.accuracy.to_string(),
        d.stepsize.to_string(),
        d.smoothing.to_string(),
        d.zoom.to_string(),
        d.max_shift.map(|v| v.to_string()).unwrap_or_default(),
        d.max_angle.map(|v| v.to_string()).unwrap_or_default(),
        hundredths(d.zoom_speed),
    ]
}

/// The Deshake row's line: the two values that matter most, and the switches that change the result's look.
fn deshake_summary(d: DeshakeParams) -> String {
    let mut parts = vec![
        format!("shakiness {}", d.shakiness),
        format!("smoothing {}", d.smoothing),
    ];
    if d.tripod {
        parts.push("tripod".into());
    }
    match d.optzoom {
        1 => parts.push("static zoom".into()),
        2 => parts.push("adaptive zoom".into()),
        _ => {}
    }
    if d.zoom != 0 {
        parts.push(format!("zoom {:+}%", d.zoom));
    }
    parts.join(" · ")
}
