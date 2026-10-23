//! The filters RustCut offers and their parameters (PyCutRv parity plus mirror and configurable deshake).

use serde::{Deserialize, Serialize};

use super::Crop;

/// The kind of a filter; a chain holds each kind exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Deinterlace,
    Crop,
    Deshake,
    Rotate,
    Mirror,
    Resize,
}

impl FilterKind {
    /// Default order of a new chain: deinterlace first (it needs the original fields), crop before deshake (black bars would
    /// pull the motion estimate toward zero), resize last (the output size is exactly what was typed).
    pub const DEFAULT_ORDER: [Self; 6] = [
        Self::Deinterlace,
        Self::Crop,
        Self::Deshake,
        Self::Rotate,
        Self::Mirror,
        Self::Resize,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Deinterlace => "Deinterlace",
            Self::Crop => "Crop",
            Self::Deshake => "Deshake",
            Self::Rotate => "Rotate",
            Self::Mirror => "Mirror",
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
    },
    Crop(Crop),
    Deshake(DeshakeParams),
    Rotate {
        rotation: Rotation,
    },
    Mirror(Mirror),
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
            Self::Rotate { .. } => FilterKind::Rotate,
            Self::Mirror(_) => FilterKind::Mirror,
            Self::Resize { .. } => FilterKind::Resize,
        }
    }

    /// The filter of `kind` with its default parameters (rotate: clockwise, mirror: horizontal, resize: unchanged size).
    pub fn default_for(kind: FilterKind) -> Self {
        match kind {
            FilterKind::Deinterlace => Self::Deinterlace {
                deinterlacer: Deinterlacer::default(),
                rate: DeinterlaceRate::default(),
            },
            FilterKind::Crop => Self::Crop(Crop::default()),
            FilterKind::Deshake => Self::Deshake(DeshakeParams::default()),
            FilterKind::Rotate => Self::Rotate {
                rotation: Rotation::Clockwise,
            },
            FilterKind::Mirror => Self::Mirror(Mirror {
                horizontal: true,
                vertical: false,
            }),
            FilterKind::Resize => Self::Resize {
                width: None,
                height: None,
                algorithm: ResizeAlgorithm::default(),
            },
        }
    }

    /// Whether the filter changes nothing (zero crop, no mirror axis, resize without a size).
    pub const fn is_identity(&self) -> bool {
        match self {
            Self::Crop(c) => c.is_identity(),
            Self::Mirror(m) => !m.horizontal && !m.vertical,
            Self::Resize { width, height, .. } => width.is_none() && height.is_none(),
            Self::Deinterlace { .. } | Self::Deshake(_) | Self::Rotate { .. } => false,
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

/// Rotation in 90 degree steps (PyCutRv: left, right, 180).
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

/// Mirror axes (PyCutRv had a mirror button without function).
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
        }
    }
}

impl DeshakeParams {
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
        let f = Filter::Rotate {
            rotation: Rotation::CounterClockwise,
        };
        assert_eq!(
            serde_json::to_string(&f).unwrap(),
            r#"{"kind":"rotate","rotation":"counter_clockwise"}"#
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

    proptest! {
        #[test]
        fn sanitized_deshake_is_in_range(sh: u8, acc: u8, step: u8, smooth: u16, zoom: i8, opt: u8) {
            let p = DeshakeParams { shakiness: sh, accuracy: acc, stepsize: step, smoothing: smooth, zoom, optzoom: opt, ..DeshakeParams::default() }.sanitized();
            prop_assert!((1..=10).contains(&p.shakiness));
            prop_assert!(p.accuracy >= p.shakiness && p.accuracy <= 15);
            prop_assert!((1..=32).contains(&p.stepsize));
            prop_assert!(p.smoothing <= 1000 && (-100..=100).contains(&p.zoom) && p.optzoom <= 2);
            prop_assert_eq!(p.sanitized(), p);
        }
    }
}
