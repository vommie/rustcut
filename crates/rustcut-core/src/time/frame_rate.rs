//! Frame rates as exact fractions (`30000/1001`) and frame-accurate positions on the timeline.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::Timestamp;

const MICROS: i128 = 1_000_000;

/// Frames per second as `num / den`, both positive (ffprobe's `r_frame_rate`/`avg_frame_rate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate {
    num: u32,
    den: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseFrameRateError {
    #[error("`{0}` is not a frame rate (expected `30000/1001`, `25` or `29.97`)")]
    Invalid(String),
    #[error("the frame rate is zero or undefined")]
    Undefined,
}

impl FrameRate {
    /// `None` when either part is zero (ffprobe reports `0/0` for streams without a rate).
    pub const fn new(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            None
        } else {
            Some(Self { num, den })
        }
    }

    pub const fn num(self) -> u32 {
        self.num
    }

    pub const fn den(self) -> u32 {
        self.den
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Length of one frame, rounded up to the next microsecond.
    pub fn frame_duration(self) -> Timestamp {
        self.frame_start(1)
    }

    /// Index of the frame shown at `t` (frame `i` covers `[frame_start(i), frame_start(i + 1))`); negative before zero.
    pub fn frame_index_at(self, t: Timestamp) -> i64 {
        let frames = i128::from(t.as_micros()) * i128::from(self.num);
        let per = MICROS * i128::from(self.den);
        i64::try_from(frames.div_euclid(per)).unwrap_or(if frames < 0 { i64::MIN } else { i64::MAX })
    }

    /// First microsecond of frame `index` (rounded up, so that `frame_index_at(frame_start(i)) == i`).
    pub fn frame_start(self, index: i64) -> Timestamp {
        let num = i128::from(index) * MICROS * i128::from(self.den);
        let den = i128::from(self.num);
        let q = num.div_euclid(den) + i128::from(num.rem_euclid(den) != 0);
        Timestamp::from_micros(i64::try_from(q).unwrap_or(if q < 0 { i64::MIN } else { i64::MAX }))
    }

    /// The start of the frame shown at `t` (snaps down to the frame grid).
    pub fn snap(self, t: Timestamp) -> Timestamp {
        self.frame_start(self.frame_index_at(t))
    }

    /// The start of the frame `frames` frames after (or, negative, before) the one shown at `t`.
    pub fn step(self, t: Timestamp, frames: i64) -> Timestamp {
        self.frame_start(self.frame_index_at(t).saturating_add(frames))
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

impl FromStr for FrameRate {
    type Err = ParseFrameRateError;

    /// Reads `num/den` (ffprobe), an integer (`25`) or a decimal (`29.97`, kept exact as `2997/100`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let invalid = || ParseFrameRateError::Invalid(s.to_owned());
        let (num, den) = if let Some((n, d)) = s.split_once('/') {
            (
                n.trim().parse::<u32>().ok().ok_or_else(invalid)?,
                d.trim().parse::<u32>().ok().ok_or_else(invalid)?,
            )
        } else if let Some((whole, frac)) = s.split_once('.') {
            if frac.is_empty() || frac.len() > 6 || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            let scale = 10u32.pow(u32::try_from(frac.len()).ok().ok_or_else(invalid)?);
            let whole = if whole.is_empty() {
                0
            } else {
                whole.parse::<u32>().ok().ok_or_else(invalid)?
            };
            let frac = frac.parse::<u32>().ok().ok_or_else(invalid)?;
            let num = whole
                .checked_mul(scale)
                .and_then(|w| w.checked_add(frac))
                .ok_or_else(invalid)?;
            (num, scale)
        } else {
            (s.parse::<u32>().ok().ok_or_else(invalid)?, 1)
        };
        Self::new(num, den).ok_or(ParseFrameRateError::Undefined)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn ntsc() -> FrameRate {
        FrameRate::new(30_000, 1_001).unwrap()
    }

    #[test]
    fn parses_ffprobe_forms() {
        assert_eq!("30000/1001".parse::<FrameRate>().unwrap(), ntsc());
        assert_eq!("25".parse::<FrameRate>().unwrap(), FrameRate::new(25, 1).unwrap());
        assert_eq!(
            "29.97".parse::<FrameRate>().unwrap(),
            FrameRate::new(2_997, 100).unwrap()
        );
        assert_eq!("0/0".parse::<FrameRate>(), Err(ParseFrameRateError::Undefined));
        assert!(matches!(
            "abc".parse::<FrameRate>(),
            Err(ParseFrameRateError::Invalid(_))
        ));
        assert!(matches!(
            "1.".parse::<FrameRate>(),
            Err(ParseFrameRateError::Invalid(_))
        ));
        assert_eq!(ntsc().to_string(), "30000/1001");
    }

    #[test]
    fn frame_grid_of_ntsc() {
        let r = ntsc();
        assert_eq!(r.frame_duration(), Timestamp::from_micros(33_367));
        assert_eq!(r.frame_start(30_000), Timestamp::from_secs(1_001));
        assert_eq!(r.frame_index_at(Timestamp::from_secs(1_001)), 30_000);
        assert_eq!(r.frame_index_at(Timestamp::from_micros(33_366)), 0);
        assert_eq!(r.frame_index_at(Timestamp::from_micros(33_367)), 1);
        assert_eq!(r.snap(Timestamp::from_micros(50_000)), Timestamp::from_micros(33_367));
        assert_eq!(r.step(Timestamp::from_micros(50_000), -1), Timestamp::ZERO);
        assert_eq!(r.frame_index_at(Timestamp::from_micros(-1)), -1);
    }

    proptest! {
        #[test]
        fn frame_start_is_inside_its_frame(num in 1u32..240_000, den in 1u32..1_001, index in -10_000_000i64..10_000_000) {
            let r = FrameRate::new(num, den).unwrap();
            // Rates above 1 MHz have frames shorter than a microsecond; they do not exist in video.
            prop_assume!(r.as_f64() < 1_000.0);
            prop_assert_eq!(r.frame_index_at(r.frame_start(index)), index);
        }

        #[test]
        fn snap_is_idempotent_and_not_later(num in 1u32..240_000, den in 1u32..1_001, us in -1_000_000_000_000i64..1_000_000_000_000) {
            let r = FrameRate::new(num, den).unwrap();
            prop_assume!(r.as_f64() < 1_000.0);
            let t = Timestamp::from_micros(us);
            let s = r.snap(t);
            prop_assert!(s <= t);
            prop_assert_eq!(r.snap(s), s);
        }
    }
}
