//! The filter text of mpv's live preview. mpv's `vf` filters see the decoded frame *before* the source's display rotation
//! (mpv's video output turns the picture afterwards), while ffmpeg's command line turns the frame first and then filters
//! it. So that a crop means the same pixels in the preview and in the render, a preview with filters starts with the
//! source's rotation and mpv's own rotation is switched off (`video-rotate=no`; checked with real mpv and ffmpeg on
//! `rotated_90_metadata.mp4`).

use super::chain::{ChainError, FilterChain};
use super::lavfi::LavfiTarget;

/// What the player's `vf` gets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewFilter {
    /// The lavfi filter list; empty = no filters (mpv turns the picture itself).
    pub lavfi: String,
    /// The text starts with the source's rotation: mpv must not rotate the picture again.
    pub rotates_source: bool,
}

impl PreviewFilter {
    pub fn is_empty(&self) -> bool {
        self.lavfi.is_empty()
    }
}

/// The lavfi filters that turn a picture `degrees` clockwise (the display rotation mpv reports for the source).
pub const fn source_rotation_lavfi(degrees: u16) -> Option<&'static str> {
    match degrees % 360 {
        90 => Some("transpose=dir=clock"),
        180 => Some("hflip,vflip"),
        270 => Some("transpose=dir=cclock"),
        _ => None,
    }
}

impl FilterChain {
    /// The preview filter of this chain for a source that mpv shows turned by `source_rotation` degrees clockwise.
    pub fn preview_filter(&self, source_rotation: u16) -> Result<PreviewFilter, ChainError> {
        let chain = self.to_lavfi(LavfiTarget::Preview)?;
        if chain.is_empty() {
            return Ok(PreviewFilter::default());
        }
        Ok(match source_rotation_lavfi(source_rotation) {
            Some(rotation) => PreviewFilter {
                lavfi: format!("{rotation},{chain}"),
                rotates_source: true,
            },
            None => PreviewFilter {
                lavfi: chain,
                rotates_source: false,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Crop, Filter};

    fn cropped() -> FilterChain {
        let mut chain = FilterChain::default();
        chain.set(Filter::Crop(Crop::new(0, 100, 0, 0)));
        chain.set_enabled(crate::edit::FilterKind::Crop, true);
        chain
    }

    #[test]
    fn a_rotated_source_is_turned_before_the_filters() {
        assert_eq!(
            cropped().preview_filter(270).unwrap(),
            PreviewFilter {
                lavfi: "transpose=dir=cclock,crop=w=iw-100:h=ih-0:x=0:y=0".into(),
                rotates_source: true,
            }
        );
        assert_eq!(
            cropped().preview_filter(0).unwrap(),
            PreviewFilter {
                lavfi: "crop=w=iw-100:h=ih-0:x=0:y=0".into(),
                rotates_source: false,
            }
        );
    }

    #[test]
    fn without_filters_mpv_turns_the_picture_itself() {
        assert_eq!(
            FilterChain::default().preview_filter(90).unwrap(),
            PreviewFilter::default()
        );
        assert_eq!(source_rotation_lavfi(180), Some("hflip,vflip"));
        assert_eq!(source_rotation_lavfi(45), None);
    }
}
