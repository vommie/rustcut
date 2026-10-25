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

/// Where a bit rate stands on the Codecs panel's slider (P9.10, D-P9i): one step per bit rate the codec offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitrateSlider {
    /// The last position (one fewer than the offered bit rates).
    pub steps: u16,
    pub position: u16,
    /// The bit rate the encoder gets (the codec's default when none is set).
    pub kbps: u32,
    /// The bit rate is one of the offered ones (any other stands at the nearest step).
    pub inside: bool,
}

impl AudioSpec {
    /// The slider for bit rate `kbps` (`None` = the codec's default); `None` for codecs without bit rates.
    pub fn bitrate_slider(&self, kbps: Option<u32>) -> Option<BitrateSlider> {
        let kbps = kbps.or(self.default_bitrate)?;
        let last = self.bitrates.len().checked_sub(1)?;
        let (index, nearest) = self
            .bitrates
            .iter()
            .enumerate()
            .min_by_key(|(_, offered)| offered.abs_diff(kbps))?;
        Some(BitrateSlider {
            steps: u16::try_from(last).unwrap_or(u16::MAX),
            position: u16::try_from(index).unwrap_or(u16::MAX),
            kbps,
            inside: *nearest == kbps,
        })
    }

    /// The bit rate at slider `position` (clamped to the slider); `None` for codecs without bit rates. Always the
    /// number, also at the default's step, as the built-in profiles store it (a step and back is no change).
    pub fn bitrate_at(&self, position: u16) -> Option<u32> {
        let last = self.bitrates.len().checked_sub(1)?;
        self.bitrates.get(usize::from(position).min(last)).copied()
    }
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

    #[test]
    fn the_bit_rate_slider_steps_through_the_offered_rates() {
        let aac = AudioCodec::Aac.spec();
        let default = aac.bitrate_slider(None).unwrap();
        assert_eq!(
            (default.steps, default.position, default.kbps, default.inside),
            (6, 4, 192, true)
        );
        assert_eq!(aac.bitrate_at(4), Some(192), "the default's step is the number");
        assert_eq!(
            (aac.bitrate_at(0), aac.bitrate_at(6), aac.bitrate_at(99)),
            (Some(64), Some(320), Some(320))
        );
        let odd = aac.bitrate_slider(Some(100)).unwrap();
        assert_eq!(
            (odd.position, odd.kbps, odd.inside),
            (1, 100, false),
            "the nearest step, marked outside"
        );
        for codec in AudioCodec::ALL {
            let spec = codec.spec();
            assert_eq!(spec.bitrate_slider(None).is_some(), codec.takes_bitrate());
            for (i, &kbps) in spec.bitrates.iter().enumerate() {
                let at = u16::try_from(i).unwrap();
                assert_eq!(spec.bitrate_at(at), Some(kbps));
                assert_eq!(
                    spec.bitrate_slider(Some(kbps)).map(|s| (s.position, s.inside)),
                    Some((at, true))
                );
            }
        }
        assert_eq!(AudioCodec::Flac.spec().bitrate_at(0), None);
    }
}
