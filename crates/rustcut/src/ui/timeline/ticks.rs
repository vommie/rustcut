//! The ruler above the track: labeled major ticks at a "nice" step that leaves room for the labels, and unlabeled minor
//! ticks between them while they are far enough apart.

use az::CheckedAs as _;
use rustcut_core::time::Timestamp;

use super::viewport::Viewport;

/// Steps of the major ticks in microseconds, with the number of minor intervals per step.
const STEPS: [(i64, i64); 25] = [
    (10_000, 5),
    (20_000, 4),
    (50_000, 5),
    (100_000, 5),
    (200_000, 4),
    (500_000, 5),
    (1_000_000, 4),
    (2_000_000, 4),
    (5_000_000, 5),
    (10_000_000, 2),
    (15_000_000, 3),
    (30_000_000, 3),
    (60_000_000, 4),
    (120_000_000, 4),
    (300_000_000, 5),
    (600_000_000, 2),
    (900_000_000, 3),
    (1_800_000_000, 3),
    (3_600_000_000, 4),
    (7_200_000_000, 4),
    (10_800_000_000, 3),
    (21_600_000_000, 6),
    (43_200_000_000, 4),
    (86_400_000_000, 4),
    (172_800_000_000, 2),
];

/// Labels need this much room per major step (a label like `1:23:45` in the 10 px ruler font is about 45 px).
pub const MIN_LABEL_SPACING: f32 = 72.0;
/// Minor ticks closer than this are left out.
pub const MIN_MINOR_SPACING: f32 = 7.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub x: f32,
    /// Major ticks carry their time.
    pub label: Option<String>,
}

/// The ticks of the visible part, left to right.
pub fn ticks(vp: &Viewport) -> Vec<Tick> {
    let pps = vp.pixels_per_second();
    if vp.is_empty() || pps <= 0.0 {
        return Vec::new();
    }
    let px = |micros: i64| micros as f64 / 1e6 * pps;
    let (major, minors) = STEPS
        .iter()
        .copied()
        .find(|(step, _)| px(*step) >= f64::from(MIN_LABEL_SPACING))
        .unwrap_or((172_800_000_000, 2));
    let minor = if px(major / minors) >= f64::from(MIN_MINOR_SPACING) {
        major / minors
    } else {
        major
    };
    let (from, to) = vp.visible();
    let long = vp.duration() >= Timestamp::from_secs(3_600);
    let first = from.as_micros().div_euclid(minor);
    let last = to.as_micros().div_euclid(minor).saturating_add(1);
    (first..=last)
        .filter_map(|i| {
            let t = i.checked_mul(minor)?;
            if t < 0 || t > vp.duration().as_micros() {
                return None;
            }
            let x = vp.x_of(Timestamp::from_micros(t));
            if !(-1.0..=vp.width() + 1.0).contains(&x) {
                return None;
            }
            let label = (t % major == 0).then(|| label(t, major, long));
            Some(Tick { x, label })
        })
        .collect()
}

/// `1:02:03` for long videos, `2:03` otherwise, with as many decimals as the step needs (`2:03.5`, `2:03.25`).
fn label(micros: i64, step: i64, long: bool) -> String {
    let total = micros.div_euclid(1_000_000);
    let frac = micros.rem_euclid(1_000_000);
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    let base = if long || h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    };
    let decimals: usize = match step {
        s if s >= 1_000_000 => 0,
        s if s >= 100_000 => 1,
        _ => 2,
    };
    if decimals == 0 {
        return base;
    }
    let scale = 10_i64.pow(6 - decimals.checked_as::<u32>().unwrap_or(0));
    format!("{base}.{:0width$}", frac / scale, width = decimals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(ticks: &[Tick]) -> Vec<&str> {
        ticks.iter().filter_map(|t| t.label.as_deref()).collect()
    }

    #[test]
    fn a_minute_on_a_wide_track() {
        // 60 s on 1200 px = 20 px/s: 5 s majors (100 px) with 1 s minors (20 px).
        let vp = Viewport::new(Timestamp::from_secs(60), 1200.0, 1.0);
        let ticks = ticks(&vp);
        assert_eq!(labels(&ticks).first(), Some(&"0:00"));
        assert_eq!(labels(&ticks).get(1), Some(&"0:05"));
        assert_eq!(labels(&ticks).last(), Some(&"1:00"));
        assert_eq!(ticks.len(), 61);
        assert!(ticks.windows(2).all(|w| w[0].x < w[1].x));
    }

    #[test]
    fn long_videos_show_hours_and_zoomed_views_decimals() {
        let vp = Viewport::new(Timestamp::from_secs(7_200), 800.0, 1.0);
        let ticks = ticks(&vp);
        assert_eq!(labels(&ticks).first(), Some(&"0:00:00"));
        assert!(labels(&ticks).contains(&"1:00:00"), "{:?}", labels(&ticks));
        // Two seconds of a long video on 800 px: tenths.
        let mut vp = Viewport::new(Timestamp::from_secs(7_200), 800.0, 3_600.0);
        vp.zoom_to(3_600.0, 0.0);
        let ticks = super::ticks(&vp);
        assert!(
            labels(&ticks).contains(&"0:00:00.5") || labels(&ticks).contains(&"0:00:00.2"),
            "{:?}",
            labels(&ticks)
        );
    }

    #[test]
    fn labels_never_crowd() {
        for (duration, width, zoom) in [
            (5, 300.0, 1.0),
            (86_400, 1500.0, 1.0),
            (600, 90.0, 1.0),
            (3, 2000.0, 3.0),
        ] {
            let vp = Viewport::new(Timestamp::from_secs(duration), width, zoom);
            let majors: Vec<f32> = ticks(&vp).iter().filter(|t| t.label.is_some()).map(|t| t.x).collect();
            assert!(
                majors.windows(2).all(|w| w[1] - w[0] >= MIN_LABEL_SPACING - 0.01),
                "{duration} s on {width} px: {majors:?}"
            );
        }
    }

    #[test]
    fn label_formats() {
        assert_eq!(label(83_000_000, 1_000_000, false), "1:23");
        assert_eq!(label(83_500_000, 500_000, false), "1:23.5");
        assert_eq!(label(83_250_000, 50_000, false), "1:23.25");
        assert_eq!(label(3_723_000_000, 60_000_000, true), "1:02:03");
        assert_eq!(label(5_000_000, 5_000_000, true), "0:00:05");
    }

    #[test]
    fn nothing_without_a_duration() {
        assert!(ticks(&Viewport::new(Timestamp::ZERO, 500.0, 1.0)).is_empty());
    }
}
