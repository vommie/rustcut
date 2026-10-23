//! Encoder arguments of a render profile (pure; the runner adds inputs, the filter graph, maps and the output path).

use std::path::Path;

use super::{
    AudioCodec, AudioSettings, BitDepth, EncoderBackend, RateControl, RenderProfile, Setting, TuneTarget, VideoEncoder,
    VideoSettings,
};
use crate::time::Timestamp;

/// Which encode pass the arguments are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPass<'a> {
    /// One pass (constant quality or average bit rate).
    Single,
    /// First of two passes; `log` is the stats file prefix (the output goes to `-f null`).
    First { log: &'a Path },
    /// Second of two passes, reading the stats of the first.
    Second { log: &'a Path },
}

/// Facts about the job that some settings need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeContext<'a> {
    /// Length of the output (sum of the sections); needed for a target size.
    pub duration: Timestamp,
    /// Number of audio streams in the output (their bit rate counts against a target size).
    pub audio_streams: usize,
    pub pass: VideoPass<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgsError {
    #[error("a target size needs an output duration above zero")]
    NoDuration,
    #[error("the target size of {mebibytes} MiB leaves no bit rate for the video (audio needs {audio_kbps} kbit/s)")]
    TargetTooSmall { mebibytes: u32, audio_kbps: u32 },
    #[error("two-pass rate control needs the pass to render (first or second)")]
    PassMissing,
    #[error("the path of the two-pass stats file is not valid UTF-8")]
    NonUtf8Path,
}

impl RateControl {
    /// Whether the render runs two encode passes.
    pub const fn is_two_pass(self) -> bool {
        matches!(self, Self::TwoPass { .. } | Self::TargetSize { .. })
    }
}

impl VideoEncoder {
    /// The codec parameter that carries the level (`level` for x264, `level-idc` for x265); others take none.
    pub const fn level_param(self) -> Option<&'static str> {
        match self {
            Self::X264 => Some("level"),
            Self::X265 => Some("level-idc"),
            _ => None,
        }
    }
}

/// Video encoder arguments: codec, preset, rate control, pixel format, tune, profile, GOP, codec parameters, passes.
pub fn video_args(v: &VideoSettings, ctx: &EncodeContext<'_>, audio: &AudioSettings) -> Result<Vec<String>, ArgsError> {
    let spec = v.encoder.spec();
    let mut args: Vec<String> = vec!["-c:v".into(), spec.ffmpeg_name.into()];
    if let (Some(option), Some(preset)) = (spec.preset_option, v.preset.as_deref().or(spec.default_preset)) {
        args.extend([option.to_owned(), preset.to_owned()]);
    }
    match v.rate_control {
        RateControl::Quality { value } => {
            args.extend([spec.quality.option.to_owned(), value.to_string()]);
            args.extend(spec.quality.extra.iter().map(|s| (*s).to_owned()));
        }
        RateControl::ConstantQp { qp } => {
            // Validation refuses encoders without a QP mode; their quality option is the closest meaning.
            let scale = spec.constant_qp.unwrap_or(spec.quality);
            args.extend([scale.option.to_owned(), qp.to_string()]);
            args.extend(scale.extra.iter().map(|s| (*s).to_owned()));
        }
        RateControl::AverageBitrate { kbps } | RateControl::TwoPass { kbps } => {
            args.extend(["-b:v".into(), format!("{kbps}k")]);
        }
        RateControl::TargetSize { mebibytes } => {
            let kbps = target_kbps(mebibytes, ctx, audio)?;
            args.extend(["-b:v".into(), format!("{kbps}k")]);
        }
    }
    let pix_fmt = match v.bit_depth {
        BitDepth::Eight => spec.pix_fmt_8,
        BitDepth::Ten => spec.pix_fmt_10.unwrap_or(spec.pix_fmt_8),
    };
    if spec.backend != EncoderBackend::Vaapi {
        args.extend(["-pix_fmt".into(), pix_fmt.into()]);
    }
    let mut params: Vec<String> = Vec::new();
    if let Some(tune) = &v.tune {
        match spec.tune_target {
            TuneTarget::Option => args.extend(["-tune".into(), tune.clone()]),
            TuneTarget::CodecParam(key) => params.push(format!("{key}={tune}")),
        }
    }
    if let Some(profile) = &v.profile {
        args.extend(["-profile:v".into(), profile.clone()]);
    }
    if let (Some(level), Some(key)) = (&v.level, v.encoder.level_param()) {
        params.push(format!("{key}={level}"));
    }
    if let Some(keyint) = v.keyint {
        args.extend(["-g".into(), keyint.to_string()]);
    }
    if let Some(grain) = v.film_grain {
        params.push(format!("film-grain={grain}"));
    }
    if let (Some(boost), true) = (v.variance_boost, spec.variance_boost) {
        params.push(format!("enable-variance-boost={}", u8::from(boost > 0)));
        if boost > 0 {
            params.push(format!("variance-boost-strength={boost}"));
        }
    }
    if let (Some(mode), Some(aq)) = (&v.aq_mode, spec.aq) {
        match aq.setting {
            Setting::Option(option) => args.extend([option.to_owned(), mode.clone()]),
            Setting::Param(key) => params.push(format!("{key}={mode}")),
        }
    }
    if v.scene_cuts == Some(false) && spec.scene_cuts {
        params.push("scenecut=0".to_owned());
    }
    args.extend(v.color.args());
    params.extend(v.codec_params.iter().map(|p| format!("{}={}", p.key, p.value)));
    if v.rate_control.is_two_pass() {
        let (number, log) = match ctx.pass {
            VideoPass::First { log } => (1, log),
            VideoPass::Second { log } => (2, log),
            VideoPass::Single => return Err(ArgsError::PassMissing),
        };
        let log = log.to_str().ok_or(ArgsError::NonUtf8Path)?;
        if v.encoder == VideoEncoder::X265 {
            // libx265 ignores ffmpeg's generic pass flags; it takes them as its own parameters.
            params.push(format!("pass={number}"));
            params.push(format!("stats={}", escape_dict_value(log)));
        } else {
            args.extend([
                "-pass".into(),
                number.to_string(),
                "-passlogfile".into(),
                log.to_owned(),
            ]);
        }
    }
    if let (Some(option), false) = (spec.params_option, params.is_empty()) {
        args.extend([option.to_owned(), params.join(":")]);
    }
    Ok(args)
}

/// Audio arguments, applied to every audio stream of the output.
pub fn audio_args(a: &AudioSettings) -> Vec<String> {
    let spec = a.codec.spec();
    let mut args: Vec<String> = vec!["-c:a".into(), spec.ffmpeg_name.into()];
    if a.codec == AudioCodec::Copy {
        return args;
    }
    if let Some(kbps) = a
        .bitrate_kbps
        .filter(|&k| k > 0 && a.codec.takes_bitrate())
        .or(spec.default_bitrate)
    {
        args.extend(["-b:a".into(), format!("{kbps}k")]);
    }
    if let Some(rate) = spec.required_sample_rate.or(a.sample_rate) {
        args.extend(["-ar".into(), rate.to_string()]);
    }
    if let Some(channels) = a.channels.filter(|&c| c > 0) {
        args.extend(["-ac".into(), channels.to_string()]);
    }
    args
}

/// Encoder and muxer arguments of a whole profile for the final pass.
pub fn profile_args(p: &RenderProfile, ctx: &EncodeContext<'_>) -> Result<Vec<String>, ArgsError> {
    let mut args = video_args(&p.video, ctx, &p.audio)?;
    if ctx.audio_streams > 0 {
        args.extend(audio_args(&p.audio));
    }
    if matches!(p.container, super::Container::Mp4 | super::Container::Mov) {
        // The index goes to the front, so the file starts playing before it is fully downloaded or read.
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-f".into(), p.container.muxer().into()]);
    Ok(args)
}

pub(super) fn target_kbps(mebibytes: u32, ctx: &EncodeContext<'_>, audio: &AudioSettings) -> Result<u64, ArgsError> {
    let micros = u64::try_from(ctx.duration.as_micros())
        .ok()
        .filter(|&us| us > 0)
        .ok_or(ArgsError::NoDuration)?;
    let per_stream = if audio.codec.takes_bitrate() {
        audio.bitrate_kbps.or(audio.codec.spec().default_bitrate).unwrap_or(0)
    } else {
        // Lossless and copied audio: assume a typical rate so the video budget is not overestimated.
        256
    };
    let audio_kbps = u64::from(per_stream) * u64::try_from(ctx.audio_streams).unwrap_or(u64::MAX);
    // kbit/s = MiB * 8 * 1024 * 1024 / 1000 / seconds; computed in microseconds to stay exact.
    let total_kbps = u64::from(mebibytes) * 8 * 1024 * 1024 * 1000 / micros;
    total_kbps
        .checked_sub(audio_kbps)
        .filter(|&k| k > 0)
        .ok_or(ArgsError::TargetTooSmall {
            mebibytes,
            audio_kbps: u32::try_from(audio_kbps).unwrap_or(u32::MAX),
        })
}

/// Escapes a value inside an ffmpeg `key=value:...` dictionary option.
fn escape_dict_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | ':' | '=' | '\'') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::render::{AudioCodec, CodecParam, Container, builtin_profiles};

    fn single() -> EncodeContext<'static> {
        EncodeContext {
            duration: Timestamp::from_secs(60),
            audio_streams: 1,
            pass: VideoPass::Single,
        }
    }

    #[test]
    fn built_in_profiles() {
        let lines: Vec<String> = builtin_profiles()
            .iter()
            .map(|p| format!("{}: {}", p.id, profile_args(p, &single()).unwrap().join(" ")))
            .collect();
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn two_pass_for_each_software_encoder() {
        let log = PathBuf::from("/data/rustcut/passlog/job-3: try");
        let mut lines = Vec::new();
        for encoder in [VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::Aom] {
            let mut p = builtin_profiles().remove(0);
            p.video = VideoSettings {
                rate_control: RateControl::TwoPass { kbps: 2500 },
                keyint: Some(240),
                ..VideoSettings::new(encoder)
            };
            p.container = Container::Mkv;
            for pass in [VideoPass::First { log: &log }, VideoPass::Second { log: &log }] {
                let ctx = EncodeContext { pass, ..single() };
                lines.push(video_args(&p.video, &ctx, &p.audio).unwrap().join(" "));
            }
            let missing = video_args(&p.video, &single(), &p.audio);
            assert_eq!(missing, Err(ArgsError::PassMissing));
        }
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn target_size_budget() {
        let audio = AudioSettings {
            codec: AudioCodec::Opus,
            bitrate_kbps: Some(128),
            channels: None,
            sample_rate: None,
        };
        let ctx = |secs, streams| EncodeContext {
            duration: Timestamp::from_secs(secs),
            audio_streams: streams,
            pass: VideoPass::Single,
        };
        // 100 MiB over 100 s = 8388 kbit/s in total, minus 128 for one Opus stream.
        assert_eq!(target_kbps(100, &ctx(100, 1), &audio), Ok(8260));
        assert_eq!(target_kbps(100, &ctx(0, 1), &audio), Err(ArgsError::NoDuration));
        assert!(matches!(
            target_kbps(1, &ctx(3600, 2), &audio),
            Err(ArgsError::TargetTooSmall { .. })
        ));
    }

    #[test]
    fn audio_variants() {
        let a = |codec, bitrate_kbps, channels, sample_rate| {
            audio_args(&AudioSettings {
                codec,
                bitrate_kbps,
                channels,
                sample_rate,
            })
            .join(" ")
        };
        assert_eq!(
            a(AudioCodec::Opus, None, Some(2), Some(44_100)),
            "-c:a libopus -b:a 128k -ar 48000 -ac 2"
        );
        assert_eq!(a(AudioCodec::Aac, Some(256), None, None), "-c:a aac -b:a 256k");
        assert_eq!(a(AudioCodec::Flac, Some(256), Some(2), None), "-c:a flac -ac 2");
        assert_eq!(
            a(AudioCodec::Flac, None, None, Some(44_100)),
            "-c:a flac -ar 44100",
            "lossless audio can still be resampled"
        );
        assert_eq!(a(AudioCodec::Copy, Some(128), Some(2), Some(44_100)), "-c:a copy");
        assert_eq!(a(AudioCodec::Copy, None, None, None), "-c:a copy");
    }

    #[test]
    fn hardware_quality_options() {
        let mut lines = Vec::new();
        for encoder in [VideoEncoder::HevcVaapi, VideoEncoder::Av1Nvenc, VideoEncoder::H264Qsv] {
            let spec = encoder.spec();
            let v = VideoSettings {
                rate_control: RateControl::Quality {
                    value: spec.quality.default,
                },
                bit_depth: BitDepth::Ten,
                codec_params: vec![CodecParam::new("ignored", "1")],
                ..VideoSettings::new(encoder)
            };
            lines.push(video_args(&v, &single(), &AudioSettings::default()).unwrap().join(" "));
        }
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn expert_options() {
        use crate::render::{ColorRange, ColorSettings, ColorTags};
        let mut lines = Vec::new();
        for encoder in [
            VideoEncoder::X264,
            VideoEncoder::X265,
            VideoEncoder::SvtAv1,
            VideoEncoder::Aom,
        ] {
            let spec = encoder.spec();
            let v = VideoSettings {
                rate_control: spec.constant_qp.map_or(RateControl::Quality { value: 30 }, |q| {
                    RateControl::ConstantQp { qp: q.default }
                }),
                aq_mode: spec.aq.and_then(|aq| aq.modes.last()).map(|m| m.value.to_owned()),
                scene_cuts: spec.scene_cuts.then_some(false),
                variance_boost: spec.variance_boost.then_some(3),
                color: ColorSettings {
                    tags: ColorTags::Bt709,
                    range: ColorRange::Limited,
                },
                ..VideoSettings::new(encoder)
            };
            lines.push(video_args(&v, &single(), &AudioSettings::default()).unwrap().join(" "));
        }
        let off = VideoSettings {
            variance_boost: Some(0),
            ..VideoSettings::new(VideoEncoder::SvtAv1)
        };
        lines.push(
            video_args(&off, &single(), &AudioSettings::default())
                .unwrap()
                .join(" "),
        );
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn dictionary_escaping() {
        assert_eq!(escape_dict_value("/a:b=c"), r"/a\:b\=c");
    }
}
