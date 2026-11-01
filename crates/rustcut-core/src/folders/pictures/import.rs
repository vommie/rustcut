//! Turning a file the user picked into a stored picture (P11.2, user decision Q-FP2): an image is decoded with limits,
//! scaled down with Lanczos3 to fit 512 px and written as WebP (quality 75, alpha kept); an SVG is read without any
//! external reference, written again by `usvg` (so scripts, links and unknown elements are gone) and kept only when it
//! draws something. Pure apart from the decoders: no file access here.

use image::{DynamicImage, GenericImageView as _, ImageReader, Limits, imageops::FilterType};

/// The largest side of a stored image.
pub const MAX_SIDE: u32 = 512;
/// WebP quality (0-100).
pub const WEBP_QUALITY: f32 = 75.0;
/// The largest file taken (an SVG or an image before it is scaled down).
pub const MAX_INPUT: u64 = 32 * 1024 * 1024;
/// Decoding limits: no side above this, no more memory than `MAX_ALLOC`.
const MAX_DECODED_SIDE: u32 = 16_384;
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// What a stored picture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PictureKind {
    /// A sanitized SVG; `mono` = drawn in one color (shown in the theme's text color or the folder's color).
    Svg {
        mono: bool,
    },
    WebP,
}

impl PictureKind {
    /// The kind's name in the store.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Svg { .. } => "svg",
            Self::WebP => "webp",
        }
    }
}

/// A picture ready to be stored: its bytes and the BLAKE3 hash that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub hash: String,
    pub kind: PictureKind,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Why a file was refused (the texts are for the user).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PictureError {
    #[error("The file is larger than 32 MiB.")]
    TooLarge,
    #[error("The file is neither an image nor an SVG RustCut can read: {0}")]
    Unreadable(String),
    #[error("The SVG draws nothing.")]
    Empty,
    #[error("The picture could not be stored: {0}")]
    Store(String),
}

/// Prepares `data` (an SVG or an image).
pub fn prepare(data: &[u8]) -> Result<Prepared, PictureError> {
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_INPUT {
        return Err(PictureError::TooLarge);
    }
    if looks_like_svg(data) {
        prepare_svg(data)
    } else {
        prepare_image(data)
    }
}

/// `<svg` or `<?xml` near the start (after a BOM and white space), or gzip (SVGZ is not taken).
fn looks_like_svg(data: &[u8]) -> bool {
    let text = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let start = text.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(text.len());
    text.get(start..).is_some_and(|t| t.starts_with(b"<"))
}

fn hash(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

fn prepare_image(data: &[u8]) -> Result<Prepared, PictureError> {
    let unreadable = |e: &dyn std::fmt::Display| PictureError::Unreadable(e.to_string());
    let mut reader = ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| unreadable(&e))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| unreadable(&e))?;
    let (w, h) = decoded.dimensions();
    let scaled = if w > MAX_SIDE || h > MAX_SIDE {
        decoded.resize(MAX_SIDE, MAX_SIDE, FilterType::Lanczos3)
    } else {
        decoded
    };
    encode_webp(&scaled)
}

/// WebP with alpha when the picture has any (a picture without it is stored without the channel).
fn encode_webp(picture: &DynamicImage) -> Result<Prepared, PictureError> {
    let (width, height) = picture.dimensions();
    let rgba = picture.to_rgba8();
    let opaque = rgba.pixels().all(|p| p.0[3] == u8::MAX);
    let memory = if opaque {
        let rgb = picture.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), width, height).encode(WEBP_QUALITY)
    } else {
        webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(WEBP_QUALITY)
    };
    let data = memory.to_vec();
    if data.is_empty() {
        return Err(PictureError::Unreadable("the WebP encoder failed".into()));
    }
    Ok(Prepared {
        hash: hash(&data),
        kind: PictureKind::WebP,
        data,
        width,
        height,
    })
}

/// Options that never read anything outside the SVG itself: no files, no fonts; embedded (data URL) images stay.
fn safe_options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_href: &str, _options: &usvg::Options<'_>| None),
        },
        ..usvg::Options::default()
    }
}

fn prepare_svg(data: &[u8]) -> Result<Prepared, PictureError> {
    let tree = usvg::Tree::from_data(data, &safe_options()).map_err(|e| PictureError::Unreadable(e.to_string()))?;
    let clean = tree.to_string(&usvg::WriteOptions::default());
    // The written SVG is read again: what is stored is exactly what draws.
    let tree = usvg::Tree::from_str(&clean, &safe_options()).map_err(|e| PictureError::Unreadable(e.to_string()))?;
    if !draws_something(&tree) {
        return Err(PictureError::Empty);
    }
    let size = tree.size();
    let mono = colors(tree.root()).is_some_and(|c| c.len() <= 1);
    let data = clean.into_bytes();
    Ok(Prepared {
        hash: hash(&data),
        kind: PictureKind::Svg { mono },
        data,
        width: to_px(size.width()),
        height: to_px(size.height()),
    })
}

/// A size in whole pixels, at least 1.
fn to_px(v: f32) -> u32 {
    if v.is_finite() {
        az::saturating_cast::<f32, u32>(v.round()).max(1)
    } else {
        1
    }
}

/// Renders the SVG small and looks for a pixel that is not transparent.
fn draws_something(tree: &usvg::Tree) -> bool {
    const SIDE: u32 = 64;
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(SIDE, SIDE) else {
        return false;
    };
    let size = tree.size();
    let scale = 64.0 / size.width().max(size.height()).max(f32::EPSILON);
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.pixels().iter().any(|p| p.alpha() > 0)
}

/// The solid colors the SVG paints with; `None` when it uses a gradient, a pattern or an embedded image (multicolor).
fn colors(group: &usvg::Group) -> Option<Vec<(u8, u8, u8)>> {
    let mut found: Vec<(u8, u8, u8)> = Vec::new();
    let mut add = |paint: &usvg::Paint| -> Option<()> {
        let usvg::Paint::Color(c) = paint else { return None };
        let rgb = (c.red, c.green, c.blue);
        if !found.contains(&rgb) {
            found.push(rgb);
        }
        Some(())
    };
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => {
                for rgb in colors(g)? {
                    add(&usvg::Paint::Color(usvg::Color::new_rgb(rgb.0, rgb.1, rgb.2)))?;
                }
            }
            usvg::Node::Path(p) => {
                if let Some(fill) = p.fill() {
                    add(fill.paint())?;
                }
                if let Some(stroke) = p.stroke() {
                    add(stroke.paint())?;
                }
            }
            usvg::Node::Image(_) => return None,
            usvg::Node::Text(t) => colors(t.flattened())?.into_iter().for_each(|rgb| {
                let _ = add(&usvg::Paint::Color(usvg::Color::new_rgb(rgb.0, rgb.1, rgb.2)));
            }),
        }
    }
    Some(found)
}

/// Straight (not premultiplied) RGBA pixels, row by row: what the UI shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decodes a stored WebP to RGBA pixels (for the UI).
pub fn decode_webp(data: &[u8]) -> Option<Rgba> {
    let picture = image::load_from_memory_with_format(data, image::ImageFormat::WebP)
        .ok()?
        .to_rgba8();
    Some(Rgba {
        width: picture.width(),
        height: picture.height(),
        pixels: picture.into_raw(),
    })
}

/// A picture cut to a spoke of the folder wheel (P12 3.5): scaled to cover `width` x `height`, centered both ways, and
/// masked by `shape` (SVG path commands in that box).
pub fn spoke_picture(picture: &Rgba, width: u32, height: u32, shape: &str) -> Option<Rgba> {
    let source = image::RgbaImage::from_raw(picture.width, picture.height, picture.pixels.clone())?;
    let (w, h) = (f64::from(picture.width.max(1)), f64::from(picture.height.max(1)));
    let scale = (f64::from(width) / w).max(f64::from(height) / h);
    let sized = |side: f64, least: u32| az::saturating_cast::<f64, u32>(side.ceil()).max(least);
    let (cover_w, cover_h) = (sized(w * scale, width), sized(h * scale, height));
    let cover = image::imageops::resize(&source, cover_w, cover_h, FilterType::Triangle);
    let left = (cover_w - width) / 2;
    let top = (cover_h - height) / 2;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\"><path d=\"{shape}\" fill=\"#fff\"/></svg>"
    );
    let mask = render_svg(svg.as_bytes(), width.max(height))?;
    if (mask.width, mask.height) != (width, height) {
        return None;
    }
    let mut pixels = Vec::with_capacity(mask.pixels.len());
    for (y, row) in mask.pixels.chunks_exact(4 * usize::try_from(width).ok()?).enumerate() {
        for (x, m) in row.as_chunks::<4>().0.iter().enumerate() {
            let (px, py) = (u32::try_from(x).ok()? + left, u32::try_from(y).ok()? + top);
            let [r, g, b, a] = cover.get_pixel_checked(px, py)?.0;
            let [.., mask_alpha] = *m;
            let alpha = u16::from(a) * u16::from(mask_alpha) / 255;
            pixels.extend_from_slice(&[r, g, b, u8::try_from(alpha).unwrap_or(u8::MAX)]);
        }
    }
    Some(Rgba { width, height, pixels })
}

/// Renders an SVG (stored or built in) to RGBA pixels fitting `side` (for the UI).
pub fn render_svg(data: &[u8], side: u32) -> Option<Rgba> {
    let tree = usvg::Tree::from_data(data, &safe_options()).ok()?;
    let size = tree.size();
    let longest = size.width().max(size.height()).max(f32::EPSILON);
    let scale = f32::from(u16::try_from(side).unwrap_or(u16::MAX)) / longest;
    let width = to_px(size.width() * scale).min(side);
    let height = to_px(size.height() * scale).min(side);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps premultiplied alpha; the UI wants straight RGBA.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Rgba { width, height, pixels })
}
