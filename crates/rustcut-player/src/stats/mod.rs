//! Frame-time statistics of the video surface (M1.1 measurement, kept as a diagnostic in the log).
//!
//! Each video frame contributes one [`FrameSample`]: how long mpv's render call took, how long the whole Slint frame took
//! (from Slint's "before rendering" to "after rendering"), and when the frame was drawn, which gives the frame interval and
//! from it the frame rate that reached the screen. Values are kept in microsecond histograms (`hdrhistogram`).

use std::time::{Duration, Instant};

use hdrhistogram::Histogram;

/// Highest value the histograms track exactly (60 s); longer values are clamped to it.
const MAX_MICROS: u64 = 60_000_000;
/// A gap between two video frames longer than this is a pause or a seek, not a slow frame, and is not counted.
const MAX_INTERVAL: Duration = Duration::from_secs(1);
/// Significant decimal digits of the histograms (three: 1 µs resolution up to 1 ms, then 0.1 %).
const SIGNIFICANT_DIGITS: u8 = 3;

/// One video frame as the surface saw it.
#[derive(Debug, Clone, Copy)]
pub struct FrameSample {
    /// Time of mpv's `render` call (CPU side; with software GL this includes most of the drawing).
    pub render: Duration,
    /// Time of the whole Slint frame that showed the video frame, without the buffer swap.
    pub frame: Duration,
    /// When the frame was drawn.
    pub at: Instant,
}

/// Summary of the samples since the statistics were created or reset.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    /// Number of video frames.
    pub frames: u64,
    /// Mean, 95th percentile and maximum of mpv's render call in milliseconds.
    pub render_ms: Spread,
    /// Mean, 95th percentile and maximum of the whole Slint frame in milliseconds.
    pub frame_ms: Spread,
    /// Mean, 95th percentile and maximum of the time between two video frames in milliseconds.
    pub interval_ms: Spread,
    /// Video frames per second that reached the screen (from the mean interval); 0 with fewer than two frames.
    pub fps: f64,
}

/// Mean, 95th percentile and maximum of a series, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spread {
    pub mean: f64,
    pub p95: f64,
    pub max: f64,
}

impl std::fmt::Display for Spread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2}/{:.2}/{:.2} ms", self.mean, self.p95, self.max)
    }
}

/// Collects [`FrameSample`]s.
pub struct FrameStats {
    render: Histogram<u64>,
    frame: Histogram<u64>,
    interval: Histogram<u64>,
    last_at: Option<Instant>,
}

impl FrameStats {
    /// Empty statistics; `None` if the histograms cannot be created (only with invalid constants).
    pub fn new() -> Option<Self> {
        let make = || Histogram::new_with_bounds(1, MAX_MICROS, SIGNIFICANT_DIGITS).ok();
        Some(Self {
            render: make()?,
            frame: make()?,
            interval: make()?,
            last_at: None,
        })
    }

    /// Adds one video frame.
    pub fn record(&mut self, sample: FrameSample) {
        self.render.saturating_record(micros(sample.render));
        self.frame.saturating_record(micros(sample.frame));
        if let Some(last) = self.last_at {
            let gap = sample.at.saturating_duration_since(last);
            if gap <= MAX_INTERVAL {
                self.interval.saturating_record(micros(gap));
            }
        }
        self.last_at = Some(sample.at);
    }

    /// Number of recorded video frames.
    pub fn frames(&self) -> u64 {
        self.render.len()
    }

    /// The summary so far; `None` without any frame.
    pub fn report(&self) -> Option<FrameReport> {
        if self.render.is_empty() {
            return None;
        }
        let interval_ms = spread(&self.interval);
        Some(FrameReport {
            frames: self.render.len(),
            render_ms: spread(&self.render),
            frame_ms: spread(&self.frame),
            interval_ms,
            fps: if interval_ms.mean > 0.0 {
                1000.0 / interval_ms.mean
            } else {
                0.0
            },
        })
    }

    /// Starts a new measuring period; the next frame has no interval (a pause is not a slow frame).
    pub fn reset(&mut self) {
        self.render.reset();
        self.frame.reset();
        self.interval.reset();
        self.last_at = None;
    }

    /// Forgets the time of the last frame, so a pause or seek does not count as one long interval.
    pub fn break_interval(&mut self) {
        self.last_at = None;
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros())
        .unwrap_or(MAX_MICROS)
        .clamp(1, MAX_MICROS)
}

fn spread(histogram: &Histogram<u64>) -> Spread {
    if histogram.is_empty() {
        return Spread::default();
    }
    let ms = |micros: u64| micros as f64 / 1000.0;
    Spread {
        mean: histogram.mean() / 1000.0,
        p95: ms(histogram.value_at_quantile(0.95)),
        max: ms(histogram.max()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(render_ms: u64, frame_ms: u64, at: Instant) -> FrameSample {
        FrameSample {
            render: Duration::from_millis(render_ms),
            frame: Duration::from_millis(frame_ms),
            at,
        }
    }

    #[test]
    fn empty_statistics_have_no_report() {
        let stats = FrameStats::new().unwrap();
        assert_eq!(stats.frames(), 0);
        assert!(stats.report().is_none());
    }

    #[test]
    fn steady_frames_give_their_rate() {
        let mut stats = FrameStats::new().unwrap();
        let start = Instant::now();
        for i in 0..25 {
            stats.record(sample(4, 10, start + Duration::from_millis(40 * i)));
        }
        let report = stats.report().unwrap();
        assert_eq!(report.frames, 25);
        assert!((report.render_ms.mean - 4.0).abs() < 0.01, "{report:?}");
        assert!((report.frame_ms.max - 10.0).abs() < 0.02, "{report:?}");
        assert!((report.interval_ms.mean - 40.0).abs() < 0.05, "{report:?}");
        assert!((report.fps - 25.0).abs() < 0.05, "{report:?}");
    }

    #[test]
    fn the_95th_percentile_ignores_a_single_spike() {
        let mut stats = FrameStats::new().unwrap();
        let start = Instant::now();
        for i in 0..99 {
            stats.record(sample(2, 5, start + Duration::from_millis(16 * i)));
        }
        stats.record(sample(80, 90, start + Duration::from_millis(16 * 99)));
        let report = stats.report().unwrap();
        assert!(report.render_ms.p95 < 2.1, "{report:?}");
        assert!(report.render_ms.max > 79.0, "{report:?}");
    }

    #[test]
    fn a_break_does_not_count_as_a_long_interval() {
        let mut stats = FrameStats::new().unwrap();
        let start = Instant::now();
        stats.record(sample(1, 1, start));
        stats.record(sample(1, 1, start + Duration::from_millis(20)));
        stats.break_interval();
        stats.record(sample(1, 1, start + Duration::from_millis(500)));
        let report = stats.report().unwrap();
        assert!((report.interval_ms.max - 20.0).abs() < 0.05, "{report:?}");
    }

    #[test]
    fn a_pause_does_not_count_as_a_long_interval() {
        let mut stats = FrameStats::new().unwrap();
        let start = Instant::now();
        stats.record(sample(1, 1, start));
        stats.record(sample(1, 1, start + Duration::from_millis(40)));
        stats.record(sample(1, 1, start + Duration::from_secs(10)));
        stats.record(sample(1, 1, start + Duration::from_millis(10_040)));
        let report = stats.report().unwrap();
        assert!((report.interval_ms.max - 40.0).abs() < 0.05, "{report:?}");
        assert!((report.fps - 25.0).abs() < 0.05, "{report:?}");
    }

    #[test]
    fn reset_starts_a_new_period() {
        let mut stats = FrameStats::new().unwrap();
        let start = Instant::now();
        stats.record(sample(1, 1, start));
        stats.reset();
        assert!(stats.report().is_none());
        stats.record(sample(3, 3, start + Duration::from_secs(5)));
        let report = stats.report().unwrap();
        assert_eq!(report.frames, 1);
        assert!(report.fps.abs() < f64::EPSILON, "{report:?}");
    }

    #[test]
    fn huge_and_zero_durations_are_clamped() {
        assert_eq!(micros(Duration::ZERO), 1);
        assert_eq!(micros(Duration::from_secs(3600)), MAX_MICROS);
    }
}
