//! Rendering (ADR-3, ADR-9, ADR-14): the encoder catalog, containers, audio codecs, render profiles and their ffmpeg
//! arguments; the pure command builder of a job; the supervised ffmpeg runner with progress, pause, cancel and priorities;
//! the verification and the final rename; the encoder capability check; the simple view over a profile, the option help,
//! the profile library with the panel's working copy, the check of a profile by the real encoder, the size estimate and
//! the probe render.

mod args;
mod audio;
mod audio_copy;
mod builtin;
mod capability;
mod check;
mod color;
mod command;
mod container;
mod encoder;
mod engine;
mod estimate;
mod help;
mod library;
mod output;
mod probe;
mod profile;
mod progress;
mod runner;
mod simple;
mod validate;
mod verify;

pub use args::{ArgsError, EncodeContext, VideoPass, audio_args, profile_args, video_args};
pub use audio::{AudioCodec, AudioSpec, BitrateSlider};
pub use audio_copy::{AudioCopyProblem, audio_copy_problem};
pub use builtin::{DEFAULT_PROFILE_ID, builtin_profiles, is_builtin_id};
pub use capability::{
    Availability, Capabilities, EncoderStatus, TEST_TIMEOUT, detect_capabilities, has_vidstab, parse_encoder_list,
    spawn_capability_check, test_args,
};
pub use check::{check_args, check_profile, read_report};
pub use color::{ColorRange, ColorSettings, ColorTags};
pub use command::{
    Expectation, HardwareContext, PassKind, PlanError, PlanInput, PlannedPass, RenderPlan, TEMP_MARKER, WorkDirs, plan,
    target_file_name, temp_path,
};
pub use container::{CompatIssue, Container, Severity};
pub use encoder::{
    AqSpec, CodecFamily, EncoderBackend, EncoderSpec, Preset, QualityScale, Setting, TuneTarget, VideoEncoder,
};
pub use engine::{RenderEnv, RenderError, RenderUpdate, RenderedFile, estimate_eta, render_job};
pub use estimate::{ESTIMATE_AFTER, estimate_size, scale_size};
pub use help::{OptionHelp, OptionId};
pub use library::{LibraryError, ProfileLibrary, Selection};
pub(crate) use output::rename_no_replace;
pub use output::{FinalizeError, Leftovers, finalize};
pub use probe::{
    DEFAULT_PROBE_SECS, PROBE_JOB, PROBE_SECS, ProbeInput, ProbeJob, clear_probe_dir, probe_dir, probe_env, probe_job,
    probe_profile, probe_window,
};
pub use profile::{
    AudioSettings, BitDepth, CodecParam, ProfileField, ProfileIssue, RateControl, RenderProfile, VideoSettings,
};
pub use progress::{ProgressParser, ProgressSample};
pub use runner::{
    Action, Control, GroupSignal, HEAD_LINES, JobLog, MAX_LINE_CHARS, PassError, PassEvent, PassOptions, QUIT_GRACE,
    RenderPriority, StopReason, Supervisor, TAIL_LINES, TERMINATE_GRACE, command_line, run_pass, stop_orphans,
};
pub use simple::{QualityBand, QualitySlider, Speed};
pub use validate::{MAX_FILM_GRAIN, MAX_VARIANCE_BOOST, issues_of};
pub use verify::{MIN_DURATION_TOLERANCE, VerifyError, check as check_render, duration_tolerance, verify_file};
