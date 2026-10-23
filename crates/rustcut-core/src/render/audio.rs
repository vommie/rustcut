//! Audio codecs of the render profiles.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    /// PyCutRv's default: transparent at 128 kbit/s stereo, 48 kHz only.
    #[default]
    Opus,
    Aac,
    Mp3,
    Vorbis,
    /// Lossless.
    Flac,
    /// The source's audio unchanged (no re-encode).
    Copy,
}

/// Static facts about one audio codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSpec {
    pub codec: AudioCodec,
    pub ffmpeg_name: &'static str,
    pub display_name: &'static str,
    /// Bit rates offered in the UI (kbit/s, per stream); empty for lossless and copy.
    pub bitrates: &'static [u32],
    pub default_bitrate: Option<u32>,
    /// A sample rate the encoder requires (`-ar`), e.g. 48 kHz for Opus.
    pub required_sample_rate: Option<u32>,
}

const LOSSY_RATES: &[u32] = &[64, 96, 128, 160, 192, 256, 320];

const SPECS: [AudioSpec; 6] = [
    AudioSpec {
        codec: AudioCodec::Opus,
        ffmpeg_name: "libopus",
        display_name: "Opus",
        bitrates: &[48, 64, 96, 128, 160, 192, 256, 320, 512],
        default_bitrate: Some(128),
        required_sample_rate: Some(48_000),
    },
    AudioSpec {
        codec: AudioCodec::Aac,
        ffmpeg_name: "aac",
        display_name: "AAC",
        bitrates: LOSSY_RATES,
        default_bitrate: Some(192),
        required_sample_rate: None,
    },
    AudioSpec {
        codec: AudioCodec::Mp3,
        ffmpeg_name: "libmp3lame",
        display_name: "MP3",
        bitrates: LOSSY_RATES,
        default_bitrate: Some(192),
        required_sample_rate: None,
    },
    AudioSpec {
        codec: AudioCodec::Vorbis,
        ffmpeg_name: "libvorbis",
        display_name: "Vorbis",
        bitrates: LOSSY_RATES,
        default_bitrate: Some(160),
        required_sample_rate: None,
    },
    AudioSpec {
        codec: AudioCodec::Flac,
        ffmpeg_name: "flac",
        display_name: "FLAC (lossless)",
        bitrates: &[],
        default_bitrate: None,
        required_sample_rate: None,
    },
    AudioSpec {
        codec: AudioCodec::Copy,
        ffmpeg_name: "copy",
        display_name: "Copy (no re-encode)",
        bitrates: &[],
        default_bitrate: None,
        required_sample_rate: None,
    },
];

impl AudioCodec {
    pub const ALL: [Self; 6] = [Self::Opus, Self::Aac, Self::Mp3, Self::Vorbis, Self::Flac, Self::Copy];

    pub const fn spec(self) -> &'static AudioSpec {
        match self {
            Self::Opus => &SPECS[0],
            Self::Aac => &SPECS[1],
            Self::Mp3 => &SPECS[2],
            Self::Vorbis => &SPECS[3],
            Self::Flac => &SPECS[4],
            Self::Copy => &SPECS[5],
        }
    }

    pub const fn takes_bitrate(self) -> bool {
        !matches!(self, Self::Flac | Self::Copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_enum() {
        for codec in AudioCodec::ALL {
            let spec = codec.spec();
            assert_eq!(spec.codec, codec);
            assert_eq!(spec.default_bitrate.is_some(), codec.takes_bitrate());
            if let Some(rate) = spec.default_bitrate {
                assert!(spec.bitrates.contains(&rate));
            }
        }
        assert_eq!(
            AudioCodec::default().spec().default_bitrate,
            Some(128),
            "PyCutRv default"
        );
    }
}
