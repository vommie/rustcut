//! A job's own picture for the queue's job card (P12 block 10, user decision 2026-11-02): the source's frame at the start
//! of the job's first section, run through the job's filters like the render (turned, mirrored, cropped, deinterlaced;
//! deshake needs two passes and is left out, like the preview), with square pixels, fitted inside [`PICTURE_WIDTH`] x
//! [`PICTURE_HEIGHT`]. ffmpeg hands it over as one lossless PNG on stdout; RustCut keeps it as WebP at quality
//! [`WEBP_QUALITY`] (user decision 2026-11-02). It never uses the timeline's thumbnails, which are optional. The queue
//! makes it when a job is saved and stores it with the job (`store::JobPicture`).

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use super::probe::run_as;
use crate::edit::LavfiTarget;
use crate::job::JobSpec;
use crate::store::JobPicture;
use crate::time::Timestamp;

/// Twice the card's 80 x 45 picture, for scaled screens.
pub const PICTURE_WIDTH: u32 = 160;
pub const PICTURE_HEIGHT: u32 = 90;
/// An exact seek decodes from the keyframe before the place; a long GOP of a big file takes a few seconds.
const LIMIT: Duration = Duration::from_secs(30);
/// The stored WebP's quality (user decision 2026-11-02).
pub const WEBP_QUALITY: f32 = 50.0;
/// A PNG of 160 x 90 is at most about 60 KiB; more than this is no picture.
const MAX_BYTES: u64 = 1024 * 1024;

/// ffmpeg's arguments for the picture of `spec`; `None` for a job without a section or with filters that cannot be
/// written (the render would refuse them too).
pub fn job_picture_args(spec: &JobSpec) -> Option<Vec<OsString>> {
    let at = spec.sections.sections().next()?.start();
    let chain = spec.filters.to_lavfi(LavfiTarget::Preview).ok()?;
    Some(picture_args(&spec.source, at, &chain))
}

fn picture_args(source: &Path, at: Timestamp, chain: &str) -> Vec<OsString> {
    let (w, h) = (PICTURE_WIDTH, PICTURE_HEIGHT);
    // The job's filters first (their numbers are the source's pixels), then square pixels, then the fit.
    let fit = format!(
        "scale=w='trunc(iw*sar/2)*2':h=ih,setsar=1,\
         scale=w={w}:h={h}:force_original_aspect_ratio=decrease:flags=bicubic"
    );
    let filters = if chain.is_empty() {
        fit
    } else {
        format!("{chain},{fit}")
    };
    let mut args: Vec<OsString> = ["-v", "error", "-hide_banner", "-nostats", "-nostdin", "-ss"]
        .map(OsString::from)
        .to_vec();
    args.push(at.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(source.as_os_str().to_owned());
    args.extend(["-map", "0:v:0", "-frames:v", "1", "-vf"].map(OsString::from));
    args.push(filters.into());
    args.extend(
        [
            "-an",
            "-sn",
            "-dn",
            "-pix_fmt",
            "rgb24",
            "-c:v",
            "png",
            "-f",
            "image2pipe",
            "-",
        ]
        .map(OsString::from),
    );
    args
}

/// Makes the picture of `spec` with `ffmpeg` at a low priority. `None` when ffmpeg cannot (a missing or broken source);
/// the reason is logged.
pub async fn make_job_picture(ffmpeg: &Path, spec: &JobSpec) -> Option<JobPicture> {
    let args = job_picture_args(spec)?;
    let output = match run_as(ffmpeg, args, LIMIT, MAX_BYTES, true).await {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            tracing::info!(source = %spec.source.display(), error = %output.failure(), "[Queue] No job picture");
            return None;
        }
        Err(e) => {
            tracing::info!(source = %spec.source.display(), error = %e, "[Queue] No job picture");
            return None;
        }
    };
    let png = output.stdout;
    let picture = tokio::task::spawn_blocking(move || to_webp(&png)).await.ok().flatten();
    if picture.is_none() {
        tracing::info!(source = %spec.source.display(), "[Queue] No job picture: ffmpeg wrote no readable PNG");
    }
    picture
}

/// ffmpeg's PNG as the stored WebP (`None` for anything that is no PNG).
fn to_webp(png: &[u8]) -> Option<JobPicture> {
    let rgb = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgb8();
    let (width, height) = rgb.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let webp = webp::Encoder::from_rgb(rgb.as_raw(), width, height)
        .encode(WEBP_QUALITY)
        .to_vec();
    Some(JobPicture { width, height, webp })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Crop, Filter, Section};
    use crate::job::sample_spec;

    fn text(args: &[OsString]) -> String {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_picture_is_the_first_sections_start_with_the_jobs_filters() {
        let mut spec = sample_spec();
        spec.sections.clear();
        spec.sections
            .push(Section::new(Timestamp::from_secs(190), Timestamp::from_secs(220)).unwrap());
        spec.sections
            .push(Section::new(Timestamp::from_secs(5), Timestamp::from_secs(9)).unwrap());
        let plain = text(&job_picture_args(&spec).unwrap());
        assert!(plain.contains("-ss 190.000000 -i /videos/in.mp4"), "{plain}");
        assert!(
            plain.contains("-vf scale=w='trunc(iw*sar/2)*2':h=ih,setsar=1,scale=w=160:h=90"),
            "no filters: only the fit: {plain}"
        );
        assert!(plain.ends_with("-c:v png -f image2pipe -"), "{plain}");
        spec.filters.set(Filter::Crop(Crop {
            left: 10,
            right: 10,
            top: 0,
            bottom: 20,
        }));
        spec.filters.set_enabled(crate::edit::FilterKind::Crop, true);
        let cropped = text(&job_picture_args(&spec).unwrap());
        assert!(cropped.contains("-vf crop="), "the crop comes first: {cropped}");
        spec.sections.clear();
        assert!(job_picture_args(&spec).is_none(), "no section, no picture");
    }

    #[test]
    fn ffmpegs_png_is_kept_as_webp() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(16, 9, image::Rgb([200, 40, 40]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let picture = to_webp(&png).unwrap();
        assert_eq!((picture.width, picture.height), (16, 9));
        assert!(picture.webp.starts_with(b"RIFF") && picture.webp.get(8..12) == Some(b"WEBP".as_slice()));
        assert_eq!(to_webp(b"not a picture"), None);
    }
}
