//! Settings > Filters: what a fresh filter chain starts with (each filter's menu saves its values here, P9.13). The
//! options were checked against `ffmpeg -h filter=NAME` and with the real ffmpeg (Q-P9.13).

use rustcut_core::edit::{
    DeinterlaceRate, Deinterlacer, DeshakeCrop, DeshakeInterpolation, DeshakeParams, FieldParity, ResizeAlgorithm,
};
use rustcut_core::media::{CropSearch, CropSides};
use rustcut_core::settings::Settings;

use super::Card;
use crate::ui::settings::field::{Field, Kind, Value, WRONG_VALUE, always, choice, number, switch};
use crate::util::amount::{hundredths, limit, parse_hundredths};

/// A deshake parameter: kept in vid.stab's ranges (accuracy is never below shakiness).
macro_rules! deshake {
    ($key:literal, $label:literal, $help:literal, $field:ident : $t:ty, $min:literal..=$max:literal, $step:literal, $unit:literal) => {
        Field {
            key: $key,
            label: $label,
            help: $help,
            kind: Kind::Number {
                min: $min,
                max: $max,
                step: $step,
                unit: $unit,
            },
            live: true,
            get: |s| Value::Number(i64::from(s.filters.deshake.$field)),
            set: |s, v| {
                let mut params = s.filters.deshake;
                params.$field = <$t>::try_from(v.as_number()?).map_err(|_out_of_range| WRONG_VALUE.to_owned())?;
                s.filters.deshake = DeshakeParams::sanitized(params);
                Ok(())
            },
            enabled: always,
            indent: false,
        }
    };
}

fn get_max_shift(s: &Settings) -> Value {
    Value::Text(s.filters.deshake.max_shift.map(|v| v.to_string()).unwrap_or_default())
}

fn set_max_shift(s: &mut Settings, v: Value) -> Result<(), String> {
    s.filters.deshake.max_shift = limit(v.as_text()?, DeshakeParams::MAX_SHIFT, "px")?;
    Ok(())
}

fn get_max_angle(s: &Settings) -> Value {
    Value::Text(s.filters.deshake.max_angle.map(|v| v.to_string()).unwrap_or_default())
}

fn set_max_angle(s: &mut Settings, v: Value) -> Result<(), String> {
    let value = limit(v.as_text()?, u16::from(DeshakeParams::MAX_ANGLE), "°")?;
    s.filters.deshake.max_angle = value.and_then(|v| u8::try_from(v).ok());
    Ok(())
}

fn get_zoom_speed(s: &Settings) -> Value {
    Value::Text(hundredths(s.filters.deshake.zoom_speed))
}

fn set_zoom_speed(s: &mut Settings, v: Value) -> Result<(), String> {
    s.filters.deshake.zoom_speed = parse_hundredths(v.as_text()?, DeshakeParams::ZOOM_SPEED_MAX)
        .ok_or("Type a number from 0 to 5 (percent per frame), for example 0.25.")?;
    Ok(())
}

fn get_samples(s: &Settings) -> Value {
    Value::Number(i64::from(s.filters.crop_search.samples))
}

fn set_samples(s: &mut Settings, v: Value) -> Result<(), String> {
    s.filters.crop_search.samples = u8::try_from(v.as_number()?)
        .ok()
        .filter(|n| (1..=CropSearch::MAX_SAMPLES).contains(n))
        .ok_or_else(|| WRONG_VALUE.to_owned())?;
    Ok(())
}

pub static FILTERS: &[Card] = &[
    Card {
        title: "Deinterlace",
        description: "",
        fields: &[
            choice!(
                "filters.deinterlacer",
                "Deinterlacer",
                "bwdif: good quality at a moderate cost (recommended); yadif: faster, a little softer; w3fdif: sharp, \
                 for little motion; kerndeint: fast.",
                filters.deinterlacer,
                [
                    (Deinterlacer::Bwdif, "bwdif"),
                    (Deinterlacer::Yadif, "yadif"),
                    (Deinterlacer::W3fdif, "w3fdif"),
                    (Deinterlacer::Kerndeint, "kerndeint")
                ]
            ),
            choice!(
                "filters.deinterlace_rate",
                "Output",
                "Frames: 25i becomes 25p (recommended). Fields: 25i becomes 50p, smoother motion but twice the frames to \
                 encode. kerndeint makes frames only.",
                filters.deinterlace_rate,
                [
                    (DeinterlaceRate::Frame, "Frames"),
                    (DeinterlaceRate::Field, "Fields (2×)")
                ]
            ),
            choice!(
                "filters.field_order",
                "Field order",
                "Auto reads it from the video (recommended). Choose it only when the output jerks back and forth: \
                 the video names the wrong field. Not for kerndeint.",
                filters.field_order,
                [
                    (FieldParity::Auto, "Auto"),
                    (FieldParity::TopFirst, "Top first"),
                    (FieldParity::BottomFirst, "Bottom first")
                ]
            ),
            switch!(
                "filters.only_interlaced",
                "Only interlaced frames",
                "Leaves frames alone that the video marks as whole frames (mixed videos). Off is safer: some files \
                 mark interlaced frames as whole ones. Not for kerndeint.",
                filters.only_interlaced
            ),
        ],
    },
    Card {
        title: "Crop",
        description: "",
        fields: &[choice!(
            "filters.crop_multiple",
            "Divisible by",
            "The crop's width and height while it is dragged or found: 2 is what every encoder needs; 16 suits H.264, \
             8 HEVC and AV1 (their block sizes; no pixels coded for nothing). Typed edges stay exact.",
            filters.crop_multiple,
            [(2, "2"), (4, "4"), (8, "8"), (16, "16")]
        )],
    },
    Card {
        title: "Auto-crop",
        description: "How auto-crop looks for black borders. Each video starts with these; the Crop filter's menu \
                      changes them for one video.",
        fields: &[
            number!(
                "filters.crop_search.limit",
                "Black up to",
                "The brightest value (of 255) that still counts as border. 24 finds black bars; around 50 also dark \
                 gray ones, but it may cut into dark pictures.",
                filters.crop_search.limit: u8,
                0..=255,
                1,
                ""
            ),
            choice!(
                "filters.crop_search.sides",
                "Search",
                "Which edges auto-crop may cut: all (recommended), or only top + bottom, which keeps the sides of a dark \
                 scene.",
                filters.crop_search.sides,
                [
                    (CropSides::All, "All"),
                    (CropSides::TopBottom, "Top + bottom"),
                    (CropSides::LeftRight, "Sides")
                ]
            ),
            Field {
                key: "filters.crop_search.samples",
                label: "Sample points",
                help: "At how many points of the video auto-crop looks (at most one per two seconds). More is \
                       surer and slower; 8 suits most videos.",
                kind: Kind::Number {
                    min: 1,
                    max: 32,
                    step: 1,
                    unit: "",
                },
                live: true,
                get: get_samples,
                set: set_samples,
                enabled: always,
                indent: false,
            },
            switch!(
                "filters.crop_search.sections_only",
                "Only inside the sections",
                "Looks only at the parts you keep, so an intro outside them does not count (recommended). The whole \
                 video while there are no sections.",
                filters.crop_search.sections_only
            ),
        ],
    },
    Card {
        title: "Deshake",
        description: "What a new deshake filter starts with (vid.stab, two passes). The Filters card changes them per \
                      job.",
        fields: &[
            deshake!(
                "filters.deshake.shakiness",
                "Shakiness",
                "How shaky and quick the camera is, 1 (little) to 10 (very). Start with 5; higher is slower.",
                shakiness: u8,
                1..=10,
                1,
                ""
            ),
            deshake!(
                "filters.deshake.accuracy",
                "Accuracy",
                "Detection accuracy, from the shakiness up to 15. Higher is surer and slower; 15 is ffmpeg's default.",
                accuracy: u8,
                1..=15,
                1,
                ""
            ),
            deshake!(
                "filters.deshake.stepsize",
                "Search step",
                "In pixels. Smaller is more accurate and slower; 6 is ffmpeg's default.",
                stepsize: u8,
                1..=32,
                1,
                "px"
            ),
            deshake!(
                "filters.deshake.smoothing",
                "Smoothing",
                "Frames used to smooth the camera path. 10 suits most videos; higher is steadier but follows pans \
                 later.",
                smoothing: u16,
                0..=1000,
                5,
                "frames"
            ),
            Field {
                key: "filters.deshake.max_shift",
                label: "Max shift",
                help: "The farthest the picture may move, in pixels (0 to 500). Keeps pans and drives from being \
                       evened out; empty = no limit (recommended to start).",
                kind: Kind::Amount {
                    placeholder: "No limit",
                    unit: "px",
                },
                live: true,
                get: get_max_shift,
                set: set_max_shift,
                enabled: always,
                indent: false,
            },
            Field {
                key: "filters.deshake.max_angle",
                label: "Max angle",
                help: "The most the picture may turn, in degrees (0 to 90). 1 to 3 keeps a tilted horizon calm \
                       without wild turns; empty = no limit.",
                kind: Kind::Amount {
                    placeholder: "No limit",
                    unit: "°",
                },
                live: true,
                get: get_max_angle,
                set: set_max_angle,
                enabled: always,
                indent: false,
            },
            deshake!(
                "filters.deshake.zoom",
                "Zoom",
                "Extra zoom against the moving borders. Start with 0; a few percent hide what the automatic zoom \
                 leaves.",
                zoom: i8,
                -100..=100,
                1,
                "%"
            ),
            choice!(
                "filters.deshake.optzoom",
                "Automatic zoom",
                "None, one zoom for the whole video (recommended: borders only on very strong moves), or a zoom that \
                 follows the motion (no borders).",
                filters.deshake.optzoom,
                [(0, "None"), (1, "Static"), (2, "Adaptive")]
            ),
            Field {
                key: "filters.deshake.zoom_speed",
                label: "Zoom speed",
                help: "How fast the adaptive zoom may change, in percent per frame (0 to 5; 0.25 is ffmpeg's \
                       default). Lower is calmer, higher shows fewer borders.",
                kind: Kind::Amount {
                    placeholder: "0.25",
                    unit: "%",
                },
                live: true,
                get: get_zoom_speed,
                set: set_zoom_speed,
                enabled: |s| s.filters.deshake.optzoom == 2,
                indent: true,
            },
            choice!(
                "filters.deshake.crop",
                "Borders",
                "What shows where the stabilized picture left the frame. The previous frame hides it better; black \
                 shows it.",
                filters.deshake.crop,
                [(DeshakeCrop::Black, "Black"), (DeshakeCrop::Keep, "Previous frame")]
            ),
            choice!(
                "filters.deshake.interpolation",
                "Interpolation",
                "How moved pixels are computed. Bicubic is the sharpest (recommended) and the slowest.",
                filters.deshake.interpolation,
                [
                    (DeshakeInterpolation::No, "None"),
                    (DeshakeInterpolation::Linear, "Linear"),
                    (DeshakeInterpolation::Bilinear, "Bilinear"),
                    (DeshakeInterpolation::Bicubic, "Bicubic")
                ]
            ),
            switch!(
                "filters.deshake.tripod",
                "Virtual tripod",
                "Keeps the camera fixed on the first frame instead of smoothing its path. Only for shots that should \
                 not move at all.",
                filters.deshake.tripod
            ),
        ],
    },
    Card {
        title: "Resize",
        description: "",
        fields: &[choice!(
            "filters.resize_algorithm",
            "Scaler",
            "lanczos: the sharpest for shrinking (recommended); bicubic: ffmpeg's balanced default; spline: close to \
             lanczos; bilinear: the fastest.",
            filters.resize_algorithm,
            [
                (ResizeAlgorithm::Lanczos, "lanczos"),
                (ResizeAlgorithm::Bicubic, "bicubic"),
                (ResizeAlgorithm::Spline, "spline"),
                (ResizeAlgorithm::Bilinear, "bilinear")
            ]
        )],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_read_and_refused() {
        let mut s = Settings::default();
        set_max_angle(&mut s, Value::Text("3".into())).unwrap();
        assert_eq!(s.filters.deshake.max_angle, Some(3));
        assert!(set_samples(&mut s, Value::Number(0)).is_err());
    }
}
