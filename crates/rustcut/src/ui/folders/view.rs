//! What a folder picker shows of an entry (P11): its picture (ready or requested), the first letter without one, and
//! its color.

use rustcut_core::folders::EntryRef;

use super::pictures::Pictures;
use crate::FolderPictureView;
use crate::util::hex_color::parse_rgb;

/// The first letter of `name`, upper case ("?" for an empty name).
pub fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// A color as Slint takes it, from `#rrggbb`.
pub fn color(text: Option<&str>) -> Option<slint::Color> {
    let [r, g, b] = parse_rgb(text?)?;
    Some(slint::Color::from_rgb_u8(r, g, b))
}

/// Black or white, whichever reads better on `color` (relative luminance).
pub fn ink(color: slint::Color) -> slint::Color {
    let luminance =
        0.2126 * f32::from(color.red()) + 0.7152 * f32::from(color.green()) + 0.0722 * f32::from(color.blue());
    if luminance > 140.0 {
        slint::Color::from_rgb_u8(0x11, 0x11, 0x1b)
    } else {
        slint::Color::from_rgb_u8(0xff, 0xff, 0xff)
    }
}

pub fn picture(entry: EntryRef<'_>, pictures: &Pictures) -> FolderPictureView {
    let shown = entry.picture().and_then(|p| pictures.image(p));
    let tint = color(entry.color());
    FolderPictureView {
        has_image: shown.is_some(),
        mono: shown.as_ref().is_some_and(|s| s.mono),
        image: shown.map(|s| s.image).unwrap_or_default(),
        initial: initial(entry.name()).into(),
        has_color: tint.is_some(),
        ink: tint.map(ink).unwrap_or_default(),
        color: tint.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_inks() {
        assert_eq!(initial(" ärger"), "Ä");
        assert_eq!(initial(""), "?");
        assert_eq!(color(Some("#ff0000")), Some(slint::Color::from_rgb_u8(255, 0, 0)));
        assert_eq!(color(Some("nope")), None);
        assert_eq!(
            ink(slint::Color::from_rgb_u8(250, 240, 200)).red(),
            0x11,
            "dark on light"
        );
        assert_eq!(ink(slint::Color::from_rgb_u8(30, 30, 80)).red(), 0xff, "light on dark");
    }
}
