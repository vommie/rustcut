//! Color metadata of the output (ADR-14): by default ffmpeg passes the source's tags on (primaries, transfer, matrix,
//! range); a profile can tag the output differently. Tags only describe the picture, they never convert it.

use serde::{Deserialize, Serialize};

/// Which color description the output carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTags {
    /// Whatever the source says (also HDR descriptions such as PQ or HLG).
    #[default]
    Source,
    /// HD video (BT.709).
    Bt709,
    /// SD video from PAL/SECAM (BT.601 625 lines).
    Bt601Pal,
    /// SD video from NTSC (BT.601 525 lines, SMPTE 170M).
    Bt601Ntsc,
}

/// The value range of the output's samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorRange {
    #[default]
    Source,
    /// Limited ("TV", 16-235 for 8 bit): what nearly all video uses.
    Limited,
    /// Full ("PC", 0-255 for 8 bit).
    Full,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorSettings {
    pub tags: ColorTags,
    pub range: ColorRange,
}

impl ColorTags {
    pub const ALL: [Self; 4] = [Self::Source, Self::Bt709, Self::Bt601Pal, Self::Bt601Ntsc];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Source => "As the source",
            Self::Bt709 => "BT.709 (HD)",
            Self::Bt601Pal => "BT.601 PAL (SD)",
            Self::Bt601Ntsc => "BT.601 NTSC (SD)",
        }
    }

    /// `(primaries, transfer, matrix)` as ffmpeg names them; `None` keeps the source's.
    pub const fn ffmpeg_values(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Self::Source => None,
            Self::Bt709 => Some(("bt709", "bt709", "bt709")),
            Self::Bt601Pal => Some(("bt470bg", "bt709", "bt470bg")),
            Self::Bt601Ntsc => Some(("smpte170m", "smpte170m", "smpte170m")),
        }
    }
}

impl ColorRange {
    pub const ALL: [Self; 3] = [Self::Source, Self::Limited, Self::Full];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Source => "As the source",
            Self::Limited => "Limited (TV)",
            Self::Full => "Full (PC)",
        }
    }

    const fn ffmpeg_value(self) -> Option<&'static str> {
        match self {
            Self::Source => None,
            Self::Limited => Some("tv"),
            Self::Full => Some("pc"),
        }
    }
}

impl ColorSettings {
    /// The ffmpeg options that tag the output (empty = the source's tags pass through).
    pub fn args(self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some((primaries, transfer, matrix)) = self.tags.ffmpeg_values() {
            for (option, value) in [
                ("-color_primaries", primaries),
                ("-color_trc", transfer),
                ("-colorspace", matrix),
            ] {
                args.extend([option.to_owned(), value.to_owned()]);
            }
        }
        if let Some(range) = self.range.ffmpeg_value() {
            args.extend(["-color_range".to_owned(), range.to_owned()]);
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_range() {
        assert!(ColorSettings::default().args().is_empty());
        let tagged = ColorSettings {
            tags: ColorTags::Bt709,
            range: ColorRange::Limited,
        };
        assert_eq!(
            tagged.args().join(" "),
            "-color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv"
        );
        let ntsc = ColorSettings {
            tags: ColorTags::Bt601Ntsc,
            range: ColorRange::Source,
        };
        assert_eq!(
            ntsc.args().join(" "),
            "-color_primaries smpte170m -color_trc smpte170m -colorspace smpte170m"
        );
    }
}
