//! `#rrggbb` color strings: the form colors are stored in the settings and typed into the color picker (taken over from
//! RustView's `rustview_core::util::hex_color` with the picker, P8).

/// Parses `rgb` or `rrggbb` (the leading `#` is optional, case does not matter). `None` for anything else.
pub fn parse_rgb(text: &str) -> Option<[u8; 3]> {
    let digits = digits(text).filter(|d| matches!(d.len(), 3 | 6))?;
    let [r, g, b, _] = parse_digits(digits)?;
    Some([r, g, b])
}

/// Parses `rgb`, `rgba`, `rrggbb` or `rrggbbaa` (the leading `#` is optional, case does not matter). A missing alpha is
/// opaque.
pub fn parse_rgba(text: &str) -> Option<[u8; 4]> {
    parse_digits(digits(text)?)
}

fn parse_digits(digits: &[u8]) -> Option<[u8; 4]> {
    let nibble = |c: u8| char::from(c).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let pair = |hi: u8, lo: u8| Some(nibble(hi)? * 16 + nibble(lo)?);
    match *digits {
        [r, g, b] => Some([pair(r, r)?, pair(g, g)?, pair(b, b)?, 255]),
        [r, g, b, a] => Some([pair(r, r)?, pair(g, g)?, pair(b, b)?, pair(a, a)?]),
        [r1, r0, g1, g0, b1, b0] => Some([pair(r1, r0)?, pair(g1, g0)?, pair(b1, b0)?, 255]),
        [r1, r0, g1, g0, b1, b0, a1, a0] => Some([pair(r1, r0)?, pair(g1, g0)?, pair(b1, b0)?, pair(a1, a0)?]),
        _ => None,
    }
}

/// The hex digits of `text` without `#` (only if all of them are hex digits).
fn digits(text: &str) -> Option<&[u8]> {
    let bytes = text.trim().trim_start_matches('#').as_bytes();
    bytes.iter().all(u8::is_ascii_hexdigit).then_some(bytes)
}

pub fn format_rgb([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn format_rgba([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accepted_forms() {
        assert_eq!(parse_rgb("#444444"), Some([0x44, 0x44, 0x44]));
        assert_eq!(parse_rgb("4A4"), Some([0x44, 0xaa, 0x44]));
        assert_eq!(parse_rgb(" #FfA0b1 "), Some([0xff, 0xa0, 0xb1]));
        assert_eq!(parse_rgba("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(parse_rgba("#123"), Some([0x11, 0x22, 0x33, 0xff]));
        assert_eq!(parse_rgba("#1234"), Some([0x11, 0x22, 0x33, 0x44]));
    }

    #[test]
    fn everything_else_is_refused() {
        for text in [
            "",
            "#",
            "black",
            "#12",
            "#12345",
            "#1234567",
            "#gg0000",
            "#11223344",
            "#+12",
            "#ä12",
        ] {
            assert_eq!(parse_rgb(text), None, "{text}");
        }
        assert_eq!(parse_rgba("#123456789"), None);
    }

    #[test]
    fn formats_round_trip() {
        assert_eq!(format_rgb([0x44, 0x44, 0x44]), "#444444");
        assert_eq!(format_rgba([1, 2, 255, 0]), "#0102ff00");
        assert_eq!(parse_rgb(&format_rgb([7, 200, 13])), Some([7, 200, 13]));
    }
}
