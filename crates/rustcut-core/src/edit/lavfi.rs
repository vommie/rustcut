//! The filter chain as lavfi text: one description for the mpv preview (`vf`) and the ffmpeg render (ADR-7).

use std::fmt::Write as _;
use std::path::Path;

use super::chain::{ChainError, FilterChain, Size};
use super::filter::{DeinterlaceRate, Deinterlacer, DeshakeCrop, DeshakeParams, FieldParity, Filter, FilterKind};

/// Which pass of a two-pass deshake render the text is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeshakePass<'a> {
    /// The chain has no enabled deshake.
    None,
    /// Pass 1: the filters up to deshake, ending in `vidstabdetect` writing `result`. The output is discarded.
    Detect { result: &'a Path },
    /// Pass 2: the whole chain with `vidstabtransform` reading `input`.
    Transform { input: &'a Path },
}

/// What the lavfi text is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavfiTarget<'a> {
    /// mpv's live preview: deshake is left out (it needs two passes; the probe render shows it).
    Preview,
    /// The ffmpeg render of a `source`-sized video; an odd output size gets a final 1-pixel crop to even numbers.
    Render { source: Size, deshake: DeshakePass<'a> },
}

impl FilterChain {
    /// The chain as a comma-separated lavfi filter list; empty when nothing is active.
    pub fn to_lavfi(&self, target: LavfiTarget<'_>) -> Result<String, ChainError> {
        let mut parts: Vec<String> = Vec::new();
        match target {
            LavfiTarget::Preview => {
                for filter in self.active().filter(|f| f.kind() != FilterKind::Deshake) {
                    parts.push(filter_lavfi(filter, None)?);
                }
            }
            LavfiTarget::Render { source, deshake } => {
                let out = self.output_size(source)?;
                if self.needs_two_passes() && deshake == DeshakePass::None {
                    return Err(ChainError::DeshakeNeedsPass);
                }
                for filter in self.active() {
                    let is_deshake = filter.kind() == FilterKind::Deshake;
                    parts.push(filter_lavfi(filter, is_deshake.then_some(deshake))?);
                    if is_deshake && matches!(deshake, DeshakePass::Detect { .. }) {
                        // Filters after the detection do not influence the transforms; the pass ends here.
                        return Ok(parts.join(","));
                    }
                }
                if !out.is_even() {
                    let even = out.even_floor();
                    parts.push(format!("crop=w={}:h={}", even.width, even.height));
                }
            }
        }
        Ok(parts.join(","))
    }
}

fn filter_lavfi(filter: &Filter, pass: Option<DeshakePass<'_>>) -> Result<String, ChainError> {
    Ok(match *filter {
        Filter::Deinterlace {
            deinterlacer,
            rate,
            field_order,
            only_interlaced,
        } => deinterlace(deinterlacer, rate, field_order, only_interlaced),
        Filter::Crop(c) => format!(
            "crop=w=iw-{}:h=ih-{}:x={}:y={}",
            c.left + c.right,
            c.top + c.bottom,
            c.left,
            c.top
        ),
        Filter::Deshake(p) => deshake(p.sanitized(), pass.unwrap_or(DeshakePass::None))?,
        Filter::Orientation(p) => p.orientation().lavfi().unwrap_or("null").to_owned(),
        Filter::Resize {
            width,
            height,
            algorithm,
        } => {
            let side = |v: Option<u32>| v.map_or_else(|| "-2".to_owned(), |v| v.to_string());
            format!(
                "scale=w={}:h={}:flags={},setsar=1",
                side(width),
                side(height),
                algorithm.ffmpeg_flag()
            )
        }
    })
}

/// bwdif, yadif and w3fdif take the field order (`parity`) and `deint`; kerndeint has neither (it finds the interlaced
/// lines itself) and ignores them.
fn deinterlace(deinterlacer: Deinterlacer, rate: DeinterlaceRate, order: FieldParity, only_interlaced: bool) -> String {
    let field = rate == DeinterlaceRate::Field && deinterlacer.supports_field_rate();
    let mut s = match deinterlacer {
        Deinterlacer::Bwdif | Deinterlacer::Yadif => {
            format!(
                "{}=mode={}",
                deinterlacer.ffmpeg_name(),
                if field { "send_field" } else { "send_frame" }
            )
        }
        Deinterlacer::W3fdif => format!("w3fdif=mode={}", if field { "field" } else { "frame" }),
        Deinterlacer::Kerndeint => return "kerndeint".to_owned(),
    };
    if let Some(parity) = order.ffmpeg_parity() {
        let _ = write!(s, ":parity={parity}");
    }
    if only_interlaced {
        s.push_str(":deint=interlaced");
    }
    s
}

fn deshake(p: DeshakeParams, pass: DeshakePass<'_>) -> Result<String, ChainError> {
    match pass {
        DeshakePass::None => Err(ChainError::DeshakeNeedsPass),
        DeshakePass::Detect { result } => {
            let mut s = format!("vidstabdetect=result={}", escape_path(result)?);
            let _ = write!(
                s,
                ":shakiness={}:accuracy={}:stepsize={}",
                p.shakiness, p.accuracy, p.stepsize
            );
            if p.tripod {
                s.push_str(":tripod=1");
            }
            Ok(s)
        }
        DeshakePass::Transform { input } => {
            let mut s = format!("vidstabtransform=input={}", escape_path(input)?);
            let crop = match p.crop {
                DeshakeCrop::Black => "black",
                DeshakeCrop::Keep => "keep",
            };
            let _ = write!(
                s,
                ":smoothing={}:optzoom={}:zoom={}:crop={crop}:interpol={}",
                p.smoothing,
                p.optzoom,
                p.zoom,
                p.interpolation.ffmpeg_name()
            );
            if p.optzoom == 2 {
                let _ = write!(s, ":zoomspeed={}", hundredths(p.zoom_speed));
            }
            if let Some(shift) = p.max_shift {
                let _ = write!(s, ":maxshift={shift}");
            }
            if let Some(angle) = p.max_angle {
                // ffmpeg takes radians (at most 3.14); 90 degrees are 1.5708.
                let _ = write!(s, ":maxangle={:.4}", f64::from(angle).to_radians());
            }
            if p.tripod {
                s.push_str(":tripod=1");
            }
            if p.sharpen {
                s.push(',');
                s.push_str(DeshakeParams::SHARPEN);
            }
            Ok(s)
        }
    }
}

/// A number in hundredths as a decimal ("0.25").
fn hundredths(value: u16) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}

fn escape_path(path: &Path) -> Result<String, ChainError> {
    path.to_str().map(escape_value).ok_or(ChainError::NonUtf8Path)
}

/// Escapes a filter option value for a lavfi graph description. Two levels apply (ffmpeg-filters "Notes on filtergraph
/// escaping"): the option parser treats `\ ' :` as special, then the graph parser `\ ' [ ] , ;`. Each level is escaped
/// with a backslash, the inner one first.
pub fn escape_value(value: &str) -> String {
    let level = |text: &str, special: &[char]| -> String {
        let mut out = String::with_capacity(text.len() + 4);
        for c in text.chars() {
            if special.contains(&c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    level(&level(value, &['\\', '\'', ':']), &['\\', '\'', '[', ']', ',', ';'])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::edit::{Crop, DeshakeParams, Orientation, ResizeAlgorithm};

    const HD: Size = Size::new(1920, 1080);

    fn chain_with(filters: &[Filter]) -> FilterChain {
        let mut chain = FilterChain::default();
        for &f in filters {
            chain.set(f);
            chain.set_enabled(f.kind(), true);
        }
        chain
    }

    fn render(chain: &FilterChain, source: Size, deshake: DeshakePass<'_>) -> String {
        chain.to_lavfi(LavfiTarget::Render { source, deshake }).unwrap()
    }

    #[test]
    fn empty_chain_is_empty_text() {
        let chain = FilterChain::default();
        assert_eq!(chain.to_lavfi(LavfiTarget::Preview).unwrap(), "");
        assert_eq!(render(&chain, HD, DeshakePass::None), "");
    }

    #[test]
    fn every_filter_kind() {
        let deinterlace = |d, r| {
            chain_with(&[Filter::Deinterlace {
                deinterlacer: d,
                rate: r,
                field_order: FieldParity::Auto,
                only_interlaced: false,
            }])
        };
        let orientation = |o: Orientation| render(&chain_with(&[Filter::Orientation(o.into())]), HD, DeshakePass::None);
        let lines = [
            render(
                &deinterlace(Deinterlacer::Bwdif, DeinterlaceRate::Frame),
                HD,
                DeshakePass::None,
            ),
            render(
                &deinterlace(Deinterlacer::Yadif, DeinterlaceRate::Field),
                HD,
                DeshakePass::None,
            ),
            render(
                &deinterlace(Deinterlacer::W3fdif, DeinterlaceRate::Field),
                HD,
                DeshakePass::None,
            ),
            render(
                &deinterlace(Deinterlacer::Kerndeint, DeinterlaceRate::Field),
                HD,
                DeshakePass::None,
            ),
            render(
                &chain_with(&[Filter::Crop(Crop::new(8, 10, 12, 14))]),
                HD,
                DeshakePass::None,
            ),
            orientation(Orientation::CLOCKWISE),
            orientation(Orientation::COUNTER_CLOCKWISE),
            orientation(Orientation::UPSIDE_DOWN),
            orientation(Orientation::MIRROR_HORIZONTAL),
            orientation(Orientation::MIRROR_VERTICAL),
            orientation(Orientation::CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL)),
            orientation(Orientation::COUNTER_CLOCKWISE.then(Orientation::MIRROR_HORIZONTAL)),
            render(
                &chain_with(&[Filter::Resize {
                    width: Some(1280),
                    height: None,
                    algorithm: ResizeAlgorithm::Lanczos,
                }]),
                HD,
                DeshakePass::None,
            ),
            render(
                &chain_with(&[Filter::Resize {
                    width: Some(1280),
                    height: Some(720),
                    algorithm: ResizeAlgorithm::Bicubic,
                }]),
                HD,
                DeshakePass::None,
            ),
        ];
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn deinterlace_field_order_and_marked_frames() {
        let chain = |deinterlacer| {
            chain_with(&[Filter::Deinterlace {
                deinterlacer,
                rate: DeinterlaceRate::Field,
                field_order: FieldParity::BottomFirst,
                only_interlaced: true,
            }])
        };
        assert_eq!(
            render(&chain(Deinterlacer::Bwdif), HD, DeshakePass::None),
            "bwdif=mode=send_field:parity=bff:deint=interlaced"
        );
        assert_eq!(
            render(&chain(Deinterlacer::W3fdif), HD, DeshakePass::None),
            "w3fdif=mode=field:parity=bff:deint=interlaced"
        );
        assert_eq!(
            render(&chain(Deinterlacer::Kerndeint), HD, DeshakePass::None),
            "kerndeint"
        );
    }

    #[test]
    fn deshake_limits_and_zoom_speed() {
        let trf = PathBuf::from("/t/job.trf");
        let chain = chain_with(&[Filter::Deshake(DeshakeParams {
            optzoom: 2,
            zoom_speed: 50,
            max_shift: Some(40),
            max_angle: Some(2),
            sharpen: false,
            ..DeshakeParams::default()
        })]);
        assert_eq!(
            render(&chain, HD, DeshakePass::Transform { input: &trf }),
            "vidstabtransform=input=/t/job.trf:smoothing=10:optzoom=2:zoom=0:crop=black:interpol=bicubic:zoomspeed=0.50:\
             maxshift=40:maxangle=0.0349"
        );
    }

    #[test]
    fn the_render_runs_the_filters_in_the_cards_order() {
        // Q-P9n: the order of the Filters card (the chain's slots) is the order ffmpeg applies them, after any move.
        let mut chain = chain_with(&[
            Filter::Crop(Crop::new(10, 0, 10, 0)),
            Filter::Resize {
                width: Some(960),
                height: None,
                algorithm: ResizeAlgorithm::Bicubic,
            },
        ]);
        let crop = "crop=w=iw-0:h=ih-20:x=0:y=10";
        let scale = "scale=w=960:h=-2:flags=bicubic,setsar=1";
        assert_eq!(render(&chain, HD, DeshakePass::None), format!("{crop},{scale}"));
        assert!(chain.move_to(FilterKind::Resize, 0));
        assert_eq!(chain.order().next(), Some(FilterKind::Resize));
        assert_eq!(render(&chain, HD, DeshakePass::None), format!("{scale},{crop}"));
        // The size follows the same order: 1920x1080 -> 960x540 -> 960x520.
        assert_eq!(chain.output_size(HD).unwrap(), Size::new(960, 520));
    }

    #[test]
    fn odd_output_gets_an_even_crop() {
        let chain = chain_with(&[Filter::Crop(Crop::new(1, 0, 0, 3))]);
        assert_eq!(
            render(&chain, HD, DeshakePass::None),
            "crop=w=iw-3:h=ih-1:x=3:y=1,crop=w=1916:h=1078"
        );
        let odd_source = FilterChain::default();
        assert_eq!(
            render(&odd_source, Size::new(719, 405), DeshakePass::None),
            "crop=w=718:h=404"
        );
        assert_eq!(
            odd_source.to_lavfi(LavfiTarget::Preview).unwrap(),
            "",
            "the preview shows odd sizes as they are"
        );
    }

    #[test]
    fn deshake_passes() {
        let chain = chain_with(&[
            Filter::Deinterlace {
                deinterlacer: Deinterlacer::Bwdif,
                rate: DeinterlaceRate::Frame,
                field_order: FieldParity::Auto,
                only_interlaced: false,
            },
            Filter::Crop(Crop::new(0, 0, 140, 0)),
            Filter::Deshake(DeshakeParams {
                tripod: true,
                ..DeshakeParams::default()
            }),
            Filter::Resize {
                width: Some(1280),
                height: None,
                algorithm: ResizeAlgorithm::Bicubic,
            },
        ]);
        let trf = PathBuf::from("/home/user/My: Videos, [old]/job-7.trf");
        let detect = render(&chain, HD, DeshakePass::Detect { result: &trf });
        let transform = render(&chain, HD, DeshakePass::Transform { input: &trf });
        let preview = chain.to_lavfi(LavfiTarget::Preview).unwrap();
        insta::assert_snapshot!(format!(
            "detect:    {detect}\ntransform: {transform}\npreview:   {preview}"
        ));
        assert_eq!(
            chain.to_lavfi(LavfiTarget::Render {
                source: HD,
                deshake: DeshakePass::None
            }),
            Err(ChainError::DeshakeNeedsPass)
        );
    }

    #[test]
    fn deshake_sharpens_after_the_transform_unless_switched_off() {
        let trf = PathBuf::from("/t/job.trf");
        let with = |sharpen| {
            let chain = chain_with(&[
                Filter::Deshake(DeshakeParams {
                    sharpen,
                    ..DeshakeParams::default()
                }),
                Filter::Resize {
                    width: Some(1280),
                    height: None,
                    algorithm: ResizeAlgorithm::Bicubic,
                },
            ]);
            (
                render(&chain, HD, DeshakePass::Detect { result: &trf }),
                render(&chain, HD, DeshakePass::Transform { input: &trf }),
            )
        };
        let (detect, transform) = with(true);
        assert!(
            !detect.contains("unsharp"),
            "the detection pass never sharpens: {detect}"
        );
        assert!(
            transform.contains("interpol=bicubic,unsharp=5:5:0.8:3:3:0.4,scale="),
            "right after the transform, before the next filter: {transform}"
        );
        assert!(!with(false).1.contains("unsharp"));
        assert!(DeshakeParams::default().sharpen, "on for a fresh chain (Q-P9n)");
    }

    #[test]
    fn escaping_covers_both_levels() {
        assert_eq!(escape_value("plain/path.trf"), "plain/path.trf");
        assert_eq!(escape_value("a:b"), r"a\\:b");
        assert_eq!(escape_value("a,b[c];d"), r"a\,b\[c\]\;d");
        assert_eq!(escape_value(r"a'b\c"), r"a\\\'b\\\\c");
    }

    #[test]
    fn crop_errors_reach_the_caller() {
        let chain = chain_with(&[Filter::Crop(Crop::new(0, 0, 1080, 0))]);
        assert!(
            chain
                .to_lavfi(LavfiTarget::Render {
                    source: HD,
                    deshake: DeshakePass::None
                })
                .is_err()
        );
    }
}
