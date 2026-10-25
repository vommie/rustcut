//! Time on the media timeline: integer microseconds everywhere, strings only at the UI edge (ADR-8).

mod format;
mod frame_rate;

use std::fmt;
use std::ops::{Add, Sub};
use std::str::FromStr;

use az::CheckedAs as _;
use serde::{Deserialize, Serialize};

pub use format::{ParseTimeError, Precision};
pub use frame_rate::{FrameRate, ParseFrameRateError};

/// A point or a length on the media timeline in microseconds.
///
/// `+` and `-` saturate instead of overflowing (an `i64` of microseconds covers about 292 000 years, so saturation is a
/// guard, not a case that happens). `Display` writes `H:MM:SS.mmm`; `FromStr` reads what [`Timestamp::parse`] reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(i64::MAX);
    pub const MICROS_PER_SECOND: i64 = 1_000_000;
    pub const MICROS_PER_MILLI: i64 = 1_000;

    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn from_millis(millis: i64) -> Self {
        Self(millis.saturating_mul(Self::MICROS_PER_MILLI))
    }

    pub const fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(Self::MICROS_PER_SECOND))
    }

    /// Seconds as reported by mpv and ffprobe, rounded to the nearest microsecond. `None` for NaN, infinity and values
    /// outside the `i64` range.
    pub fn from_secs_f64(secs: f64) -> Option<Self> {
        (secs * 1_000_000.0).round().checked_as::<i64>().map(Self)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Distance between two points, always positive.
    pub fn abs_diff(self, other: Self) -> Self {
        Self(i64::try_from(self.0.abs_diff(other.0)).unwrap_or(i64::MAX))
    }

    /// Parses `H:MM:SS.ffffff`, `M:SS.f` or `S.f` (see [`format::parse`]).
    pub fn parse(text: &str) -> Result<Self, ParseTimeError> {
        format::parse(text)
    }

    /// Formats as `H:MM:SS.mmm` ([`Precision::Millis`], truncated) or `H:MM:SS.ffffff` ([`Precision::Micros`], exact).
    pub fn format(self, precision: Precision) -> String {
        format::format(self, precision)
    }

    /// The compact form of a length for summaries (`05.500s`, `02:33.100`, `3:02:33.000`; see [`format::compact`]).
    pub fn format_compact(self) -> String {
        format::compact(self)
    }

    /// Seconds with six decimals, the form ffmpeg's `-ss`/`-to`/`-t` accept exactly (`12.345678`).
    pub fn to_ffmpeg_arg(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let per_second = Self::MICROS_PER_SECOND.unsigned_abs();
        format!("{sign}{}.{:06}", abs / per_second, abs % per_second)
    }
}

impl Add for Timestamp {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        self.saturating_add(rhs)
    }
}

impl Sub for Timestamp {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        self.saturating_sub(rhs)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format(Precision::Millis))
    }
}

impl FromStr for Timestamp {
    type Err = ParseTimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn constructors_agree() {
        assert_eq!(Timestamp::from_secs(2), Timestamp::from_millis(2_000));
        assert_eq!(Timestamp::from_millis(2), Timestamp::from_micros(2_000));
        assert_eq!(Timestamp::from_secs(i64::MAX), Timestamp::MAX);
    }

    #[test]
    fn from_secs_f64_rounds_and_rejects_non_finite() {
        assert_eq!(
            Timestamp::from_secs_f64(1.234_567_4),
            Some(Timestamp::from_micros(1_234_567))
        );
        assert_eq!(
            Timestamp::from_secs_f64(1.234_567_6),
            Some(Timestamp::from_micros(1_234_568))
        );
        assert_eq!(Timestamp::from_secs_f64(f64::NAN), None);
        assert_eq!(Timestamp::from_secs_f64(f64::INFINITY), None);
        assert_eq!(Timestamp::from_secs_f64(1e300), None);
    }

    #[test]
    fn arithmetic_saturates() {
        assert_eq!(Timestamp::MAX + Timestamp::from_secs(1), Timestamp::MAX);
        assert_eq!(
            Timestamp::from_micros(i64::MIN) - Timestamp::from_secs(1),
            Timestamp::from_micros(i64::MIN)
        );
        assert_eq!(Timestamp::MAX.checked_add(Timestamp::from_micros(1)), None);
        assert_eq!(
            Timestamp::from_micros(i64::MIN).abs_diff(Timestamp::MAX),
            Timestamp::MAX
        );
    }

    #[test]
    fn ffmpeg_argument_is_exact() {
        assert_eq!(Timestamp::from_micros(12_345_678).to_ffmpeg_arg(), "12.345678");
        assert_eq!(Timestamp::ZERO.to_ffmpeg_arg(), "0.000000");
        assert_eq!(Timestamp::from_micros(-500_000).to_ffmpeg_arg(), "-0.500000");
        assert_eq!(Timestamp::from_secs(90_000).to_ffmpeg_arg(), "90000.000000");
    }

    #[test]
    fn serializes_as_plain_micros() {
        let json = serde_json::to_string(&Timestamp::from_micros(1_500_000)).unwrap();
        assert_eq!(json, "1500000");
        assert_eq!(
            serde_json::from_str::<Timestamp>("42").unwrap(),
            Timestamp::from_micros(42)
        );
    }

    proptest! {
        #[test]
        fn add_then_sub_is_identity_within_range(a in -1_000_000_000_000i64..1_000_000_000_000, b in -1_000_000_000_000i64..1_000_000_000_000) {
            let (a, b) = (Timestamp::from_micros(a), Timestamp::from_micros(b));
            prop_assert_eq!(a + b - b, a);
        }

        #[test]
        fn ffmpeg_argument_parses_back(us in 0i64..i64::MAX / 2) {
            let t = Timestamp::from_micros(us);
            prop_assert_eq!(Timestamp::parse(&t.to_ffmpeg_arg()).unwrap(), t);
        }
    }
}
