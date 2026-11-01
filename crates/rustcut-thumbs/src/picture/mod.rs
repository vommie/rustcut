//! From a decoded frame to a small WebP. libavfilter does the scaling, because it honors the frame's color space and
//! range (plain swscale reads everything as BT.601) and can take one field of an interlaced frame. Rotation stays out of
//! here on purpose: RustCut turns the one picture it shows when it draws it, so a changed orientation costs nothing.

use ffmpeg_next::format::Pixel;
use ffmpeg_next::util::color::TransferCharacteristic;
use ffmpeg_next::{Rational, filter, frame};
use rustcut_thumbs_protocol::fit;

/// What a graph was built for; a frame that differs gets a new one (a stream may change its size).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Input {
    format: Pixel,
    width: u32,
    height: u32,
    interlaced: bool,
}

pub struct Picturer {
    long_side: u32,
    quality: f32,
    aspect: (i32, i32),
    time_base: Rational,
    graph: Option<(Input, filter::Graph)>,
    output: frame::Video,
    curve: Option<[u8; 256]>,
}

pub struct Picture {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Picturer {
    pub fn new(long_side: u32, quality: u8, aspect: (i32, i32), time_base: Rational) -> Self {
        Self {
            long_side,
            quality: f32::from(quality.min(100)),
            aspect,
            time_base,
            graph: None,
            output: frame::Video::empty(),
            curve: None,
        }
    }

    /// The frame as a WebP, `None` when libav refuses it (a broken frame is a missing thumbnail, nothing more).
    pub fn make(&mut self, source: &frame::Video) -> Option<Picture> {
        let input = Input {
            format: source.format(),
            width: source.width(),
            height: source.height(),
            interlaced: source.is_interlaced(),
        };
        if self.graph.as_ref().is_none_or(|(known, _graph)| *known != input) {
            self.graph = Some((input, self.build(input)?));
            self.curve = hdr_curve(source.color_transfer_characteristic());
        }
        let (_input, graph) = self.graph.as_mut()?;
        graph.get("in")?.source().add(source).ok()?;
        graph.get("out")?.sink().frame(&mut self.output).ok()?;
        let (width, height) = (self.output.width(), self.output.height());
        let mut rgb = packed_rows(&self.output)?;
        if let Some(curve) = &self.curve {
            for value in &mut rgb {
                *value = curve.get(usize::from(*value)).copied().unwrap_or(*value);
            }
        }
        let webp = encode(&rgb, width, height, self.quality)?;
        Some(Picture { webp, width, height })
    }

    fn build(&self, input: Input) -> Option<filter::Graph> {
        let (width, height) = fit(input.width, input.height, self.aspect, self.long_side);
        let mut graph = filter::Graph::new();
        let (aspect_n, aspect_d) = self.aspect;
        let arguments = format!(
            "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect={aspect_n}/{aspect_d}",
            input.width,
            input.height,
            pixel_id(input.format)?,
            self.time_base.numerator(),
            self.time_base.denominator().max(1),
        );
        graph.add(&filter::find("buffer")?, "in", &arguments).ok()?;
        graph.add(&filter::find("buffersink")?, "out", "").ok()?;
        let field = if input.interlaced { "field=top," } else { "" };
        let chain = format!("{field}scale={width}:{height}:flags=bicubic,format=rgb24");
        graph.output("in", 0).ok()?.input("out", 0).ok()?.parse(&chain).ok()?;
        graph.validate().ok()?;
        Some(graph)
    }
}

/// libavfilter names a pixel format by its number.
fn pixel_id(format: Pixel) -> Option<i32> {
    let raw: ffmpeg_next::ffi::AVPixelFormat = format.into();
    i32::try_from(raw as isize).ok()
}

/// The frame's RGB without the row padding.
fn packed_rows(output: &frame::Video) -> Option<Vec<u8>> {
    let width = usize::try_from(output.width()).ok()?;
    let height = usize::try_from(output.height()).ok()?;
    let stride = output.stride(0);
    let row = width.checked_mul(3)?;
    let mut rgb = Vec::with_capacity(row.checked_mul(height)?);
    for line in output.data(0).chunks(stride.max(1)).take(height) {
        rgb.extend_from_slice(line.get(..row)?);
    }
    Some(rgb)
}

fn encode(rgb: &[u8], width: u32, height: u32, quality: f32) -> Option<Vec<u8>> {
    let mut config = webp::WebPConfig::new().ok()?;
    config.quality = quality;
    // Method 0: an order of magnitude faster for a few percent of size (the lab's numbers).
    config.method = 0;
    let encoded = webp::Encoder::from_rgb(rgb, width, height)
        .encode_advanced(&config)
        .ok()?;
    Some(encoded.to_vec())
}

/// A curve that brings HDR video to a normal brightness, a table so it costs next to nothing. Not color accurate, enough
/// to recognize a place (user decision Q-P12e). PQ reaches a normal white at about 58 % of its range, HLG at 75 %.
fn hdr_curve(transfer: TransferCharacteristic) -> Option<[u8; 256]> {
    let white = match transfer {
        TransferCharacteristic::SMPTE2084 => 0.58,
        TransferCharacteristic::ARIB_STD_B67 => 0.75,
        _ => return None,
    };
    let mut table = [0_u8; 256];
    for (value, entry) in (0_u8..=255).zip(table.iter_mut()) {
        let shown = (f64::from(value) / 255.0 / white).min(1.0).powf(0.9);
        // Between 0 and 255 by the min and the factor.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "0.0..=255.0 here"
        )]
        let byte = (shown * 255.0).round() as u8;
        *entry = byte;
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_white_reaches_full_brightness() {
        let curve = hdr_curve(TransferCharacteristic::SMPTE2084).unwrap();
        assert_eq!(curve[150], 255);
        assert_eq!(curve[0], 0);
        assert!(curve.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(hdr_curve(TransferCharacteristic::BT709).is_none());
    }
}
