//! Rendering (ADR-3, ADR-9, ADR-14): the encoder catalog, containers, audio codecs, render profiles and their ffmpeg
//! arguments; the pure command builder of a job; the supervised ffmpeg runner with progress, pause, cancel and priorities;
//! the verification and the final rename; and the encoder capability check.

mod args;
mod audio;
mod builtin;
mod capability;
mod command;
mod container;
mod encoder;
mod engine;
mod output;
mod profile;
mod progress;
mod runner;
mod verify;

pub use args::{ArgsError, EncodeContext, VideoPass, audio_args, profile_args, video_args};
pub use audio::{AudioCodec, AudioSpec};
pub use builtin::{DEFAULT_PROFILE_ID, builtin_profiles, is_builtin_id};
pub use capability::{
    Availability, Capabilities, EncoderStatus, TEST_TIMEOUT, detect_capabilities, has_vidstab, parse_encoder_list,
    spawn_capability_check, test_args,
};
pub use command::{
    Expectation, HardwareContext, PassKind, PlanError, PlanInput, PlannedPass, RenderPlan, TEMP_MARKER, WorkDirs, plan,
    target_file_name, temp_path,
};
pub use container::{CompatIssue, Container, Severity};
pub use encoder::{CodecFamily, EncoderBackend, EncoderSpec, Preset, QualityScale, TuneTarget, VideoEncoder};
pub use engine::{RenderEnv, RenderError, RenderUpdate, RenderedFile, estimate_eta, render_job};
pub use output::{FinalizeError, Leftovers, finalize};
pub use profile::{
    AudioSettings, BitDepth, CodecParam, ProfileField, ProfileIssue, RateControl, RenderProfile, VideoSettings,
};
pub use progress::{ProgressParser, ProgressSample};
pub use runner::{
    Action, Control, GroupSignal, HEAD_LINES, JobLog, MAX_LINE_CHARS, PassError, PassEvent, PassOptions, QUIT_GRACE,
    RenderPriority, StopReason, Supervisor, TAIL_LINES, TERMINATE_GRACE, command_line, run_pass,
};
pub use verify::{MIN_DURATION_TOLERANCE, VerifyError, check as check_render, duration_tolerance, verify_file};
