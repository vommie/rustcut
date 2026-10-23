//! The preview's audio filter chain (mpv `af`): muting the last second (against crackling at the end of a file) and loudness
//! normalization. Only the preview is affected; renders build their own filters.

/// Loudness normalization of the preview (PyCutRv's values: quiet, narrow range). `loudnorm` resamples to 192 kHz, so the
/// chain resamples back to 48 kHz for the audio output.
const LOUDNORM: &str = "loudnorm=I=-22:TP=-1.5:LRA=2,aresample=48000";

/// The end mute needs a file longer than this, so a very short clip is never muted completely.
const MIN_DURATION_FOR_END_MUTE: f64 = 2.0;

/// How the preview audio is filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioOptions {
    /// Silence the last second of each file.
    pub mute_last_second: bool,
    /// Normalize the loudness.
    pub loudnorm: bool,
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
    if options.loudnorm {
        filters.push(format!("@rcnorm:lavfi=[{LOUDNORM}]"));
    }
    filters.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: AudioOptions = AudioOptions {
        mute_last_second: true,
        loudnorm: true,
    };

    #[test]
    fn nothing_selected_clears_the_chain() {
        assert_eq!(filter_chain(AudioOptions::default(), Some(60.0)), "");
    }

    #[test]
    fn the_end_mute_needs_a_known_duration() {
        let mute = AudioOptions {
            mute_last_second: true,
            loudnorm: false,
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
