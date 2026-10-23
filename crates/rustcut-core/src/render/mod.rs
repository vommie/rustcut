//! Render planning: the encoder catalog, containers, audio codecs, render profiles and their ffmpeg arguments (ADR-3, ADR-14).
//! The process runner, progress parsing and verification arrive in M2.4.

mod args;
mod audio;
mod builtin;
mod container;
mod encoder;
mod profile;

pub use args::{ArgsError, EncodeContext, VideoPass, audio_args, profile_args, video_args};
pub use audio::{AudioCodec, AudioSpec};
pub use builtin::{DEFAULT_PROFILE_ID, builtin_profiles, is_builtin_id};
pub use container::{CompatIssue, Container, Severity};
pub use encoder::{CodecFamily, EncoderBackend, EncoderSpec, Preset, QualityScale, TuneTarget, VideoEncoder};
pub use profile::{
    AudioSettings, BitDepth, CodecParam, ProfileField, ProfileIssue, RateControl, RenderProfile, VideoSettings,
};
