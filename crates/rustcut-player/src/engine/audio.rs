//! The preview's audio filter chain (mpv `af`): muting the last second (against crackling at the end of a file) and loudness
//! normalization. Only the preview is affected; renders build their own filters. Changing `af` while a video plays works
//! without a stop (measured, P12 5.3: at most 35 ms of silence in eight changes), and no filter here makes a seek slower.

/// How the preview evens out its loudness. Each kind keeps its filter's own level (user decision Q-P12d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Normalization {
    #[default]
    None,
    Loudness,
    Dynamic,
    Speech,
    Compressor,
}

impl Normalization {
    /// The libavfilter graph. Loudness has PyCutRv's values (quiet, narrow range); `loudnorm` resamples to 192 kHz, so the
    /// chain resamples back to 48 kHz for the audio output. Dynamic keeps `dynaudnorm`'s look-ahead short and its gain
    /// at 8x, Compressor is a 4:1 "night mode" that reacts in 5 ms.
    const fn graph(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Loudness => Some("loudnorm=I=-22:TP=-1.5:LRA=2,aresample=48000"),
            Self::Dynamic => Some("dynaudnorm=f=250:g=11:p=0.9:m=8"),
            Self::Speech => Some("speechnorm=e=6.25:r=0.0001:l=1"),
            Self::Compressor => Some("acompressor=threshold=-24dB:ratio=4:attack=5:release=250:makeup=6dB"),
        }
    }
}

/// The end mute needs a file longer than this, so a very short clip is never muted completely.
const MIN_DURATION_FOR_END_MUTE: f64 = 2.0;

/// How the preview audio is filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioOptions {
    /// Silence the last second of each file.
    pub mute_last_second: bool,
    /// Even out the loudness.
    pub normalization: Normalization,
}

/// The value of mpv's `af` property for `options` and the current file's `duration` in seconds (unknown before the file is
/// loaded). The end mute comes first: it compares the decoded frames' timestamps (`t`) with the duration, and `loudnorm`
/// does not keep the source's timestamps.
pub(crate) fn filter_chain(options: AudioOptions, duration: Option<f64>) -> String {
    let mut filters = Vec::new();
    if options.mute_last_second
        && let Some(duration) = duration.filter(|d| d.is_finite() && *d > MIN_DURATION_FOR_END_MUTE)
    {
        let from = duration - 1.0;
        filters.push(format!("@rcend:lavfi=[volume=volume=0:enable='gte(t,{from:.3})']"));
    }
    if let Some(graph) = options.normalization.graph() {
        filters.push(format!("@rcnorm:lavfi=[{graph}]"));
    }
    filters.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: AudioOptions = AudioOptions {
        mute_last_second: true,
        normalization: Normalization::Loudness,
    };

    #[test]
    fn nothing_selected_clears_the_chain() {
        assert_eq!(filter_chain(AudioOptions::default(), Some(60.0)), "");
    }

    #[test]
    fn the_end_mute_needs_a_known_duration() {
        let mute = AudioOptions {
            mute_last_second: true,
            normalization: Normalization::None,
        };
        assert_eq!(filter_chain(mute, None), "");
        assert_eq!(filter_chain(mute, Some(f64::NAN)), "");
        assert_eq!(filter_chain(mute, Some(1.5)), "");
        assert_eq!(
            filter_chain(mute, Some(20.0)),
            "@rcend:lavfi=[volume=volume=0:enable='gte(t,19.000)']"
        );
    }

    #[test]
    fn every_kind_has_a_named_filter() {
        for (kind, name) in [
            (Normalization::Loudness, "loudnorm="),
            (Normalization::Dynamic, "dynaudnorm="),
            (Normalization::Speech, "speechnorm="),
            (Normalization::Compressor, "acompressor="),
        ] {
            let options = AudioOptions {
                mute_last_second: false,
                normalization: kind,
            };
            let chain = filter_chain(options, None);
            assert!(chain.starts_with(&format!("@rcnorm:lavfi=[{name}")), "{chain}");
        }
    }

    #[test]
    fn the_end_mute_comes_before_loudnorm() {
        assert_eq!(
            filter_chain(BOTH, Some(90.5)),
            "@rcend:lavfi=[volume=volume=0:enable='gte(t,89.500)'],\
             @rcnorm:lavfi=[loudnorm=I=-22:TP=-1.5:LRA=2,aresample=48000]"
        );
        assert_eq!(
            filter_chain(BOTH, None),
            "@rcnorm:lavfi=[loudnorm=I=-22:TP=-1.5:LRA=2,aresample=48000]"
        );
    }
}
