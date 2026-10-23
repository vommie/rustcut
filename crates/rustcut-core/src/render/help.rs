//! What every render option does and what it costs (ADR-14: each option has a tooltip saying both), and which
//! encoders have it. One table, used by the Render Settings panel now and the profile editor of the Settings window later.

use super::{EncoderSpec, VideoEncoder};

/// The options of the simple and the expert view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionId {
    Profile,
    Encoder,
    Quality,
    Speed,
    Container,
    AudioCodec,
    AudioBitrate,
    AudioChannels,
    AudioSampleRate,
    RateControl,
    ConstantQp,
    Bitrate,
    TargetSize,
    Preset,
    Tune,
    CodecProfile,
    Level,
    BitDepth,
    Keyint,
    FilmGrain,
    VarianceBoost,
    AqMode,
    SceneCuts,
    ColorTags,
    ColorRange,
    CodecParams,
    ProbeRender,
    ProbeLength,
    ProbeExact,
}

/// The name and the tooltip of an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionHelp {
    pub label: &'static str,
    /// What it does, then what it costs.
    pub tooltip: &'static str,
}

const fn help(label: &'static str, tooltip: &'static str) -> OptionHelp {
    OptionHelp { label, tooltip }
}

impl OptionId {
    pub const fn help(self) -> OptionHelp {
        match self {
            Self::Profile => help(
                "Profile",
                "A named set of encoding settings. Changes apply to a working copy marked \"modified\"; save them as a \
                 new profile or into your own profile. Built-in profiles never change.",
            ),
            Self::Encoder => help(
                "Codec",
                "The video encoder. AV1 makes the smallest files but encodes slowest; H.265 is about 30-50 % smaller \
                 than H.264 at the same look; H.264 plays everywhere. Only encoders that work on this computer are \
                 offered.",
            ),
            Self::Quality => help(
                "Quality",
                "Constant quality: the encoder spends as many bits as each scene needs to look this good. Further \
                 right = better picture and larger files (roughly double the size every few steps). The same position \
                 looks about the same with every codec.",
            ),
            Self::Speed => help(
                "Speed",
                "How hard the encoder searches for savings. Slower speeds make smaller files at the same quality but \
                 take much longer (often two to four times per step); the picture quality itself stays about the same.",
            ),
            Self::Container => help(
                "Container",
                "The file format that holds video and audio. MKV takes every codec; MP4 plays on most devices and \
                 phones; WebM is for the web (AV1 + Opus only). Costs nothing.",
            ),
            Self::AudioCodec => help(
                "Audio",
                "The audio encoder for every audio track. Opus is the best at small sizes; AAC plays everywhere; FLAC \
                 is lossless and large; Copy keeps the source's audio untouched (fast, but must fit the container).",
            ),
            Self::AudioBitrate => help(
                "Bit rate",
                "Bits per second for each audio track. Higher = closer to the source and larger files; Opus is \
                 transparent for stereo at about 128 kbit/s, AAC at about 192 kbit/s.",
            ),
            Self::AudioChannels => help(
                "Channels",
                "Keep the source's channel layout or mix down (e.g. 5.1 to stereo). Fewer channels need fewer bits; a \
                 downmix cannot be undone.",
            ),
            Self::AudioSampleRate => help(
                "Sample rate",
                "Keep the source's sample rate or resample. Resampling costs a little time and never adds detail; \
                 Opus always uses 48 kHz.",
            ),
            Self::RateControl => help(
                "Rate control",
                "How the encoder decides the bit rate. Constant quality (CRF) gives the best result per byte with an \
                 unknown size; constant QP is predictable but larger; an average bit rate fixes the size roughly; two \
                 passes or a target size hit it exactly at twice the encoding time.",
            ),
            Self::ConstantQp => help(
                "QP",
                "The same quantizer for every frame, whatever it shows. Lower = better and larger. Wastes bits on easy \
                 scenes compared with constant quality; mostly for tests and special workflows.",
            ),
            Self::Bitrate => help(
                "Bit rate",
                "The average video bit rate in kbit/s. Predictable size, but simple scenes get more and complex ones \
                 fewer bits than they need; with two passes the bits are spread better at twice the encoding time.",
            ),
            Self::TargetSize => help(
                "Target size",
                "The size of the whole output file in MiB; the video bit rate follows from the length of the sections \
                 minus the audio. Always two passes: twice the encoding time.",
            ),
            Self::Preset => help(
                "Preset",
                "The encoder's own speed preset (the full list). Slower presets compress better at the same quality \
                 and take much longer.",
            ),
            Self::Tune => help(
                "Tune",
                "Adjusts the encoder for a kind of content (film, animation, grain, visual quality). Costs nothing in \
                 time; the wrong tune can cost quality.",
            ),
            Self::CodecProfile => help(
                "Profile",
                "The codec profile limits the tools the encoder may use so older devices can decode the file. \
                 Restrictive profiles (baseline, main) make files somewhat larger.",
            ),
            Self::Level => help(
                "Level",
                "The codec level caps resolution, frame rate and bit rate for hardware decoders (e.g. 4.1 for many TVs). \
                 Only needed for picky devices; too low a level makes the encoder fail or lower quality.",
            ),
            Self::BitDepth => help(
                "Bit depth",
                "10-bit avoids banding in gradients and skies and often makes AV1/H.265 files a little smaller. Costs \
                 some encoding time; 10-bit H.264 plays on few devices.",
            ),
            Self::Keyint => help(
                "Keyframe interval",
                "Most frames between two keyframes (GOP length; empty = the encoder's choice, usually about 10 s). \
                 Shorter = faster seeking and more robust files, but larger.",
            ),
            Self::FilmGrain => help(
                "Film grain",
                "SVT-AV1 removes the grain and the player adds synthetic grain back (0-50). Saves a lot of bits on \
                 grainy sources; costs encoding time, and too much looks artificial on clean video.",
            ),
            Self::VarianceBoost => help(
                "Variance boost",
                "SVT-AV1 gives low-contrast areas (dark scenes, skies, skin) more bits (strength 1-4). Better detail \
                 there for slightly larger files.",
            ),
            Self::AqMode => help(
                "AQ mode",
                "Adaptive quantization moves bits between flat and detailed parts of a frame. Auto-variance modes \
                 usually look best; the dark-scene variant helps against blocking in shadows. Costs little time.",
            ),
            Self::SceneCuts => help(
                "Keyframes at scene cuts",
                "Place extra keyframes where the scene changes (the default). Off = keyframes only at the fixed \
                 interval: more predictable for editing, slightly worse quality right after cuts.",
            ),
            Self::ColorTags => help(
                "Color tags",
                "The color description written into the file (primaries, transfer, matrix). \"As the source\" keeps \
                 the source's, including HDR. Only a label: wrong tags make players show wrong colors; costs nothing.",
            ),
            Self::ColorRange => help(
                "Color range",
                "Whether black and white sit at 16/235 (limited, nearly all video) or 0/255 (full). Only a label for \
                 the player; costs nothing.",
            ),
            Self::CodecParams => help(
                "Codec parameters",
                "Extra key=value pairs for the encoder, separated by \":\" (passed to -x264-params, -x265-params, \
                 -svtav1-params, ...). Checked with the real encoder in the background; unknown keys are reported.",
            ),
            Self::ProbeRender => help(
                "Probe render",
                "Renders a few seconds around the playhead with the current filters and settings and plays them in the \
                 player, the only realistic preview of deshake. Takes a few seconds; Esc returns to the source.",
            ),
            Self::ProbeLength => help(
                "Probe length",
                "Seconds of the probe render (a quarter of it before the playhead). Longer probes show more and take \
                 longer.",
            ),
            Self::ProbeExact => help(
                "Exact settings",
                "Probe with the profile's own speed instead of the fastest one. Shows exactly what the render will look \
                 like and estimates the job's file size, but takes as long per second as the real render.",
            ),
        }
    }

    /// Whether `encoder` has this option (options that do not depend on the encoder: always).
    pub fn applies_to(self, encoder: VideoEncoder) -> bool {
        let spec: &EncoderSpec = encoder.spec();
        match self {
            Self::Speed => spec.speeds.is_some(),
            Self::Preset => !spec.presets.is_empty(),
            Self::Tune => !spec.tunes.is_empty(),
            Self::CodecProfile => !spec.profiles.is_empty(),
            Self::Level => encoder.level_param().is_some(),
            Self::BitDepth => spec.pix_fmt_10.is_some(),
            Self::ConstantQp => spec.constant_qp.is_some(),
            Self::Bitrate => spec.average_bitrate,
            Self::TargetSize => spec.two_pass,
            Self::FilmGrain => spec.film_grain,
            Self::VarianceBoost => spec.variance_boost,
            Self::AqMode => spec.aq.is_some(),
            Self::SceneCuts => spec.scene_cuts,
            Self::CodecParams => spec.params_option.is_some(),
            Self::Profile
            | Self::Encoder
            | Self::Quality
            | Self::Container
            | Self::AudioCodec
            | Self::AudioBitrate
            | Self::AudioChannels
            | Self::AudioSampleRate
            | Self::RateControl
            | Self::Keyint
            | Self::ColorTags
            | Self::ColorRange
            | Self::ProbeRender
            | Self::ProbeLength
            | Self::ProbeExact => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_explains_itself() {
        let all = [
            OptionId::Profile,
            OptionId::Encoder,
            OptionId::Quality,
            OptionId::Speed,
            OptionId::Container,
            OptionId::AudioCodec,
            OptionId::AudioBitrate,
            OptionId::AudioChannels,
            OptionId::AudioSampleRate,
            OptionId::RateControl,
            OptionId::ConstantQp,
            OptionId::Bitrate,
            OptionId::TargetSize,
            OptionId::Preset,
            OptionId::Tune,
            OptionId::CodecProfile,
            OptionId::Level,
            OptionId::BitDepth,
            OptionId::Keyint,
            OptionId::FilmGrain,
            OptionId::VarianceBoost,
            OptionId::AqMode,
            OptionId::SceneCuts,
            OptionId::ColorTags,
            OptionId::ColorRange,
            OptionId::CodecParams,
            OptionId::ProbeRender,
            OptionId::ProbeLength,
            OptionId::ProbeExact,
        ];
        for id in all {
            let h = id.help();
            assert!(!h.label.is_empty(), "{id:?}");
            assert!(h.tooltip.len() > 60, "{id:?}: say what it does and what it costs");
        }
        assert!(OptionId::FilmGrain.applies_to(VideoEncoder::SvtAv1));
        assert!(!OptionId::FilmGrain.applies_to(VideoEncoder::X264));
        assert!(OptionId::SceneCuts.applies_to(VideoEncoder::X265));
        assert!(!OptionId::Speed.applies_to(VideoEncoder::HevcVaapi));
        assert!(OptionId::Level.applies_to(VideoEncoder::X264));
    }
}
