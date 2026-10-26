//! Picture analysis with ffmpeg's detection filters, as supervised children like the probe (ADR-3): black borders
//! (`cropdetect`, sampled at several points of the video instead of only the playhead, PyCutRv bug 14) and interlacing
//! (`idet`). The filters write their findings to stderr; the parsers here read them. Dropping a detection kills its
//! ffmpeg processes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::crop_search::CropSearch;
use super::probe::{ProbeError, run};
use crate::edit::{Crop, Size};
use crate::time::Timestamp;

/// Frames decoded per crop sample (the first two are skipped by `cropdetect`, they may be half-decoded after the seek).
const CROP_SAMPLE_FRAMES: u32 = 8;
/// Crop samples running at the same time.
const CROP_PARALLEL: usize = 3;
/// Frames `idet` looks at.
const IDET_FRAMES: u32 = 250;
/// stdout of a detection is discarded (`-f null`); this only guards against surprises.
const MAX_STDOUT: u64 = 64 * 1024;

/// What ffmpeg to run and how long one sample may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectOptions {
    /// `None` = `ffmpeg` from `PATH` (settings: `advanced.ffmpeg`).
    pub program: Option<PathBuf>,
    pub sample_timeout: Duration,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            program: None,
            sample_timeout: Duration::from_secs(20),
        }
    }
}

impl DetectOptions {
    pub(crate) fn program(&self) -> &Path {
        self.program.as_deref().unwrap_or_else(|| Path::new("ffmpeg"))
    }
}

/// The picture area one crop sample saw (inclusive pixel bounds, as `cropdetect` reports them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropBox {
    pub x1: u32,
    pub x2: u32,
    pub y1: u32,
    pub y2: u32,
}

impl CropBox {
    /// The smallest box holding both.
    pub fn union(self, other: Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            x2: self.x2.max(other.x2),
            y1: self.y1.min(other.y1),
            y2: self.y2.max(other.y2),
        }
    }

    /// The crop that keeps this box of a `frame`-sized picture.
    pub fn to_crop(self, frame: Size) -> Crop {
        let last_x = frame.width.saturating_sub(1);
        let last_y = frame.height.saturating_sub(1);
        Crop::new(
            self.y1.min(last_y),
            last_x.saturating_sub(self.x2),
            last_y.saturating_sub(self.y2),
            self.x1.min(last_x),
        )
    }
}

/// The result of auto-crop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropDetection {
    /// The black borders to remove (even pixels, even size; zero when there are none).
    pub crop: Crop,
    /// Samples that showed some picture (black samples say nothing about the borders).
    pub samples_used: usize,
    pub samples_taken: usize,
    /// The search that found it (its tolerance) and how many tries it took (0.0.52).
    pub search: CropSearch,
    pub tries: usize,
}

/// The ffmpeg arguments of one crop sample at `at`: the filters before the crop (`prefix`, lavfi text, may be empty) so
/// the borders are found in the frame the crop gets, then `cropdetect` over a few frames.
pub fn cropdetect_args(file: &Path, at: Timestamp, prefix: &str, search: &CropSearch) -> Vec<OsString> {
    let detect = search.detect_filters();
    let filters = if prefix.is_empty() {
        detect
    } else {
        format!("{prefix},{detect}")
    };
    let mut args: Vec<OsString> = ["-hide_banner", "-nostats", "-nostdin", "-ss"]
        .map(OsString::from)
        .to_vec();
    args.push(at.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args.extend(["-map", "0:v:0", "-frames:v"].map(OsString::from));
    args.push(CROP_SAMPLE_FRAMES.to_string().into());
    args.push("-vf".into());
    args.push(filters.into());
    args.extend(["-an", "-sn", "-dn", "-f", "null", "-"].map(OsString::from));
    args
}

/// The area the last `cropdetect` line reports (with `reset=0` it covers every frame of the sample); `None` when the
/// sample was black (no picture: the bounds are reversed) or there is no such line.
pub fn parse_cropdetect(stderr: &str) -> Option<CropBox> {
    let line = stderr.lines().rev().find(|l| l.contains("x1:") && l.contains("y2:"))?;
    let value = |key: &str| -> Option<i64> {
        line.split_whitespace()
            .find_map(|token| token.strip_prefix(key))
            .and_then(|v| v.parse().ok())
    };
    let (x1, x2, y1, y2) = (value("x1:")?, value("x2:")?, value("y1:")?, value("y2:")?);
    if x1 > x2 || y1 > y2 {
        return None;
    }
    Some(CropBox {
        x1: u32::try_from(x1).ok()?,
        x2: u32::try_from(x2).ok()?,
        y1: u32::try_from(y1).ok()?,
        y2: u32::try_from(y2).ok()?,
    })
}

/// The crop that removes only what was black in every sample (the union of their picture areas), moved onto even
/// pixels; `None` when no sample showed a picture.
pub fn combine_crop_samples(boxes: &[CropBox], frame: Size) -> Option<Crop> {
    let union = boxes.iter().copied().reduce(CropBox::union)?;
    Some(union.to_crop(frame).clamped(frame, 4).evened_inward(frame))
}

/// Finds the black borders of `file`'s frames as they reach the crop (`frame` big, after the `prefix` filters), sampled
/// at `times` ([`super::crop_sample_times`]), on the sides `search` allows. Must run on the tokio runtime.
pub async fn detect_crop(
    file: &Path,
    frame: Size,
    times: &[Timestamp],
    prefix: &str,
    search: &CropSearch,
    options: &DetectOptions,
) -> Result<CropDetection, ProbeError> {
    let gate = Arc::new(Semaphore::new(CROP_PARALLEL));
    let mut samples = JoinSet::new();
    for at in times {
        let args = cropdetect_args(file, *at, prefix, search);
        let program = options.program().to_path_buf();
        let timeout = options.sample_timeout;
        let gate = Arc::clone(&gate);
        samples.spawn(async move {
            let _permit = gate.acquire_owned().await;
            run(&program, args, timeout, MAX_STDOUT).await
        });
    }
    let mut boxes = Vec::with_capacity(times.len());
    let mut first_error = None;
    while let Some(joined) = samples.join_next().await {
        match joined {
            Ok(Ok(output)) if output.status.success() => boxes.extend(parse_cropdetect(&output.stderr)),
            Ok(Ok(output)) => {
                first_error.get_or_insert_with(|| output.failure());
            }
            Ok(Err(e)) => {
                first_error.get_or_insert(e);
            }
            Err(e) => tracing::warn!(error = %e, "[Media] A crop sample task ended abnormally"),
        }
    }
    // With a picked color a sample without that color anywhere at its edges (a fade to black covers the bars too) says
    // nothing about them while another sample saw them, as a black sample says nothing about black bars.
    if search.color.is_some() {
        let whole = |b: &CropBox| b.x1 == 0 && b.y1 == 0 && b.x2 + 1 >= frame.width && b.y2 + 1 >= frame.height;
        if boxes.iter().any(|b| !whole(b)) {
            boxes.retain(|b| !whole(b));
        }
    }
    match (combine_crop_samples(&boxes, frame), first_error) {
        (Some(crop), _) => Ok(CropDetection {
            crop: search.sides.keep(crop),
            samples_used: boxes.len(),
            samples_taken: times.len(),
            search: *search,
            tries: 1,
        }),
        (None, Some(error)) => Err(error),
        (None, None) => Ok(CropDetection {
            crop: Crop::default(),
            samples_used: 0,
            samples_taken: times.len(),
            search: *search,
            tries: 1,
        }),
    }
}

/// [`detect_crop`] with the tries of `search` ([`CropSearch::tries`]): the next try runs only while nothing was found
/// in samples that showed a picture. Must run on the tokio runtime.
pub async fn detect_crop_retrying(
    file: &Path,
    frame: Size,
    times: &[Timestamp],
    prefix: &str,
    search: &CropSearch,
    options: &DetectOptions,
) -> Result<CropDetection, ProbeError> {
    let mut found = None;
    for (index, attempt) in search.tries().into_iter().enumerate() {
        let result = detect_crop(file, frame, times, prefix, &attempt, options).await?;
        let done = result.samples_used == 0 || !result.crop.is_identity();
        tracing::debug!(
            try_number = index + 1,
            tolerance = attempt.tolerance(),
            done,
            "[Media] Auto-crop try"
        );
        found = Some(CropDetection {
            tries: index + 1,
            ..result
        });
        if done {
            break;
        }
    }
    found.ok_or_else(|| ProbeError::Failed {
        program: options.program().display().to_string(),
        status: "not run".into(),
        message: "no try".into(),
    })
}

/// What `idet`'s multi-frame detection counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdetCounts {
    pub tff: u32,
    pub bff: u32,
    pub progressive: u32,
    pub undetermined: u32,
}

/// Whether the picture is interlaced, as far as `idet` can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interlacing {
    Progressive,
    /// Interlaced, with the field that comes first.
    Interlaced {
        top_field_first: bool,
    },
    /// Too few frames could be classified (still pictures, black frames, a very short video).
    Unknown,
}

impl IdetCounts {
    /// Interlaced when one field order clearly wins (at least four times the other: real interlacing has one order,
    /// while fine synthetic detail fools `idet` into both) and beats the progressive count; progressive when the
    /// progressive frames are the majority; unknown with fewer than 20 decided frames or a mixed field order.
    pub fn verdict(self) -> Interlacing {
        let interlaced = self.tff.saturating_add(self.bff);
        let decided = interlaced.saturating_add(self.progressive);
        let (first, second) = (self.tff.max(self.bff), self.tff.min(self.bff));
        if decided < 20 {
            Interlacing::Unknown
        } else if first > self.progressive && first >= second.saturating_mul(4) {
            Interlacing::Interlaced {
                top_field_first: self.tff >= self.bff,
            }
        } else if self.progressive >= interlaced {
            Interlacing::Progressive
        } else {
            Interlacing::Unknown
        }
    }
}

/// The ffmpeg arguments of the interlace check: [`IDET_FRAMES`] frames from `start` on.
pub fn idet_args(file: &Path, start: Timestamp) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostats", "-nostdin", "-ss"]
        .map(OsString::from)
        .to_vec();
    args.push(start.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args.extend(["-map", "0:v:0", "-frames:v"].map(OsString::from));
    args.push(IDET_FRAMES.to_string().into());
    args.extend(["-vf", "idet", "-an", "-sn", "-dn", "-f", "null", "-"].map(OsString::from));
    args
}

/// The counts of the last "Multi frame detection" line (ffmpeg may print an empty one for a filter graph it rebuilt).
pub fn parse_idet(stderr: &str) -> Option<IdetCounts> {
    let line = stderr.lines().rev().find(|l| l.contains("Multi frame detection:"))?;
    let rest = line.split("Multi frame detection:").nth(1)?;
    let mut counts = IdetCounts::default();
    let mut tokens = rest.split_whitespace();
    while let Some(key) = tokens.next() {
        let value: u32 = tokens.next()?.parse().ok()?;
        match key {
            "TFF:" => counts.tff = value,
            "BFF:" => counts.bff = value,
            "Progressive:" => counts.progressive = value,
            "Undetermined:" => counts.undetermined = value,
            _ => {}
        }
    }
    Some(counts)
}

/// Where the interlace check starts: a tenth into the video (past intros and black frames), at most one minute in.
pub fn idet_start(duration: Option<Timestamp>) -> Timestamp {
    duration.map_or(Timestamp::ZERO, |d| {
        Timestamp::from_micros((d.as_micros() / 10).clamp(0, 60_000_000))
    })
}

/// Checks whether `file`'s primary video is interlaced. Must run on the tokio runtime.
pub async fn detect_interlace(
    file: &Path,
    duration: Option<Timestamp>,
    options: &DetectOptions,
) -> Result<IdetCounts, ProbeError> {
    let output = run(
        options.program(),
        idet_args(file, idet_start(duration)),
        options.sample_timeout,
        MAX_STDOUT,
    )
    .await?;
    if !output.status.success() {
        return Err(output.failure());
    }
    parse_idet(&output.stderr).ok_or_else(|| ProbeError::Output("ffmpeg reported no interlace counts".into()))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn cropdetect_lines_are_read_and_black_samples_dropped() {
        let stderr = "\
[Parsed_cropdetect_0 @ 0x1] x1:2 x2:637 y1:61 y2:419 w:636 h:358 x:2 y:62 pts:1 t:0.04 limit:0.094 crop=636:358:2:62
[Parsed_cropdetect_0 @ 0x1] x1:0 x2:639 y1:60 y2:419 w:640 h:360 x:0 y:60 pts:2 t:0.08 limit:0.094 crop=640:360:0:60
[out#0/null @ 0x2] video:1KiB audio:0KiB";
        assert_eq!(
            parse_cropdetect(stderr),
            Some(CropBox {
                x1: 0,
                x2: 639,
                y1: 60,
                y2: 419
            })
        );
        let black =
            "[Parsed_cropdetect_0 @ 0x1] x1:319 x2:0 y1:239 y2:0 w:-318 h:-238 x:320 y:240 crop=-318:-238:320:240";
        assert_eq!(parse_cropdetect(black), None);
        assert_eq!(parse_cropdetect("nothing here"), None);
    }

    #[test]
    fn the_union_of_the_samples_keeps_every_picture_seen() {
        let frame = Size::new(640, 480);
        let dark_scene = CropBox {
            x1: 40,
            x2: 600,
            y1: 90,
            y2: 380,
        };
        let bright_scene = CropBox {
            x1: 0,
            x2: 639,
            y1: 61,
            y2: 419,
        };
        // 61 rounds inward to 62; the kept height 420 - 62 = 358 is even.
        assert_eq!(
            combine_crop_samples(&[dark_scene, bright_scene], frame),
            Some(Crop::new(62, 0, 60, 0))
        );
        assert_eq!(combine_crop_samples(&[], frame), None);
    }

    #[test]
    fn idet_counts_and_verdicts() {
        let stderr = "\
[Parsed_idet_0 @ 0x1] Multi frame detection: TFF:     0 BFF:     0 Progressive:     0 Undetermined:     0
[Parsed_idet_0 @ 0x2] Single frame detection: TFF:   240 BFF:     0 Progressive:     3 Undetermined:     7
[Parsed_idet_0 @ 0x2] Multi frame detection: TFF:   250 BFF:     0 Progressive:     0 Undetermined:     0";
        let counts = parse_idet(stderr).unwrap();
        assert_eq!(counts.tff, 250);
        assert_eq!(counts.verdict(), Interlacing::Interlaced { top_field_first: true });
        let progressive = IdetCounts {
            progressive: 240,
            undetermined: 10,
            ..IdetCounts::default()
        };
        assert_eq!(progressive.verdict(), Interlacing::Progressive);
        assert_eq!(IdetCounts::default().verdict(), Interlacing::Unknown);
        let bff = IdetCounts {
            bff: 150,
            tff: 3,
            progressive: 40,
            undetermined: 0,
        };
        assert_eq!(bff.verdict(), Interlacing::Interlaced { top_field_first: false });
        // What idet says about fine synthetic detail (PyCutRv's 320x180 test clip): both orders, no clear winner.
        let mixed = IdetCounts {
            tff: 149,
            bff: 102,
            progressive: 0,
            undetermined: 0,
        };
        assert_eq!(mixed.verdict(), Interlacing::Unknown);
        assert_eq!(idet_start(Some(Timestamp::from_secs(3600))), Timestamp::from_secs(60));
    }

    #[test]
    fn arguments_keep_the_path_a_path_and_chain_the_prefix() {
        let args = cropdetect_args(
            Path::new("-a.mkv"),
            Timestamp::from_secs(5),
            "transpose=dir=clock",
            &CropSearch::default(),
        );
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let i = text.iter().position(|a| a == "-i").unwrap();
        assert_eq!(text.get(i + 1).map(String::as_str), Some("-a.mkv"));
        assert!(text.contains(&"transpose=dir=clock,cropdetect=limit=0.094:round=2:reset=0".to_owned()));
        let gray = CropSearch {
            limit: 51,
            ..CropSearch::default()
        };
        let args = cropdetect_args(Path::new("/v/a.mkv"), Timestamp::ZERO, "", &gray);
        assert!(args.iter().any(|a| a == "cropdetect=limit=0.200:round=2:reset=0"));
        assert!(
            idet_args(Path::new("/v/a.mpg"), Timestamp::ZERO)
                .iter()
                .any(|a| a == "idet")
        );
    }

    proptest! {
        #[test]
        fn combined_crops_are_even_and_inside_the_frame(w in 4u32..4000, h in 4u32..4000,
            boxes in prop::collection::vec((0u32..4000, 0u32..4000, 0u32..4000, 0u32..4000), 1..8)) {
            let frame = Size::new(w, h);
            let boxes: Vec<CropBox> = boxes.into_iter().map(|(a, b, c, d)| CropBox {
                x1: a.min(b) % w, x2: a.max(b) % w, y1: c.min(d) % h, y2: c.max(d) % h,
            }).filter(|b| b.x1 <= b.x2 && b.y1 <= b.y2).collect();
            prop_assume!(!boxes.is_empty());
            let crop = combine_crop_samples(&boxes, frame).unwrap();
            let kept = crop.kept(frame).unwrap();
            prop_assert!(kept.is_even() && crop.top.is_multiple_of(2) && crop.left.is_multiple_of(2));
        }
    }
}
