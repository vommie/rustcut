//! The color of one pixel of a video's frame (P9.14, D-P9l: auto-crop on a border color picked in the video): ffmpeg
//! decodes one frame at a time, runs the filters before the crop (`prefix`, so the coordinates are those of the frame the
//! crop gets), cuts the pixel out and writes it as three bytes of RGB (checked with the real ffmpeg: a 0x808080 bar gave
//! 128 128 128).

use std::ffi::OsString;
use std::path::Path;

use super::detect::DetectOptions;
use super::probe::{ProbeError, run};
use crate::time::Timestamp;

/// The ffmpeg arguments that print the pixel at (`x`, `y`) of the frame at `at` as RGB bytes.
pub fn pick_args(file: &Path, at: Timestamp, prefix: &str, x: u32, y: u32) -> Vec<OsString> {
    let pick = format!("crop=1:1:{x}:{y},format=rgb24");
    let filters = if prefix.is_empty() {
        pick
    } else {
        format!("{prefix},{pick}")
    };
    let mut args: Vec<OsString> = ["-hide_banner", "-nostats", "-nostdin", "-ss"]
        .map(OsString::from)
        .to_vec();
    args.push(at.to_ffmpeg_arg().into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args.extend(["-map", "0:v:0", "-frames:v", "1", "-vf"].map(OsString::from));
    args.push(filters.into());
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
    match output.stdout.as_slice() {
        [r, g, b, ..] => Ok([*r, *g, *b]),
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
}
