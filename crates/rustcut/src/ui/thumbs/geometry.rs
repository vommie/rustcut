//! Where the thumbnail stands and how big it is (pure).

use rustcut_core::settings::{ThumbPlace, ThumbPortrait, ThumbnailSettings};

/// The picture's width by default, and the share of the column it may take.
const DEFAULT_WIDTH: f32 = 240.0;
const COLUMN_SHARE: f32 = 0.6;
/// Air to the column's edges.
const MARGIN: f32 = 4.0;
/// A wide picture's shape, the measure of "same height" and "same area".
const WIDE: f32 = 16.0 / 9.0;

/// The picture's size for a shown shape (width / height) in a video column of this width.
pub fn size(settings: &ThumbnailSettings, column: f32, aspect: f32) -> (f32, f32) {
    let room = (column - 2.0 * MARGIN).max(40.0);
    let width = if settings.width == 0 {
        DEFAULT_WIDTH.min(column * COLUMN_SHARE)
    } else {
        f32::from(settings.width).min(room)
    };
    let aspect = aspect.clamp(0.1, 10.0);
    if aspect >= 1.0 {
        return (width, width / aspect);
    }
    let height = width / WIDE;
    let (shown_width, shown_height) = match settings.portrait {
        ThumbPortrait::SameHeight => (height * aspect, height),
        ThumbPortrait::SameArea => {
            let tall = (width * height / aspect).sqrt();
            (tall * aspect, tall)
        }
        ThumbPortrait::SameWidth => (width, width / aspect),
    };
    let factor = 1.0 + f32::from(settings.portrait_percent) / 100.0;
    let fit = (room / (shown_width * factor)).min(1.0);
    (shown_width * factor * fit, shown_height * factor * fit)
}

/// The left edge from the column's left. The picture follows the pointer and stays in the column, or stands in the middle.
pub fn left(place: ThumbPlace, column: f32, pointer: f32, width: f32) -> f32 {
    let box_width = width + 4.0;
    let wanted = match place {
        ThumbPlace::FollowPointer => pointer - box_width / 2.0,
        ThumbPlace::Center => (column - box_width) / 2.0,
    };
    wanted.clamp(MARGIN, (column - box_width - MARGIN).max(MARGIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(portrait: ThumbPortrait, percent: i16) -> ThumbnailSettings {
        ThumbnailSettings {
            portrait,
            portrait_percent: percent,
            ..ThumbnailSettings::default()
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) {
        assert!((a.0 - b.0).abs() < 0.6 && (a.1 - b.1).abs() < 0.6, "{a:?} {b:?}");
    }

    #[test]
    fn it_fits_the_column_by_itself() {
        close(size(&ThumbnailSettings::default(), 610.0, WIDE), (240.0, 135.0));
        close(size(&ThumbnailSettings::default(), 240.0, WIDE), (144.0, 81.0));
    }

    #[test]
    fn a_fixed_width_stays_inside_the_column() {
        let fixed = ThumbnailSettings {
            width: 320,
            ..ThumbnailSettings::default()
        };
        close(size(&fixed, 610.0, WIDE), (320.0, 180.0));
        close(size(&fixed, 240.0, WIDE), (232.0, 130.5));
    }

    #[test]
    fn portrait_keeps_the_chosen_measure() {
        let aspect = 9.0 / 16.0;
        close(
            size(&settings(ThumbPortrait::SameHeight, 0), 610.0, aspect),
            (75.9, 135.0),
        );
        close(
            size(&settings(ThumbPortrait::SameArea, 0), 610.0, aspect),
            (135.0, 240.0),
        );
        close(
            size(&settings(ThumbPortrait::SameWidth, 0), 610.0, aspect),
            (240.0, 426.7),
        );
    }

    #[test]
    fn the_percent_adjusts_portrait_only() {
        let aspect = 9.0 / 16.0;
        close(
            size(&settings(ThumbPortrait::SameHeight, 50), 610.0, aspect),
            (113.9, 202.5),
        );
        close(
            size(&settings(ThumbPortrait::SameHeight, 50), 610.0, WIDE),
            (240.0, 135.0),
        );
    }

    #[test]
    fn the_picture_stays_in_the_column() {
        assert!((left(ThumbPlace::FollowPointer, 610.0, 300.0, 240.0) - 178.0).abs() < 0.1);
        assert!((left(ThumbPlace::FollowPointer, 610.0, 5.0, 240.0) - 4.0).abs() < 0.1);
        assert!((left(ThumbPlace::FollowPointer, 610.0, 605.0, 240.0) - 362.0).abs() < 0.1);
        assert!((left(ThumbPlace::Center, 610.0, 5.0, 240.0) - 183.0).abs() < 0.1);
    }

    #[test]
    fn a_column_narrower_than_the_picture_does_not_panic() {
        assert!((left(ThumbPlace::FollowPointer, 100.0, 50.0, 240.0) - 4.0).abs() < 0.1);
    }
}
