//! One stored thumbnail as the big picture shows it: the source's rotation, the orientation filters and the crop. Only the
//! picture on screen is touched, once, when it shows; the stored pictures never change, so turning or cropping costs
//! nothing however many thumbnails exist (user decision Q-P12e, `benches/thumb_view.rs`: 0.16 ms, with crop and turn 0.26 ms).

use image::{RgbImage, imageops};

use crate::edit::{Crop, Orientation, Size};

/// How the shown picture differs from the stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbView {
    /// The source's rotation and what turns it before the crop.
    pub before: Orientation,
    /// The crop in pixels of the frame it applies to.
    pub crop: Option<(Crop, Size)>,
    /// What turns it after the crop.
    pub after: Orientation,
}

impl Default for ThumbView {
    fn default() -> Self {
        Self {
            before: Orientation::IDENTITY,
            crop: None,
            after: Orientation::IDENTITY,
        }
    }
}

fn oriented(image: RgbImage, orientation: Orientation) -> RgbImage {
    let (turns, mirrored) = orientation.turns_and_mirror();
    let image = match turns {
        1 => imageops::rotate90(&image),
        2 => imageops::rotate180(&image),
        3 => imageops::rotate270(&image),
        _ => image,
    };
    if mirrored { imageops::flip_horizontal(&image) } else { image }
}

/// A length of the frame in pixels of the picture.
fn scaled(length: u32, picture: u32, frame: u32) -> u32 {
    let value = u64::from(length) * u64::from(picture) / u64::from(frame.max(1));
    u32::try_from(value).unwrap_or(picture)
}

fn cropped(image: RgbImage, crop: Crop, frame: Size) -> RgbImage {
    let (width, height) = image.dimensions();
    let left = scaled(crop.left, width, frame.width);
    let top = scaled(crop.top, height, frame.height);
    let right = scaled(crop.right, width, frame.width);
    let bottom = scaled(crop.bottom, height, frame.height);
    let kept_width = width.saturating_sub(left + right).max(1);
    let kept_height = height.saturating_sub(top + bottom).max(1);
    imageops::crop_imm(&image, left, top, kept_width, kept_height).to_image()
}

/// Decodes a stored picture and applies the view.
pub fn render(webp: &[u8], view: &ThumbView) -> Option<RgbImage> {
    let decoded = webp::Decoder::new(webp).decode()?;
    let (width, height) = (decoded.width(), decoded.height());
    let pixels: Vec<u8> = if decoded.is_alpha() {
        decoded.as_chunks::<4>().0.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()
    } else {
        decoded.to_vec()
    };
    let mut image = RgbImage::from_raw(width, height, pixels)?;
    if !view.before.is_identity() {
        image = oriented(image, view.before);
    }
    if let Some((crop, frame)) = view.crop {
        image = cropped(image, crop, frame);
    }
    if !view.after.is_identity() {
        image = oriented(image, view.after);
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 x 2 pixels, red on the left half, blue on the right.
    fn stored() -> Vec<u8> {
        let mut rgb = Vec::new();
        for _row in 0..16 {
            for column in 0..32 {
                rgb.extend_from_slice(if column < 16 { &[255, 0, 0] } else { &[0, 0, 255] });
            }
        }
        webp::Encoder::from_rgb(&rgb, 32, 16).encode(100.0).to_vec()
    }

    fn red_at(image: &RgbImage, x: u32, y: u32) -> bool {
        let pixel = image.get_pixel(x, y).0;
        pixel[0] > pixel[2]
    }

    #[test]
    fn nothing_to_do_keeps_the_picture() {
        let image = render(&stored(), &ThumbView::default()).unwrap();
        assert_eq!(image.dimensions(), (32, 16));
        assert!(red_at(&image, 2, 8));
        assert!(!red_at(&image, 30, 8));
    }

    #[test]
    fn a_turn_to_the_right_brings_the_left_side_up() {
        let view = ThumbView {
            before: Orientation::CLOCKWISE,
            ..ThumbView::default()
        };
        let image = render(&stored(), &view).unwrap();
        assert_eq!(image.dimensions(), (16, 32));
        assert!(red_at(&image, 8, 2));
        assert!(!red_at(&image, 8, 30));
    }

    #[test]
    fn the_crop_counts_in_pixels_of_its_frame() {
        let crop = Crop {
            top: 0,
            right: 960,
            bottom: 0,
            left: 0,
        };
        let frame = Size {
            width: 1920,
            height: 1080,
        };
        let view = ThumbView {
            crop: Some((crop, frame)),
            ..ThumbView::default()
        };
        let image = render(&stored(), &view).unwrap();
        assert_eq!(image.dimensions(), (16, 16));
        assert!(red_at(&image, 14, 8));
    }

    #[test]
    fn a_mirror_after_the_crop_swaps_the_sides() {
        let view = ThumbView {
            after: Orientation::MIRROR_HORIZONTAL,
            ..ThumbView::default()
        };
        let image = render(&stored(), &view).unwrap();
        assert!(!red_at(&image, 2, 8));
        assert!(red_at(&image, 30, 8));
    }

    #[test]
    fn what_is_no_picture_gives_nothing() {
        assert!(render(b"nothing", &ThumbView::default()).is_none());
    }
}
