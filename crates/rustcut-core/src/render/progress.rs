//! ffmpeg's `-progress pipe:1` output: blocks of `key=value` lines, each ended by `progress=continue` (or `progress=end`
//! after the last one). The parser collects a block and hands it over as one [`ProgressSample`].

use crate::time::Timestamp;

/// One block of ffmpeg's progress report. Values ffmpeg reports as `N/A` stay `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProgressSample {
    /// Output frames so far.
    pub frame: Option<u64>,
    pub fps: Option<f32>,
    /// Output time reached (never negative).
    pub out_time: Option<Timestamp>,
    /// Bytes written so far.
    pub total_size: Option<u64>,
    /// Encoding speed relative to real time (`1.5x` = 1.5).
    pub speed: Option<f32>,
    /// The last block: ffmpeg is finishing.
    pub end: bool,
}

impl ProgressSample {
    /// Whether this block shows the render moved since `earlier` (the stall watchdog's test).
    pub fn advanced_since(&self, earlier: &Self) -> bool {
        self.end
            || self.frame > earlier.frame
            || self.out_time > earlier.out_time
            || self.total_size > earlier.total_size
    }
}

/// Feeds lines in, gets finished blocks out.
#[derive(Debug, Default)]
pub struct ProgressParser {
    block: ProgressSample,
    /// `out_time_us` was seen in this block, so the older keys are not needed.
    exact_time: bool,
}

impl ProgressParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// One line of the report (with or without its line break). Returns the block when the line ends it.
    pub fn push_line(&mut self, line: &str) -> Option<ProgressSample> {
        let (key, value) = line.trim().split_once('=')?;
        let value = value.trim();
        let block = &mut self.block;
        match key.trim() {
            "frame" => block.frame = value.parse().ok(),
            "fps" => block.fps = parse_float(value),
            "total_size" => block.total_size = value.parse().ok(),
            "speed" => block.speed = parse_float(value.trim_end_matches('x')),
            "out_time_us" => {
                block.out_time = parse_micros(value);
                self.exact_time = block.out_time.is_some();
            }
            // Despite its name ffmpeg writes microseconds here too (a long-standing quirk kept for compatibility).
            "out_time_ms" if !self.exact_time => block.out_time = parse_micros(value),
            "out_time" if !self.exact_time && block.out_time.is_none() => {
                block.out_time = Timestamp::parse(value).ok().filter(|t| !t.is_negative());
            }
            "progress" => {
                block.end = value == "end";
                let done = std::mem::take(&mut self.block);
                self.exact_time = false;
                return Some(done);
            }
            _ => {}
        }
        None
    }
}

fn parse_float(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

fn parse_micros(value: &str) -> Option<Timestamp> {
    value
        .parse::<i64>()
        .ok()
        .filter(|&us| us >= 0)
        .map(Timestamp::from_micros)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "\
frame=0
fps=0.00
stream_0_0_q=0.0
bitrate=N/A
total_size=N/A
out_time_us=N/A
out_time_ms=N/A
out_time=N/A
dup_frames=0
drop_frames=0
speed=N/A
progress=continue
frame=48
fps=23.95
stream_0_0_q=31.0
bitrate= 143.2kbits/s
total_size=36123
out_time_us=2016000
out_time_ms=2016000
out_time=00:00:02.016000
dup_frames=0
drop_frames=0
speed=1.01x
progress=continue
frame=75
fps=24.10
bitrate= 152.0kbits/s
total_size=57801
out_time_us=3040000
out_time_ms=3040000
out_time=00:00:03.040000
speed=1.02x
progress=end
";

    fn samples(text: &str) -> Vec<ProgressSample> {
        let mut parser = ProgressParser::new();
        text.lines().filter_map(|l| parser.push_line(l)).collect()
    }

    #[test]
    fn blocks_of_a_real_report() {
        let s = samples(REPORT);
        assert_eq!(s.len(), 3);
        assert_eq!(
            s[0],
            ProgressSample {
                frame: Some(0),
                fps: Some(0.0),
                ..ProgressSample::default()
            }
        );
        assert_eq!(s[1].frame, Some(48));
        assert_eq!(s[1].out_time, Some(Timestamp::from_micros(2_016_000)));
        assert_eq!(s[1].total_size, Some(36_123));
        assert_eq!(s[1].speed, Some(1.01));
        assert!(!s[1].end);
        assert!(s[2].end);
        assert!(s[2].advanced_since(&s[1]) && s[1].advanced_since(&s[0]));
        assert!(!s[1].advanced_since(&s[1]));
    }

    #[test]
    fn older_keys_and_odd_values() {
        // Only the text time (older builds), a negative start time and CRLF line ends.
        let s = samples(
            "out_time=00:01:02.500000\r\nprogress=continue\r\nout_time_us=-23000\nspeed= 0.5x\nprogress=continue\n",
        );
        assert_eq!(s[0].out_time, Some(Timestamp::from_millis(62_500)));
        assert_eq!(s[1].out_time, None);
        assert_eq!(s[1].speed, Some(0.5));
        // `out_time_ms` (microseconds) when `out_time_us` is missing; an exact value wins over it.
        let s = samples("out_time_ms=1500000\nprogress=continue\nout_time_us=7\nout_time_ms=9\nprogress=continue\n");
        assert_eq!(s[0].out_time, Some(Timestamp::from_micros(1_500_000)));
        assert_eq!(s[1].out_time, Some(Timestamp::from_micros(7)));
        // Garbage is ignored.
        assert!(samples("no equals sign\n=\nfps=inf\n").is_empty());
    }
}
