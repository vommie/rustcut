//! Frame spacing from packet timestamps: the probe reads the timestamps of the first seconds of the primary video stream,
//! and this module decides whether the frames are evenly spaced (constant frame rate) or not. Matroska stores no frame
//! count, so ffprobe's average rate equals the base rate there and only the timestamps reveal a variable frame rate.

use super::model::FrameTiming;

/// Fewer frames than this say nothing about the spacing.
const MIN_FRAMES: usize = 8;
/// A gap counts as different when it is this much longer or shorter than the usual gap...
const RELATIVE_TOLERANCE: f64 = 0.10;
/// ...and at least this many seconds off (Matroska rounds timestamps to milliseconds: 29.97 fps alternates 33 and 34 ms).
const ABSOLUTE_TOLERANCE_SECS: f64 = 0.0015;
/// A cut or a dropped frame or two is not a variable frame rate.
const STRAY_GAPS: usize = 2;

/// The spacing of `pts` (seconds, in any order; B-frames arrive out of order). `None` for too few distinct timestamps.
pub fn analyze(pts: &[f64]) -> Option<FrameTiming> {
    let mut sorted: Vec<f64> = pts.iter().copied().filter(|t| t.is_finite()).collect();
    sorted.sort_by(f64::total_cmp);
    sorted.dedup();
    if sorted.len() < MIN_FRAMES {
        return None;
    }
    let mut gaps: Vec<f64> = sorted
        .windows(2)
        .filter_map(|pair| match pair {
            [a, b] => Some(b - a),
            _ => None,
        })
        .collect();
    gaps.sort_by(f64::total_cmp);
    let usual = *gaps.get(gaps.len() / 2)?;
    let (shortest, longest) = (*gaps.first()?, *gaps.last()?);
    if usual <= 0.0 || shortest <= 0.0 {
        return None;
    }
    let tolerance = (usual * RELATIVE_TOLERANCE).max(ABSOLUTE_TOLERANCE_SECS);
    let odd = gaps.iter().filter(|gap| (**gap - usual).abs() > tolerance).count();
    let span_secs = sorted.last()? - sorted.first()?;
    Some(FrameTiming {
        frames: sorted.len(),
        span_secs,
        min_fps: 1.0 / longest,
        max_fps: 1.0 / shortest,
        variable: odd > STRAY_GAPS.max(gaps.len() / 50),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn even(count: usize, step: f64) -> Vec<f64> {
        (0..count).map(|i| i as f64 * step).collect()
    }

    #[test]
    fn evenly_spaced_frames_are_constant() {
        let t = analyze(&even(250, 0.04)).unwrap();
        assert!(!t.variable);
        assert_eq!(t.frames, 250);
        assert!((t.min_fps - 25.0).abs() < 1e-6 && (t.max_fps - 25.0).abs() < 1e-6);
        assert!((t.span_secs - 9.96).abs() < 1e-9);
    }

    #[test]
    fn millisecond_rounding_is_not_variable() {
        // 29.97 fps in a 1/1000 time base: gaps of 33 and 34 ms.
        let pts: Vec<f64> = (0..300)
            .map(|i| (f64::from(i) * 1001.0 / 30.0).round() / 1000.0)
            .collect();
        assert!(!analyze(&pts).unwrap().variable);
    }

    #[test]
    fn out_of_order_timestamps_are_sorted() {
        let mut pts = even(100, 0.04);
        pts.swap(3, 5);
        pts.swap(10, 11);
        assert!(!analyze(&pts).unwrap().variable);
    }

    #[test]
    fn a_stray_gap_is_tolerated_and_a_changing_rate_is_variable() {
        let mut pts = even(100, 0.04);
        pts.retain(|t| (*t - 2.0).abs() > 1e-9);
        assert!(!analyze(&pts).unwrap().variable);
        // 30 fps for two seconds, then every third frame (10 fps): what a screen recorder writes.
        let mut pts = even(60, 1.0 / 30.0);
        pts.extend((0..20).map(|i| 2.0 + f64::from(i) * 0.1));
        let t = analyze(&pts).unwrap();
        assert!(t.variable);
        assert!((t.min_fps - 10.0).abs() < 1e-6);
        assert!((t.max_fps - 30.0).abs() < 1e-6);
    }

    #[test]
    fn too_few_or_broken_timestamps_say_nothing() {
        assert_eq!(analyze(&even(5, 0.04)), None);
        assert_eq!(analyze(&[0.0; 20]), None);
        assert_eq!(analyze(&[f64::NAN; 20]), None);
        assert_eq!(analyze(&[]), None);
    }
}
