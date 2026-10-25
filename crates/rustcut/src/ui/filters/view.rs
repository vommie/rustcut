//! What the Filters card shows, computed from the state (pure, tested); the bridge copies it into the `Filters` global.

use rustcut_core::edit::{
    ChainError, Crop, CropAspect, DeinterlaceRate, Deinterlacer, DeshakeParams, FilterKind, OrientationParams,
    ResizeAlgorithm, Size,
};
use rustcut_core::media::Interlacing;

use super::state::{CropMode, FiltersState, InterlaceHint};

/// The deinterlacers in the order of the card's list.
pub const DEINTERLACERS: [Deinterlacer; 4] = Deinterlacer::ALL;
/// The resize algorithms in the order of the card's list.
pub const ALGORITHMS: [ResizeAlgorithm; 4] = [
    ResizeAlgorithm::Lanczos,
    ResizeAlgorithm::Bicubic,
    ResizeAlgorithm::Spline,
    ResizeAlgorithm::Bilinear,
];

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

#[derive(Debug, Clone, PartialEq)]
pub struct CardView {
    pub has_media: bool,
    pub rows: Vec<RowView>,
    /// "Output 1920×1080"; empty without a probed video.
    pub output: String,
    pub output_warning: bool,
    pub deinterlacer: usize,
    pub field_rate: bool,
    pub field_rate_supported: bool,
    pub interlace_hint: String,
    pub interlace_level: HintLevel,
    pub interlace_detail: String,
    /// The tooltip's frame counts (label, value).
    pub interlace_rows: Vec<(String, String)>,
    /// The crop as seen in the output.
    pub crop: Crop,
    pub crop_editing: bool,
    pub crop_quick: bool,
    pub can_edit_crop: bool,
    pub auto_crop_running: bool,
    pub aspect: usize,
    pub orientation: OrientationParams,
    /// Typed sizes ("" = automatic) and what "automatic" comes to.
    pub resize_width: String,
    pub resize_height: String,
    pub resize_auto_width: String,
    pub resize_auto_height: String,
    pub resize_algorithm: usize,
    pub deshake: DeshakeParams,
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
    let (output, output_warning) = output_text(state);
    let (interlace_hint, interlace_level, interlace_detail) = interlace(state.interlace);
    let interlace_rows = interlace_counts(state.interlace);
    let (width, height, algorithm) = state.resize();
    let (auto_w, auto_h) = resize_auto(state, width, height);
    let session = state.crop_session();
    CardView {
        has_media: state.source().is_some(),
        rows,
        output,
        output_warning,
        deinterlacer: DEINTERLACERS
            .iter()
            .position(|d| *d == state.deinterlacer())
            .unwrap_or(0),
        field_rate: state.deinterlace_rate() == DeinterlaceRate::Field,
        field_rate_supported: state.deinterlacer().supports_field_rate(),
        interlace_hint,
        interlace_level,
        interlace_detail,
        interlace_rows,
        crop: state.shown_crop(),
        crop_editing: session.is_some_and(|s| s.mode == CropMode::Edit),
        crop_quick: session.is_some_and(|s| s.mode == CropMode::Quick),
        can_edit_crop: state.crop_view().is_some(),
        auto_crop_running: state.auto_crop_running,
        aspect: CropAspect::ALL.iter().position(|a| *a == state.aspect).unwrap_or(0),
        orientation: state.orientation(),
        resize_width: width.map(|w| w.to_string()).unwrap_or_default(),
        resize_height: height.map(|h| h.to_string()).unwrap_or_default(),
        resize_auto_width: auto_w,
        resize_auto_height: auto_h,
        resize_algorithm: ALGORITHMS.iter().position(|a| *a == algorithm).unwrap_or(0),
        deshake: state.deshake(),
        preview: state.preview,
        keep: state.keep,
    }
}

/// "Output 1920×1080", with the even size the render makes of an odd one, or why there is no output.
fn output_text(state: &FiltersState) -> (String, bool) {
    let Some(source) = state.source() else {
        return (String::new(), false);
    };
    match state.chain().output_size(source.size) {
        Ok(size) if size.is_even() => (format!("Output {}", dims(size)), false),
        Ok(size) => (
            format!("Output {} (rendered {})", dims(size), dims(size.even_floor())),
            false,
        ),
        Err(ChainError::CropTooLarge { .. }) => ("The crop removes the whole picture".into(), true),
        Err(e) => (capitalized(&e.to_string()), true),
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

/// The hint next to "Deinterlace", how it is drawn, and its tooltip.
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
                format!("Interlaced ({})", if top_field_first { "TFF" } else { "BFF" }),
                HintLevel::Attention,
                format!(
                    "The video is interlaced ({} field first): switch Deinterlace on, or the output shows combing \
                         on movement.",
                    if top_field_first { "top" } else { "bottom" }
                ),
            ),
            Interlacing::Progressive => (
                "Progressive".into(),
                HintLevel::Muted,
                "The video has whole frames: Deinterlace is not needed.".into(),
            ),
            Interlacing::Unknown => (
                "Unclear".into(),
                HintLevel::Muted,
                "The frames do not tell clearly: look at movement in the video.".into(),
            ),
        },
    }
}

/// The frames ffmpeg's idet counted (the interlace note's tooltip, Q-N10).
fn interlace_counts(hint: InterlaceHint) -> Vec<(String, String)> {
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

/// What an empty width or height comes to (ffmpeg keeps the aspect ratio), as a field placeholder.
fn resize_auto(state: &FiltersState, width: Option<u32>, height: Option<u32>) -> (String, String) {
    let Some(input) = state.resize_input() else {
        return ("auto".into(), "auto".into());
    };
    let mut probe = rustcut_core::edit::FilterChain::default();
    probe.set(rustcut_core::edit::Filter::Resize {
        width,
        height,
        algorithm: ResizeAlgorithm::default(),
    });
    probe.set_enabled(FilterKind::Resize, true);
    let out = probe.output_size(input).unwrap_or(input);
    (format!("auto ({})", out.width), format!("auto ({})", out.height))
}
