//! The preview's video filters (mpv `vf`): the app's lavfi text, wrapped so that mpv's option parser passes it through
//! untouched, and mpv's own display rotation switched off while the text turns the picture itself.
//!
//! mpv's `vf` filters get the decoded frame before the source's display rotation (the video output turns it afterwards),
//! while ffmpeg turns the frame before its filters. The app's preview text therefore starts with the source rotation
//! when filters are active, and mpv must then not rotate again (`video-rotate=no`).

/// What the preview's `vf` shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoFilter {
    /// A lavfi filter list (ffmpeg syntax, any characters); empty = no filters.
    pub lavfi: String,
    /// The list starts with the source's display rotation: mpv's own rotation is switched off.
    pub rotates_source: bool,
}

/// mpv's `vf` value for `filter`: one labeled `lavfi` filter whose graph is passed with mpv's fixed-length quoting
/// (`%<bytes>%<text>`), so commas, colons, brackets and quotes inside the text need no escaping. Empty = no filters.
pub(crate) fn vf_value(filter: &VideoFilter) -> String {
    if filter.lavfi.is_empty() {
        return String::new();
    }
    format!("@rcvf:lavfi=graph=%{}%{}", filter.lavfi.len(), filter.lavfi)
}

/// mpv's `video-rotate` value: `no` ignores the source's rotation, `0` follows it.
pub(crate) const fn video_rotate_value(filter: &VideoFilter) -> &'static str {
    if filter.rotates_source { "no" } else { "0" }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command as Process;
    use std::time::{Duration, Instant};

    use libmpv2::Mpv;
    use libmpv2::events::Event;

    use super::*;
    use crate::engine::commands::{Command, Session, execute};
    use crate::engine::{AudioOptions, PreviewQuality};

    #[test]
    fn the_text_is_passed_with_its_length() {
        let filter = VideoFilter {
            lavfi: "crop=w=iw-2:h=ih,scale=w=-2:h=10".into(),
            rotates_source: false,
        };
        assert_eq!(
            vf_value(&filter),
            "@rcvf:lavfi=graph=%32%crop=w=iw-2:h=ih,scale=w=-2:h=10"
        );
        assert_eq!(vf_value(&VideoFilter::default()), "");
        let umlaut = VideoFilter {
            lavfi: "drawtext=text='Größe'".into(),
            rotates_source: true,
        };
        assert!(
            vf_value(&umlaut).starts_with("@rcvf:lavfi=graph=%23%"),
            "bytes, not characters"
        );
        assert_eq!(video_rotate_value(&umlaut), "no");
    }

    /// The width and height of a PNG file (its IHDR chunk).
    fn png_size(path: &Path) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap();
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (word(16), word(20))
    }

    /// A 64x36 clip whose display matrix turns it a quarter (mpv shows it 36x64), written by ffmpeg.
    fn turned_clip(dir: &Path) -> std::path::PathBuf {
        let plain = dir.join("plain.mp4");
        let turned = dir.join("turned.mp4");
        let ok = Process::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x36:rate=5",
                "-frames:v",
                "3",
            ])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&plain)
            .status()
            .expect("ffmpeg must be installed (sudo pacman -S ffmpeg)")
            .success();
        assert!(ok);
        let ok = Process::new("ffmpeg")
            .args(["-v", "error", "-y", "-display_rotation", "90", "-i"])
            .arg(&plain)
            .args(["-c", "copy"])
            .arg(&turned)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        turned
    }

    /// Shows the first frame of `clip` through the session's `vf` ops with mpv's image output and returns its size.
    fn first_frame(clip: &Path, out: &Path, filter: VideoFilter) -> (u32, u32) {
        std::fs::create_dir_all(out).unwrap();
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vo", "image")?;
            init.set_option("vo-image-format", "png")?;
            init.set_option("vo-image-outdir", out.to_str().unwrap())?;
            init.set_option("ao", "null")?;
            init.set_option("frames", 1)?;
            init.set_option("msg-level", "all=fatal")?;
            Ok(())
        })
        .unwrap();
        let mut session = Session::new(PreviewQuality::High, AudioOptions::default());
        for op in session.handle(Command::SetVideoFilter(filter)) {
            execute(&mpv, &op);
        }
        mpv.command("loadfile", &[clip.to_str().unwrap()]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "mpv did not finish the clip");
            match mpv.wait_event(1.0) {
                Some(Ok(Event::EndFile(_))) => break,
                Some(Err(e)) => panic!("mpv failed: {e}"),
                _ => {}
            }
        }
        drop(mpv);
        png_size(&out.join("00000001.png"))
    }

    #[test]
    fn real_mpv_takes_the_text_and_crops_like_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let clip = turned_clip(dir.path());
        // Without filters mpv turns the picture itself.
        assert_eq!(
            first_frame(&clip, &dir.path().join("a"), VideoFilter::default()),
            (36, 64)
        );
        // The preview turns the source first (as ffmpeg does), so the crop takes 10 columns of the *turned* frame.
        // Brackets, a semicolon and colons in the graph pass mpv's option parser unescaped.
        let turned_then_cropped = VideoFilter {
            lavfi: "transpose=dir=cclock,crop=w=iw-10:h=ih:x=0:y=0,split[a][b];[a][b]hstack".into(),
            rotates_source: true,
        };
        assert_eq!(first_frame(&clip, &dir.path().join("b"), turned_then_cropped), (52, 64));
        // What happens without the rotation in front: mpv's vf crops the unturned frame (the reason for the above).
        let unturned = VideoFilter {
            lavfi: "crop=w=iw-10:h=ih:x=0:y=0".into(),
            rotates_source: false,
        };
        assert_eq!(first_frame(&clip, &dir.path().join("c"), unturned), (36, 54));
    }
}
