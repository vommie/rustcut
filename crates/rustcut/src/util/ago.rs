//! How long ago a day was, in words ("today", "yesterday", "5 weeks ago", "3 years ago"), counted in calendar days of
//! the local time (user review 2026-10-27: after a file's "Modified" date). `timeago` and `chrono-humanize` were
//! checked: both count whole 24-hour spans and go from weeks to months after a month, so neither says "yesterday" by the
//! calendar nor "5 weeks ago".

use chrono::{Datelike as _, NaiveDate};

/// Weeks are counted up to this many days; months after it.
const WEEKS_UNTIL_DAYS: i64 = 60;

/// `then` seen from `today`; `None` for a day after today (a clock that was off).
pub fn ago(then: NaiveDate, today: NaiveDate) -> Option<String> {
    let days = today.signed_duration_since(then).num_days();
    let text = match days {
        ..0 => return None,
        0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        2..7 => format!("{days} days ago"),
        7..WEEKS_UNTIL_DAYS => plural(days / 7, "week"),
        _ => {
            let mut months = (today.year() - then.year()) * 12 + i32::try_from(today.month()).unwrap_or(0)
                - i32::try_from(then.month()).unwrap_or(0);
            if today.day() < then.day() {
                months -= 1;
            }
            let months = months.max(2);
            if months < 12 {
                plural(i64::from(months), "month")
            } else {
                plural(i64::from(months / 12), "year")
            }
        }
    };
    Some(text)
}

fn plural(count: i64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn days_weeks_months_and_years_by_the_calendar() {
        let today = day(2026, 10, 27);
        let cases = [
            (day(2026, 10, 27), "today"),
            (day(2026, 10, 26), "yesterday"),
            (day(2026, 10, 25), "2 days ago"),
            (day(2026, 10, 21), "6 days ago"),
            (day(2026, 10, 20), "1 week ago"),
            (day(2026, 10, 7), "2 weeks ago"),
            (day(2026, 9, 22), "5 weeks ago"),
            (day(2026, 8, 29), "8 weeks ago"),
            (day(2026, 8, 28), "2 months ago"),
            (day(2026, 3, 1), "7 months ago"),
            (day(2025, 10, 28), "11 months ago"),
            (day(2025, 10, 27), "1 year ago"),
            (day(2023, 5, 2), "3 years ago"),
        ];
        for (then, want) in cases {
            assert_eq!(ago(then, today).as_deref(), Some(want), "{then}");
        }
        assert_eq!(ago(day(2026, 10, 28), today), None, "a day in the future says nothing");
    }
}
