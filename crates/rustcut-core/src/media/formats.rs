//! Which files are videos RustCut works with (user request 2026-10-24: a video cutter cares only about videos): the
//! file name extensions of the video containers ffmpeg reads, with the ffmpeg demuxer that reads each one (a golden
//! test checks the installed ffmpeg against this table). The one place for "is this a video file?": the conflict
//! dialog's lists and the file dialog's filter use it.

use std::path::Path;

/// A video container's file name extension (lowercase) and the ffmpeg demuxer that reads it.
pub const VIDEO_FORMATS: &[(&str, &str)] = &[
    ("mkv", "matroska"),
    ("webm", "matroska"),
    ("mp4", "mov"),
    ("m4v", "mov"),
    ("mov", "mov"),
    ("3gp", "mov"),
    ("3g2", "mov"),
    ("f4v", "mov"),
    ("avi", "avi"),
    ("divx", "avi"),
    ("mpg", "mpeg"),
    ("mpeg", "mpeg"),
    ("vob", "mpeg"),
    ("ts", "mpegts"),
    ("m2ts", "mpegts"),
    ("mts", "mpegts"),
    ("wmv", "asf"),
    ("asf", "asf"),
    ("flv", "flv"),
    ("ogv", "ogg"),
    ("mxf", "mxf"),
    ("dv", "dv"),
    ("rm", "rm"),
    ("rmvb", "rm"),
    ("nut", "nut"),
];

/// The extensions of [`VIDEO_FORMATS`], for file dialog filters.
pub fn video_extensions() -> impl Iterator<Item = &'static str> {
    VIDEO_FORMATS.iter().map(|(ext, _)| *ext)
}

/// The file name ends in a video extension (any case). The content is not looked at: a broken file named `.mp4` is a
/// video file that ffprobe will call unreadable.
pub fn is_video_file(name: impl AsRef<Path>) -> bool {
    name.as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| VIDEO_FORMATS.iter().any(|(known, _)| known.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn videos_by_extension_in_any_case() {
        for name in ["a.mkv", "A.MP4", "x.y.Webm", "/videos/clip.m2ts", "Ärger.mov"] {
            assert!(is_video_file(name), "{name}");
        }
        for name in [
            "a.ogg",
            "notes.txt",
            "clip",
            ".mkv",
            "a.mkv.md5",
            "a.mkv.rustcut-part",
            "a.jpg",
        ] {
            assert!(!is_video_file(name), "{name}");
        }
        assert_eq!(video_extensions().count(), VIDEO_FORMATS.len());
    }
}
