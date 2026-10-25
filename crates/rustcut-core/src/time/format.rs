//! Text form of [`Timestamp`]: `H:MM:SS.mmm` for display, exact microseconds when needed, the compact form of lengths
//! (`05.500s`, `02:33.100`, `3:02:33.000`), and a forgiving parser for what users type into section fields (hours are
//! unbounded: PyCutRv's `%H:%M:%S.%f` broke at 24 h).

use super::Timestamp;

/// How many fraction digits [`Timestamp::format`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Precision {
    /// `H:MM:SS.mmm`, truncated toward zero (a position never shows later than it is).
    #[default]
    Millis,
    /// `H:MM:SS.ffffff`, exact.
    Micros,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseTimeError {
    #[error("the time is empty")]
    Empty,
    #[error("`{0}` is not a number")]
    InvalidNumber(String),
    #[error("minutes and seconds must be below 60, found {0}")]
    OutOfRange(u64),
    #[error("a time has at most three parts (H:MM:SS)")]
    TooManyParts,
    #[error("the time is too large")]
    Overflow,
}

pub(super) fn format(t: Timestamp, precision: Precision) -> String {
    let sign = if t.is_negative() { "-" } else { "" };
    let us = t.as_micros().unsigned_abs();
    let total_secs = us / 1_000_000;
    let frac = us % 1_000_000;
    let (h, m, s) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
    match precision {
        Precision::Millis => format!("{sign}{h}:{m:02}:{s:02}.{:03}", frac / 1_000),
        Precision::Micros => format!("{sign}{h}:{m:02}:{s:02}.{frac:06}"),
    }
}

/// The compact form (user decision Q-P9): only the units a length needs, but never a width that changes within a unit
/// range: always three decimals, seconds and minutes always two digits, hours one digit or more. Below a minute `SS.mmm`
/// with an `s` (`05.500s`), below an hour `MM:SS.mmm` (`02:33.100`), else `H:MM:SS.mmm` (`3:02:33.000`); truncated toward
/// zero like [`Precision::Millis`].
pub(super) fn compact(t: Timestamp) -> String {
    let sign = if t.is_negative() { "-" } else { "" };
    let us = t.as_micros().unsigned_abs();
    let total_secs = us / 1_000_000;
    let millis = (us % 1_000_000) / 1_000;
    let (h, m, s) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
    if h > 0 {
        format!("{sign}{h}:{m:02}:{s:02}.{millis:03}")
    } else if m > 0 {
        format!("{sign}{m:02}:{s:02}.{millis:03}")
    } else {
        format!("{sign}{s:02}.{millis:03}s")
    }
}

/// Reads `H:MM:SS[.f]`, `M:SS[.f]` or `S[.f]`. The leading part is unbounded (`90` = 90 s, `100:00:00` = 100 h); later parts
/// must be below 60. The fraction may use `.` or `,` and has any number of digits (rounded to microseconds, half up).
/// Surrounding whitespace is ignored; negative times are not accepted.
pub(super) fn parse(text: &str) -> Result<Timestamp, ParseTimeError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ParseTimeError::Empty);
    }
    let (whole, fraction) = match text.find(['.', ',']) {
        Some(pos) => (text.get(..pos).unwrap_or(""), text.get(pos + 1..)),
        None => (text, None),
    };
    let parts: Vec<&str> = whole.split(':').collect();
    if parts.len() > 3 {
        return Err(ParseTimeError::TooManyParts);
    }
    let mut secs: u64 = 0;
    for (i, part) in parts.iter().enumerate() {
        let value = parse_digits(part)?;
        if i > 0 && value >= 60 {
            return Err(ParseTimeError::OutOfRange(value));
        }
        secs = secs
            .checked_mul(60)
            .and_then(|s| s.checked_add(value))
            .ok_or(ParseTimeError::Overflow)?;
    }
    let micros = match fraction {
        Some(f) => parse_fraction(f)?,
        None => 0,
    };
    let total = secs
        .checked_mul(1_000_000)
        .and_then(|s| s.checked_add(micros))
        .ok_or(ParseTimeError::Overflow)?;
    i64::try_from(total)
        .ok()
        .map(Timestamp::from_micros)
        .ok_or(ParseTimeError::Overflow)
}

fn parse_digits(part: &str) -> Result<u64, ParseTimeError> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseTimeError::InvalidNumber(part.to_owned()));
    }
    part.parse::<u64>().ok().ok_or(ParseTimeError::Overflow)
}

/// Fraction digits to microseconds, rounding half up on the seventh digit (`"9999995"` = 1 000 000).
fn parse_fraction(digits: &str) -> Result<u64, ParseTimeError> {
    if digits.is_empty() {
        return Ok(0);
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseTimeError::InvalidNumber(digits.to_owned()));
    }
    let mut micros: u64 = 0;
    let mut bytes = digits.bytes();
    for _ in 0..6 {
        let d = bytes.next().map_or(0, |b| u64::from(b - b'0'));
        micros = micros * 10 + d;
    }
    if bytes.next().is_some_and(|b| b >= b'5') {
        micros += 1;
    }
    Ok(micros)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn t(us: i64) -> Timestamp {
        Timestamp::from_micros(us)
    }

    #[test]
    fn formats_like_pycutrv() {
        assert_eq!(t(0).to_string(), "0:00:00.000");
        assert_eq!(t(3_723_456_789).to_string(), "1:02:03.456");
        assert_eq!(t(1_999_999).to_string(), "0:00:01.999");
        assert_eq!(t(-1_500_000).to_string(), "-0:00:01.500");
        assert_eq!(t(3_723_456_789).format(Precision::Micros), "1:02:03.456789");
    }

    #[test]
    fn compact_keeps_its_width_in_each_range() {
        assert_eq!(compact(t(0)), "00.000s");
        assert_eq!(compact(t(5_500_000)), "05.500s");
        assert_eq!(compact(t(10_500_000)), "10.500s");
        assert_eq!(compact(t(59_999_999)), "59.999s");
        assert_eq!(compact(t(60_000_000)), "01:00.000");
        assert_eq!(compact(t(153_100_000)), "02:33.100");
        assert_eq!(compact(t(3_599_999_999)), "59:59.999");
        assert_eq!(compact(t(10_953_000_000)), "3:02:33.000");
        assert_eq!(compact(Timestamp::from_secs(100 * 3600)), "100:00:00.000");
        assert_eq!(compact(t(-1_500_000)), "-01.500s");
    }

    #[test]
    fn hours_beyond_a_day_work() {
        let long = Timestamp::from_secs(100 * 3600);
        assert_eq!(long.to_string(), "100:00:00.000");
        assert_eq!(Timestamp::parse("100:00:00.000").unwrap(), long);
    }

    #[test]
    fn parses_the_short_forms() {
        assert_eq!(Timestamp::parse("90").unwrap(), Timestamp::from_secs(90));
        assert_eq!(Timestamp::parse("1:30").unwrap(), Timestamp::from_secs(90));
        assert_eq!(Timestamp::parse(" 0:01:30.5 ").unwrap(), t(90_500_000));
        assert_eq!(Timestamp::parse("1,25").unwrap(), t(1_250_000));
        assert_eq!(Timestamp::parse("2.").unwrap(), Timestamp::from_secs(2));
    }

    #[test]
    fn rounds_extra_fraction_digits_half_up() {
        assert_eq!(Timestamp::parse("0.0000014").unwrap(), t(1));
        assert_eq!(Timestamp::parse("0.0000015").unwrap(), t(2));
        assert_eq!(Timestamp::parse("0.9999995").unwrap(), t(1_000_000));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Timestamp::parse(""), Err(ParseTimeError::Empty));
        assert_eq!(Timestamp::parse("1:60"), Err(ParseTimeError::OutOfRange(60)));
        assert_eq!(Timestamp::parse("1:2:3:4"), Err(ParseTimeError::TooManyParts));
        assert!(matches!(Timestamp::parse("-1"), Err(ParseTimeError::InvalidNumber(_))));
        assert!(matches!(Timestamp::parse("1:x"), Err(ParseTimeError::InvalidNumber(_))));
        assert!(matches!(
            Timestamp::parse("1::2"),
            Err(ParseTimeError::InvalidNumber(_))
        ));
        assert!(matches!(
            Timestamp::parse("1.2.3"),
            Err(ParseTimeError::InvalidNumber(_))
        ));
        assert_eq!(Timestamp::parse("99999999999999999999"), Err(ParseTimeError::Overflow));
        assert_eq!(Timestamp::parse("9223372036854:00:00"), Err(ParseTimeError::Overflow));
    }

    proptest! {
        #[test]
        fn micros_format_round_trips(us in 0i64..i64::MAX) {
            let ts = t(us);
            prop_assert_eq!(Timestamp::parse(&ts.format(Precision::Micros)).unwrap(), ts);
        }

        #[test]
        fn millis_format_truncates_to_the_millisecond(us in 0i64..i64::MAX) {
            let ts = t(us);
            prop_assert_eq!(Timestamp::parse(&ts.to_string()).unwrap(), t(us - us % 1_000));
        }

        #[test]
        fn compact_has_one_width_per_range(a in 0i64..60_000_000, b in 0i64..60_000_000, m in 1i64..60, h in 1i64..10) {
            prop_assert_eq!(compact(t(a)).len(), compact(t(b)).len());
            let in_minutes = |us: i64| compact(t(m * 60_000_000 + us % 60_000_000)).len();
            prop_assert_eq!(in_minutes(a), in_minutes(b));
            let in_hours = |us: i64| compact(t(h * 3_600_000_000 + us)).len();
            prop_assert_eq!(in_hours(a), in_hours(b));
        }

        #[test]
        fn compact_parses_back_to_the_millisecond(us in 0i64..360_000_000_000) {
            let text = compact(t(us));
            prop_assert_eq!(Timestamp::parse(text.trim_end_matches('s')).unwrap(), t(us - us % 1_000));
        }

        #[test]
        fn parser_never_panics(text in "\\PC{0,24}") {
            let _ = Timestamp::parse(&text);
        }
    }
}
