//! Where a video's sidecar lives: `.<file name>.md5` in the same folder (rule 1).

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The extension every sidecar ends with.
pub const EXTENSION: &str = "md5";

/// The sidecar of `video`: `dir/clip.mp4` -> `dir/.clip.mp4.md5`. `None` when the path has no file name (`/`, `..`).
pub fn path_for(video: &Path) -> Option<PathBuf> {
    let name = video.file_name()?;
    let mut sidecar = OsString::with_capacity(name.len() + 2 + EXTENSION.len());
    sidecar.push(".");
    sidecar.push(name);
    sidecar.push(".");
    sidecar.push(EXTENSION);
    Some(video.with_file_name(sidecar))
}

/// The video name a sidecar name belongs to (`.clip.mp4.md5` -> `clip.mp4`); `None` when it is no sidecar name.
pub fn video_name(sidecar_name: &OsStr) -> Option<&OsStr> {
    let inner = sidecar_name.as_bytes().strip_prefix(b".")?.strip_suffix(b".md5")?;
    match inner {
        b"" | b"." | b".." => None,
        _ if inner.contains(&b'/') => None,
        _ => Some(OsStr::from_bytes(inner)),
    }
}

/// The video a sidecar belongs to: `dir/.clip.mp4.md5` -> `dir/clip.mp4`; `None` when it is no sidecar path.
pub fn video_for(sidecar: &Path) -> Option<PathBuf> {
    let video = video_name(sidecar.file_name()?)?;
    Some(sidecar.with_file_name(video))
}

/// Whether a file name has the form of a sidecar (`.<name>.md5`). Such a file is never a media file (rule 7).
pub fn is_sidecar_name(name: &OsStr) -> bool {
    video_name(name).is_some()
}

/// Whether a file name is a temporary file a sidecar write left behind when it was interrupted
/// (`..clip.mp4.md5.Ab12Cd`: the write's dot, the sidecar name, a dot and six letters or digits). Like an orphan, it
/// goes to the trash.
pub fn is_write_leftover(name: &OsStr) -> bool {
    let Some(rest) = name.as_bytes().strip_prefix(b".") else {
        return false;
    };
    let Some(split) = rest.len().checked_sub(7) else {
        return false;
    };
    let (sidecar, suffix) = rest.split_at(split);
    match suffix.split_first() {
        Some((b'.', random)) => {
            random.iter().all(u8::is_ascii_alphanumeric) && is_sidecar_name(OsStr::from_bytes(sidecar))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_path_is_hidden_next_to_the_video() {
        assert_eq!(
            path_for(Path::new("/m/clip.mp4")),
            Some(PathBuf::from("/m/.clip.mp4.md5"))
        );
        assert_eq!(path_for(Path::new("clip.mp4")), Some(PathBuf::from(".clip.mp4.md5")));
        assert_eq!(
            path_for(Path::new("/m/.hidden.mkv")),
            Some(PathBuf::from("/m/..hidden.mkv.md5"))
        );
        assert_eq!(path_for(Path::new("/")), None);
        assert_eq!(path_for(Path::new("/m/..")), None);
    }

    #[test]
    fn video_name_reverses_path_for() {
        for video in ["clip.mp4", ".hidden.mkv", "a b (2).webm", "x", "clip.mp4.md5"] {
            let sidecar = path_for(Path::new(video)).unwrap();
            assert_eq!(video_for(&sidecar), Some(PathBuf::from(video)), "{video}");
        }
        for name in [
            ".md5",
            "..md5",
            "...md5",
            "clip.mp4.md5",
            ".clip.mp4",
            ".clip.mp4.MD5",
            "..clip.mp4.md5.Ab12Cd",
        ] {
            assert!(!is_sidecar_name(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn write_leftovers_are_recognized() {
        assert!(is_write_leftover(OsStr::new("..clip.mp4.md5.Ab12Cd")));
        assert!(!is_write_leftover(OsStr::new("..clip.mp4.md5.Ab12C")));
        assert!(!is_write_leftover(OsStr::new("..clip.mp4.md5.Ab-2Cd")));
        assert!(!is_write_leftover(OsStr::new(".clip.mp4.md5")));
        assert!(!is_write_leftover(OsStr::new(".clip.mp4.rtpart.Ab12Cd")));
        assert!(!is_write_leftover(OsStr::new(".")));
    }
}
