//! The one bubble on the Filters card's header about the output size (P9.14, Q-P9.13 "Resize": only from the final size
//! of the whole chain, for the profile's encoder, never twice). Every encoder needs even sizes (the render makes an odd
//! size even); H.264 codes blocks of 16 and HEVC and AV1 blocks of 8, so another size makes the encoder code a padded
//! picture (checked with `trace_headers`: x264 codes 1918x1078 as 1920x1088, x265 as 1920x1080). The source's own size
//! is never the user's doing: no bubble while no filter changes the size.

use rustcut_core::edit::Size;
use rustcut_core::render::CodecFamily;

/// What the bubble says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeAdvice {
    pub title: String,
    pub detail: String,
}

/// The largest multiple of `n` not above `v` (at least `n`).
fn floor_to(v: u32, n: u32) -> u32 {
    (v / n * n).max(n)
}

/// The advice for an output of `size` from a source of `source` encoded as `family`; `None` when nothing is worth
/// saying.
pub fn size_advice(size: Size, source: Size, family: CodecFamily) -> Option<SizeAdvice> {
    if size == source {
        return None;
    }
    if !size.is_even() {
        let even = size.even_floor();
        return Some(SizeAdvice {
            title: format!("{}×{} is odd", size.width, size.height),
            detail: format!(
                "Every encoder needs even sizes: the render makes it {}×{}.",
                even.width, even.height
            ),
        });
    }
    let n = family.block_multiple();
    if size.width % n == 0 && size.height % n == 0 {
        return None;
    }
    let (w, h) = (floor_to(size.width, n), floor_to(size.height, n));
    Some(SizeAdvice {
        title: format!("{}×{} is not a multiple of {n}", size.width, size.height),
        detail: format!(
            "{} codes blocks of {n}: the encoder fills the rest. {w}×{h} avoids it (Crop's menu: Divisible by {n}).",
            family.label()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bubble_speaks_only_about_a_size_the_filters_made() {
        let source = Size::new(1920, 1080);
        // 1080 is no multiple of 16, but the source's own size is not the user's doing.
        assert_eq!(size_advice(source, source, CodecFamily::H264), None);
        assert_eq!(size_advice(Size::new(1920, 800), source, CodecFamily::H264), None);
        let h264 = size_advice(Size::new(1918, 1078), source, CodecFamily::H264).unwrap();
        assert_eq!(h264.title, "1918×1078 is not a multiple of 16");
        assert!(h264.detail.contains("1904×1072"), "{}", h264.detail);
        assert_eq!(size_advice(Size::new(1912, 1072), source, CodecFamily::Hevc), None);
        let odd = size_advice(Size::new(1917, 1079), source, CodecFamily::Av1).unwrap();
        assert!(odd.title.ends_with("is odd") && odd.detail.contains("1916×1078"));
    }
}
