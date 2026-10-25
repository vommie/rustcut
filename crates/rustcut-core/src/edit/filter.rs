//! The filters RustCut offers and their parameters (PyCutRv parity plus orientation, field order and configurable
//! deshake).

use serde::{Deserialize, Serialize};

use super::{Crop, Orientation, OrientationParams};

/// The kind of a filter; a chain holds each kind exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Deinterlace,
    Crop,
    Deshake,
    /// Turns and mirrors in one (0.0.48; replaces Rotate and Mirror, which old data still names).
    #[serde(alias = "rotate", alias = "mirror")]
    Orientation,
    Resize,
}

impl FilterKind {
    /// Default order of a new chain: deinterlace first (it needs the original fields), crop before deshake (black bars would
    /// pull the motion estimate toward zero), resize last (the output size is exactly what was typed).
    pub const DEFAULT_ORDER: [Self; 5] = [
        Self::Deinterlace,
        Self::Crop,
        Self::Deshake,
        Self::Orientation,
        Self::Resize,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Deinterlace => "Deinterlace",
            Self::Crop => "Crop",
            Self::Deshake => "Deshake",
            Self::Orientation => "Orientation",
            Self::Resize => "Resize",
        }
    }
}

/// A filter with its parameters. Disabled filters keep their parameters in the chain ([`super::FilterSlot`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filter {
    Deinterlace {
        #[serde(default)]
        deinterlacer: Deinterlacer,
        #[serde(default)]
        rate: DeinterlaceRate,
        /// Which field comes first (0.0.48; auto = what the frames say).
        #[serde(default)]
        field_order: FieldParity,
        /// Leaves frames alone that are not marked as interlaced (0.0.48; ffmpeg's `deint=interlaced`).
        #[serde(default)]
        only_interlaced: bool,
    },
    Crop(Crop),
    Deshake(DeshakeParams),
    /// How the picture is turned and mirrored
    /// (`{"kind":"orientation","turns":1,"mirror_horizontal":false,"mirror_vertical":true}`).
    Orientation(OrientationParams),
    Resize {
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        algorithm: ResizeAlgorithm,
    },
}

impl Filter {
    pub const fn kind(&self) -> FilterKind {
        match self {
            Self::Deinterlace { .. } => FilterKind::Deinterlace,
            Self::Crop(_) => FilterKind::Crop,
            Self::Deshake(_) => FilterKind::Deshake,
            Self::Orientation(_) => FilterKind::Orientation,
            Self::Resize { .. } => FilterKind::Resize,
        }
    }

    /// The filter of `kind` with its default parameters (orientation: upright, resize: unchanged size).
    pub fn default_for(kind: FilterKind) -> Self {
        match kind {
            FilterKind::Deinterlace => Self::Deinterlace {
                deinterlacer: Deinterlacer::default(),
                rate: DeinterlaceRate::default(),
                field_order: FieldParity::default(),
                only_interlaced: false,
            },
            FilterKind::Crop => Self::Crop(Crop::default()),
            FilterKind::Deshake => Self::Deshake(DeshakeParams::default()),
            FilterKind::Orientation => Self::Orientation(OrientationParams::UPRIGHT),
            FilterKind::Resize => Self::Resize {
                width: None,
                height: None,
                algorithm: ResizeAlgorithm::default(),
            },
        }
    }

    /// Whether the filter changes nothing (zero crop, upright orientation, resize without a size).
    pub const fn is_identity(&self) -> bool {
        match self {
            Self::Crop(c) => c.is_identity(),
            Self::Orientation(o) => o.is_identity(),
            Self::Resize { width, height, .. } => width.is_none() && height.is_none(),
            Self::Deinterlace { .. } | Self::Deshake(_) => false,
        }
    }

    /// The same filter placed after `turn` instead of before it: `self` then `turn` equals `turn` then the result. A crop
    /// keeps the same pixels, a resize trades its sides with a quarter turn; the others do not depend on the turn.
    pub fn seen_through(self, turn: Orientation) -> Self {
        match self {
            Self::Crop(c) => Self::Crop(turn.map_crop(c)),
            Self::Resize {
                width,
                height,
                algorithm,
            } if turn.swaps_axes() => Self::Resize {
                width: height,
                height: width,
                algorithm,
            },
            other => other,
        }
    }
}

/// Deinterlacing filters of ffmpeg that need no extra files (`nnedi` is not offered: it needs a weights file, PyCutRv bug 13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deinterlacer {
    /// Bob Weaver: motion adaptive, good quality at a moderate cost (ffmpeg's recommendation).
    #[default]
    Bwdif,
    /// Yet Another Deinterlacing Filter: fast, slightly softer than bwdif.
    Yadif,
    /// Weston 3-field: sharp, for content with little motion.
    W3fdif,
    /// Donald Graft's kernel deinterlacer: fast, frame rate only.
    Kerndeint,
}

impl Deinterlacer {
    pub const ALL: [Self; 4] = [Self::Bwdif, Self::Yadif, Self::W3fdif, Self::Kerndeint];

    pub const fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Bwdif => "bwdif",
            Self::Yadif => "yadif",
            Self::W3fdif => "w3fdif",
            Self::Kerndeint => "kerndeint",
        }
    }

    /// Whether the filter can output one frame per field ([`DeinterlaceRate::Field`]).
    pub const fn supports_field_rate(self) -> bool {
        !matches!(self, Self::Kerndeint)
    }

    /// Whether the filter takes the field order and "only interlaced frames" (`parity`, `deint`; kerndeint has neither).
    pub const fn has_field_options(self) -> bool {
        !matches!(self, Self::Kerndeint)
    }
}

/// Output rate of a deinterlacer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeinterlaceRate {
    /// One frame per frame (25i -> 25p).
    #[default]
    Frame,
    /// One frame per field (25i -> 50p): smoother motion, twice the frames to encode.
    Field,
}

/// Which field of an interlaced frame comes first (`parity` of bwdif, yadif and w3fdif).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldParity {
    /// What the frames say (ffmpeg's default).
    #[default]
    Auto,
    TopFirst,
    BottomFirst,
}

impl FieldParity {
    /// The `parity` value; `None` = automatic (left out).
    pub const fn ffmpeg_parity(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::TopFirst => Some("tff"),
            Self::BottomFirst => Some("bff"),
        }
    }
}

/// Rotation in 90 degree steps (PyCutRv: left, right, 180): the Rotate filter of 0.0.47 and older, read from old jobs
/// and kept filters ([`Orientation::from_rotation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    Clockwise,
    CounterClockwise,
    UpsideDown,
}

impl Rotation {
    /// Whether width and height trade places.
    pub const fn swaps_axes(self) -> bool {
        !matches!(self, Self::UpsideDown)
    }
}

/// Mirror axes (PyCutRv had a mirror button without function): the Mirror filter of 0.0.47 and older, read from old jobs
/// and kept filters ([`Orientation::from_mirror`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Mirror {
    /// Left and right trade places (`hflip`).
    pub horizontal: bool,
    /// Top and bottom trade places (`vflip`).
    pub vertical: bool,
}

/// Scaling algorithm of the resize filter (`scale` `flags`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeAlgorithm {
    Bilinear,
    /// ffmpeg's default: a good balance of sharpness and speed.
    #[default]
    Bicubic,
    /// Sharpest for downscaling, slightly slower.
    Lanczos,
    Spline,
}

impl ResizeAlgorithm {
    pub const fn ffmpeg_flag(self) -> &'static str {
        match self {
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
            Self::Spline => "spline",
        }
    }
}

/// Parameters of the two-pass vid.stab deshake. Defaults are PyCutRv's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct DeshakeParams {
    /// How shaky the video is, 1 (little) to 10 (very strong). Higher is slower.
    pub shakiness: u8,
    /// Detection accuracy, 1 to 15; must be at least `shakiness`. Higher is slower.
    pub accuracy: u8,
    /// Search step in pixels, 1 to 32. Smaller is more accurate and slower.
    pub stepsize: u8,
    /// Frames used to smooth the camera path (`2 * n + 1`), 0 to 1000. Higher is steadier but follows pans later.
    pub smoothing: u16,
    /// Extra zoom in percent, -100 to 100.
    pub zoom: i8,
    /// Automatic zoom: 0 none, 1 optimal static, 2 optimal adaptive.
    pub optzoom: u8,
    pub crop: DeshakeCrop,
    pub interpolation: DeshakeInterpolation,
    /// Virtual tripod: keep the camera fixed on the first frame.
    pub tripod: bool,
    /// The largest shift in pixels, 0 to 500 (`maxshift`); `None` = no limit (0.0.48).
    pub max_shift: Option<u16>,
    /// The largest turn in degrees, 0 to 90 (`maxangle`, which ffmpeg takes in radians); `None` = no limit (0.0.48).
    pub max_angle: Option<u8>,
    /// How fast the adaptive zoom (`optzoom` 2) may change, in hundredths of a percent per frame, 0 to 500
    /// (`zoomspeed` 0 to 5 %; ffmpeg's default 0.25 %; 0.0.48).
    pub zoom_speed: u16,
}

impl Default for DeshakeParams {
    fn default() -> Self {
        Self {
            shakiness: 5,
            accuracy: 10,
            stepsize: 12,
            smoothing: 10,
            zoom: 0,
            optzoom: 0,
            crop: DeshakeCrop::Black,
            interpolation: DeshakeInterpolation::Bicubic,
            tripod: false,
            max_shift: None,
            max_angle: None,
            zoom_speed: Self::ZOOM_SPEED_DEFAULT,
        }
    }
}

impl DeshakeParams {
    pub const MAX_SHIFT: u16 = 500;
    pub const MAX_ANGLE: u8 = 90;
    pub const ZOOM_SPEED_MAX: u16 = 500;
    pub const ZOOM_SPEED_DEFAULT: u16 = 25;

    /// The parameters limited to the ranges vid.stab accepts (`accuracy` raised to `shakiness`).
    pub fn sanitized(self) -> Self {
        let shakiness = self.shakiness.clamp(1, 10);
        Self {
            shakiness,
            accuracy: self.accuracy.clamp(shakiness, 15),
            stepsize: self.stepsize.clamp(1, 32),
            smoothing: self.smoothing.min(1000),
            zoom: self.zoom.clamp(-100, 100),
            optzoom: self.optzoom.min(2),
            max_shift: self.max_shift.map(|v| v.min(Self::MAX_SHIFT)),
            max_angle: self.max_angle.map(|v| v.min(Self::MAX_ANGLE)),
            zoom_speed: self.zoom_speed.min(Self::ZOOM_SPEED_MAX),
            ..self
        }
    }
}

/// What vid.stab shows where the stabilized picture moved away from the frame edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeshakeCrop {
    /// Black borders.
    #[default]
    Black,
    /// The previous frame's content.
    Keep,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeshakeInterpolation {
    No,
    Linear,
    Bilinear,
    #[default]
    Bicubic,
}

impl DeshakeInterpolation {
    pub const fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::Linear => "linear",
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn default_filters_have_their_kind() {
        for kind in FilterKind::DEFAULT_ORDER {
            assert_eq!(Filter::default_for(kind).kind(), kind);
        }
    }

    #[test]
    fn serialized_form_is_tagged() {
        let f = Filter::Orientation(OrientationParams {
            turns: 3,
            mirror_horizontal: false,
            mirror_vertical: true,
        });
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"orientation","turns":3,"mirror_horizontal":false,"mirror_vertical":true}"#
        );
        assert_eq!(serde_json::from_str::<Filter>(&json).unwrap(), f);
        let deinterlace: Filter = serde_json::from_str(r#"{"kind":"deinterlace","deinterlacer":"yadif"}"#).unwrap();
        assert_eq!(
            deinterlace,
            Filter::Deinterlace {
                deinterlacer: Deinterlacer::Yadif,
                rate: DeinterlaceRate::Frame,
                field_order: FieldParity::Auto,
                only_interlaced: false,
            },
            "a deinterlace of 0.0.47 reads with the new options at their defaults"
        );
        let crop: Filter = serde_json::from_str(r#"{"kind":"crop","top":8}"#).unwrap();
        assert_eq!(crop, Filter::Crop(Crop::new(8, 0, 0, 0)));
        let deshake: Filter = serde_json::from_str(r#"{"kind":"deshake","smoothing":30}"#).unwrap();
        assert_eq!(
            deshake,
            Filter::Deshake(DeshakeParams {
                smoothing: 30,
                ..DeshakeParams::default()
            })
        );
    }

    #[test]
    fn seen_through_keeps_the_pixels_and_trades_the_sides() {
        let crop = Filter::Crop(Crop::new(1, 2, 3, 4));
        assert_eq!(
            crop.seen_through(Orientation::CLOCKWISE),
            Filter::Crop(Crop::new(4, 1, 2, 3))
        );
        let resize = Filter::Resize {
            width: Some(1280),
            height: None,
            algorithm: ResizeAlgorithm::Lanczos,
        };
        assert_eq!(
            resize.seen_through(Orientation::COUNTER_CLOCKWISE),
            Filter::Resize {
                width: None,
                height: Some(1280),
                algorithm: ResizeAlgorithm::Lanczos,
            }
        );
        assert_eq!(resize.seen_through(Orientation::MIRROR_VERTICAL), resize);
    }

    proptest! {
        #[test]
        fn sanitized_deshake_is_in_range(sh: u8, acc: u8, step: u8, smooth: u16, zoom: i8, opt: u8,
            shift: Option<u16>, angle: Option<u8>, speed: u16) {
            let p = DeshakeParams { shakiness: sh, accuracy: acc, stepsize: step, smoothing: smooth, zoom, optzoom: opt,
                max_shift: shift, max_angle: angle, zoom_speed: speed, ..DeshakeParams::default() }.sanitized();
            prop_assert!((1..=10).contains(&p.shakiness));
            prop_assert!(p.accuracy >= p.shakiness && p.accuracy <= 15);
            prop_assert!((1..=32).contains(&p.stepsize));
            prop_assert!(p.smoothing <= 1000 && (-100..=100).contains(&p.zoom) && p.optzoom <= 2);
            prop_assert!(p.max_shift.is_none_or(|v| v <= 500) && p.max_angle.is_none_or(|v| v <= 90));
            prop_assert!(p.zoom_speed <= 500);
            prop_assert_eq!(p.sanitized(), p);
        }
    }
}
