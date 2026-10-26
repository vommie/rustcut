//! Amounts typed as text, shared by Settings > Filters and the filters' options menus (P9.13): a limit that may be empty
//! ("no limit") and hundredths written as a decimal ("0.25").

/// A limit typed as text: empty = no limit, else a whole number up to `max` (with or without its `unit`).
pub fn limit(text: &str, max: u16, unit: &str) -> Result<Option<u16>, String> {
    let text = text.trim();
    let text = text.strip_suffix(unit).unwrap_or(text).trim_end();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u16>()
        .ok()
        .filter(|v| *v <= max)
        .map(Some)
        .ok_or_else(|| format!("Type a whole number from 0 to {max}, or leave it empty for no limit."))
}

/// Hundredths as a decimal ("0.25").
pub fn hundredths(value: u16) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}

/// A decimal with at most two places in `0..=max` hundredths ("0.5" = 50).
pub fn parse_hundredths(text: &str, max: u16) -> Option<u16> {
    let text = text.trim();
    let text = text.strip_suffix('%').unwrap_or(text).trim_end().replace(',', ".");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    if fraction.len() > 2 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: u16 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let fraction: u16 = format!("{fraction:0<2}").parse().ok()?;
    whole.checked_mul(100)?.checked_add(fraction).filter(|v| *v <= max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_and_hundredths_are_read_and_refused() {
        assert_eq!(limit("", 500, "px"), Ok(None));
        assert_eq!(limit(" 40 ", 500, "px"), Ok(Some(40)));
        assert_eq!(limit("40 px", 500, "px"), Ok(Some(40)));
        assert!(limit("501", 500, "px").is_err() && limit("-1", 500, "px").is_err());
        assert_eq!(parse_hundredths("0.5 %", 500), Some(50));
        assert_eq!(parse_hundredths("0.25", 500), Some(25));
        assert_eq!(parse_hundredths("1,5", 500), Some(150));
        assert_eq!(parse_hundredths(".5", 500), Some(50));
        assert_eq!(parse_hundredths("5", 500), Some(500));
        assert_eq!(parse_hundredths("5.01", 500), None);
        assert_eq!(parse_hundredths("0.125", 500), None);
        assert_eq!(hundredths(25), "0.25");
    }
}
