//! The color of one pixel of a video's frame (P9.14, D-P9l: auto-crop on a border color picked in the video): ffmpeg
//! decodes one frame at a time, runs the filters before the crop (`prefix`, so the coordinates are those of the frame the
//! crop gets), cuts the pixel out and writes it as three bytes of RGB (checked with the real ffmpeg: a 0x808080 bar gave
//! 128 128 128). On a video's last frame no frame starts at or after the playhead, so a second call reads the frames of the
//! second up to it and takes the last (0.0.56).

use std::ffi::OsString;
use std::path::Path;

use super::detect::DetectOptions;
use super::probe::{ProbeError, run};
use crate::time::Timestamp;

/// How far before the playhead the second try looks for the frame shown there.
const SHOWN_WINDOW: Timestamp = Timestamp::from_secs(1);
/// The most the second try may print: one pixel (three bytes) for each frame of the window, at any frame rate.
const SHOWN_MAX_BYTES: u64 = 64 * 1024;

/// The ffmpeg arguments that print the pixel at (`x`, `y`) of the frame at `at` as RGB bytes.
pub fn pick_args(file: &Path, at: Timestamp, prefix: &str, x: u32, y: u32) -> Vec<OsString> {
    let mut args = seek_args(file, at);
    args.extend(["-map", "0:v:0", "-frames:v", "1"].map(OsString::from));
    args.extend(pixel_args(prefix, x, y));
    args
}

/// The ffmpeg arguments that print the pixel of every frame in the second up to `at`: the last one printed is the
/// frame shown at `at`. For a playhead on a video's last frame, where no frame starts at or after it (`-sseof` cannot
/// stand in: it refuses videos shorter than its offset).
pub fn shown_args(file: &Path, at: Timestamp, prefix: &str, x: u32, y: u32) -> Vec<OsString> {
    let start = at.saturating_sub(SHOWN_WINDOW).max(Timestamp::ZERO);
    let length = at.saturating_sub(start).saturating_add(Timestamp::from_millis(1));
    let mut args = seek_args(file, start);
    args.extend(["-map", "0:v:0", "-t"].map(OsString::from));
    args.push(length.to_ffmpeg_arg().into());
    args.extend(pixel_args(prefix, x, y));
    args
}

fn seek_args(file: &Path, at: Timestamp) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostats", "-nostdin", "-ss"]
        .map(OsString::from)
        .to_vec();
    args.push(at.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args
}

/// The filters before the crop, the pixel cut out, and raw RGB on stdout.
fn pixel_args(prefix: &str, x: u32, y: u32) -> Vec<OsString> {
    let pick = format!("crop=1:1:{x}:{y},format=rgb24");
    let filters = if prefix.is_empty() {
        pick
    } else {
        format!("{prefix},{pick}")
    };
    let mut args = vec![OsString::from("-vf"), filters.into()];
    args.extend(["-an", "-sn", "-dn", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"].map(OsString::from));
    args
}

/// The color at (`x`, `y`) of `file`'s frame at `at` after the `prefix` filters. Must run on the tokio runtime.
pub async fn pick_color(
    file: &Path,
    at: Timestamp,
    prefix: &str,
    (x, y): (u32, u32),
    options: &DetectOptions,
) -> Result<[u8; 3], ProbeError> {
    let output = run(
        options.program(),
        pick_args(file, at, prefix, x, y),
        options.sample_timeout,
        64,
    )
    .await?;
    if !output.status.success() {
        return Err(output.failure());
    }
    if let [r, g, b, ..] = output.stdout.as_slice() {
        return Ok([*r, *g, *b]);
    }
    // No frame starts at or after the playhead: it stands on the video's last frame.
    tracing::debug!(at = %at, "[Media] No frame starts at the playhead; reading the frame shown there");
    let output = run(
        options.program(),
        shown_args(file, at, prefix, x, y),
        options.sample_timeout,
        SHOWN_MAX_BYTES,
    )
    .await?;
    if !output.status.success() {
        return Err(output.failure());
    }
    match output.stdout.rchunks_exact(3).next() {
        Some(&[r, g, b]) => Ok([r, g, b]),
        _ => Err(output.failure()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pixel_is_cut_after_the_filters_before_the_crop() {
        let args: Vec<String> = pick_args(Path::new("/v.mkv"), Timestamp::from_secs(3), "hflip", 10, 20)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let vf = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(args[vf + 1], "hflip,crop=1:1:10:20,format=rgb24");
        assert!(args.ends_with(&["-f", "rawvideo", "-pix_fmt", "rgb24", "-"].map(String::from)));
        let plain = pick_args(Path::new("/v.mkv"), Timestamp::ZERO, "", 0, 0);
        assert!(plain.iter().any(|a| a == "crop=1:1:0:0,format=rgb24"));
    }

    #[test]
    fn the_shown_frame_is_looked_for_in_the_second_up_to_the_playhead() {
        let text = |args: Vec<OsString>| {
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let args = text(shown_args(Path::new("/v.mkv"), Timestamp::from_millis(2_005), "", 1, 2));
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(args[ss + 1], "1.005000");
        let t = args.iter().position(|a| a == "-t").unwrap();
        assert_eq!(args[t + 1], "1.001000", "the frame at the playhead itself counts");
        assert!(
            !args.iter().any(|a| a == "-frames:v"),
            "every frame of the window is printed"
        );
        let early = text(shown_args(Path::new("/v.mkv"), Timestamp::from_millis(400), "", 1, 2));
        let ss = early.iter().position(|a| a == "-ss").unwrap();
        assert_eq!((early[ss + 1].as_str(), early[ss + 3].as_str()), ("0.000000", "/v.mkv"));
        assert!(early.iter().any(|a| a == "0.401000"));
    }
}
