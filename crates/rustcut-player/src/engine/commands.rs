//! The command thread: every libmpv call except the render calls runs here, never on the UI thread. Slint's UI thread is
//! mpv's render thread, and mpv's `render.h` forbids it the synchronous client API (`mpv_command`, `mpv_set_property`, ...):
//! the core may wait for the renderer while the renderer waits for the core.
//!
//! [`Session`] is the pure part (which mpv operations a command means, given what happened before); [`run`] executes them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use libmpv2::Mpv;

use super::audio::{self, AudioOptions};
use super::options::{self, PreviewQuality};
use super::video::{self, VideoFilter};

/// What the app (and the player's own threads) ask mpv to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Command {
    /// Load a file; `start` in seconds, `play` sets the pause state first (`None` keeps it, e.g. for a reload).
    Open {
        path: PathBuf,
        start: Option<f64>,
        play: Option<bool>,
    },
    SetPaused(bool),
    /// Toggle pause; at the end of the file, playing starts over from the beginning.
    TogglePause,
    /// Jump to `seconds`; `exact` decodes up to the precise frame, otherwise the nearest keyframe is enough (dragging).
    Seek {
        seconds: f64,
        exact: bool,
    },
    FrameStep {
        back: bool,
    },
    /// mpv could not step back from `at` seconds (its "Backstep failed."; from the event thread): the demuxer landed on
    /// the keyframe after the step's target, so no earlier frame was decoded (MPEG-TS, user report 2026-11-03).
    BackstepFailed {
        at: f64,
    },
    SetMuted(bool),
    /// Volume in percent (100 = unchanged).
    SetVolume(f64),
    /// Playback speed (1 = normal).
    SetSpeed(f64),
    /// Play the file in a loop (the probe render) or once.
    SetLoop(bool),
    SetAudio(AudioOptions),
    /// Hardware decoding (`hwdec`; mpv reinitializes the decoder of a playing file).
    SetHwdec(String),
    /// The color around the video (`#RRGGBB`); other text is ignored.
    SetBackground(String),
    /// The preview's video filters.
    SetVideoFilter(VideoFilter),
    /// The loaded file's duration in seconds (from the event thread; the end mute needs it).
    Duration(f64),
    /// mpv's observed `pause` and `eof-reached` (from the event thread; Play at the end starts over). The command
    /// thread never reads a property itself.
    Observed {
        paused: Option<bool>,
        at_end: Option<bool>,
    },
    /// A render context exists now; `software` = the OpenGL renderer is a software rasterizer.
    SurfaceReady {
        software: bool,
    },
    SurfaceLost,
    /// Shut the core down; the thread ends after it.
    Quit,
}

/// One libmpv call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    Command(&'static str, Vec<String>),
    Flag(&'static str, bool),
    Double(&'static str, f64),
    Text(&'static str, String),
}

#[derive(Debug, Clone, PartialEq)]
struct Load {
    path: PathBuf,
    start: Option<f64>,
    play: Option<bool>,
}

/// What the command thread remembers between commands.
#[derive(Debug)]
pub(crate) struct Session {
    surface: bool,
    waiting: Option<Load>,
    quality: PreviewQuality,
    fast_applied: bool,
    audio: AudioOptions,
    duration: Option<f64>,
    /// The `af` value last set, so an unchanged chain does not restart the audio output.
    af: String,
    /// The video filters last set (a change reinitializes mpv's filters and redraws the frame).
    vf: VideoFilter,
    /// mpv's observed `pause` and `eof-reached`.
    paused: bool,
    at_end: bool,
    /// How far before a target mpv's demuxer seeks (`hr-seek-demuxer-offset`, seconds): 0 until stepping back failed in
    /// this file, then raised until it works; a new file starts at 0 again.
    demuxer_offset: f64,
}

impl Session {
    pub(crate) fn new(quality: PreviewQuality, audio: AudioOptions) -> Self {
        Self {
            surface: false,
            waiting: None,
            quality,
            fast_applied: false,
            audio,
            duration: None,
            // The core starts with this chain (`PlayerOptions::apply`).
            af: audio::filter_chain(audio, None),
            vf: VideoFilter::default(),
            paused: true,
            at_end: false,
            demuxer_offset: 0.0,
        }
    }

    /// The mpv operations for `command`, in order.
    pub(crate) fn handle(&mut self, command: Command) -> Vec<Op> {
        match command {
            Command::Open { path, start, play } => {
                let load = Load { path, start, play };
                if self.surface {
                    self.load(load)
                } else {
                    tracing::info!(path = %load.path.display(), "[Player] Waiting for the video surface before loading");
                    self.waiting = Some(load);
                    Vec::new()
                }
            }
            Command::SurfaceReady { software } => {
                self.surface = true;
                let mut ops = Vec::new();
                if self.quality == PreviewQuality::Auto && software && !self.fast_applied {
                    tracing::info!("[Player] Software OpenGL: fast preview scaling");
                    self.fast_applied = true;
                    ops.push(Op::Command("apply-profile", vec![options::FAST_PROFILE.into()]));
                }
                if let Some(load) = self.waiting.take() {
                    tracing::info!(path = %load.path.display(), "[Player] Video surface ready, loading the waiting file");
                    ops.extend(self.load(load));
                }
                ops
            }
            Command::SurfaceLost => {
                self.surface = false;
                Vec::new()
            }
            Command::Duration(seconds) => {
                self.duration = Some(seconds);
                self.audio_ops()
            }
            Command::SetAudio(audio) => {
                self.audio = audio;
                self.audio_ops()
            }
            Command::SetHwdec(value) => vec![Op::Text("hwdec", value)],
            Command::SetBackground(color) if options::is_color(&color) => vec![Op::Text("background-color", color)],
            Command::SetBackground(color) => {
                tracing::warn!(color = %color, "[Player] The background is not a #RRGGBB color; unchanged");
                Vec::new()
            }
            Command::SetVideoFilter(filter) => self.video_ops(filter),
            Command::SetPaused(paused) => vec![Op::Flag("pause", paused)],
            Command::Observed { paused, at_end } => {
                self.paused = paused.unwrap_or(self.paused);
                self.at_end = at_end.unwrap_or(self.at_end);
                Vec::new()
            }
            // mpv toggles itself (`cycle`), so a pause change still on its way cannot be undone by a stale value; at
            // the end (keep-open holds the last frame) playing starts over.
            Command::TogglePause => {
                let mut ops = Vec::new();
                if self.paused && self.at_end {
                    ops.push(Op::Command("seek", vec!["0".into(), "absolute+exact".into()]));
                }
                ops.push(Op::Command("cycle", vec!["pause".into()]));
                ops
            }
            Command::Seek { seconds, exact } => {
                let mode = if exact { "absolute+exact" } else { "absolute+keyframes" };
                vec![Op::Command(
                    "seek",
                    vec![format!("{:.6}", seconds.max(0.0)), mode.into()],
                )]
            }
            Command::FrameStep { back } => {
                vec![Op::Command(
                    if back { "frame-back-step" } else { "frame-step" },
                    Vec::new(),
                )]
            }
            Command::BackstepFailed { at } => self.backstep_failed(at),
            Command::SetMuted(muted) => vec![Op::Flag("mute", muted)],
            Command::SetVolume(volume) => vec![Op::Double("volume", volume.clamp(0.0, super::MAX_VOLUME))],
            Command::SetSpeed(speed) => vec![Op::Double("speed", speed.clamp(MIN_SPEED, MAX_SPEED))],
            Command::SetLoop(on) => vec![Op::Text("loop-file", if on { "inf" } else { "no" }.to_owned())],
            Command::Quit => vec![Op::Command("quit", Vec::new())],
        }
    }

    fn load(&mut self, load: Load) -> Vec<Op> {
        self.duration = None;
        let mut ops = Vec::new();
        if self.demuxer_offset > 0.0 {
            self.demuxer_offset = 0.0;
            ops.push(Op::Double("hr-seek-demuxer-offset", 0.0));
        }
        if let Some(play) = load.play {
            ops.push(Op::Flag("pause", !play));
        }
        // The duration of the new file is not known yet: the end mute waits for it.
        ops.extend(self.audio_ops());
        let mut args = vec![load.path.to_string_lossy().into_owned(), "replace".into()];
        if let Some(start) = load.start.filter(|s| s.is_finite() && *s > 0.0) {
            args.push("-1".into());
            args.push(format!("start={start:.6}"));
        }
        tracing::info!(path = %load.path.display(), start = ?load.start, "[Player] Loading");
        ops.push(Op::Command("loadfile", args));
        ops
    }

    /// Some demuxers (MPEG-TS) seek to the keyframe after the time they are asked for, so mpv's step back (which asks
    /// for half a second before the frame) can reach a keyframe but never pass it. Each failure doubles how far before
    /// the target the demuxer is asked to seek, and the step is tried again; the offset stays for the file.
    fn backstep_failed(&mut self, at: f64) -> Vec<Op> {
        // Within mpv's own half second of the start there is no frame before: nothing to retry.
        if at <= BACKSTEP_MARGIN {
            return Vec::new();
        }
        if self.demuxer_offset >= MAX_DEMUXER_OFFSET {
            tracing::warn!(
                at,
                offset = self.demuxer_offset,
                "[Player] Stepping back failed although the demuxer seeks far back"
            );
            return Vec::new();
        }
        self.demuxer_offset = if self.demuxer_offset > 0.0 {
            (self.demuxer_offset * 2.0).min(MAX_DEMUXER_OFFSET)
        } else {
            FIRST_DEMUXER_OFFSET
        };
        tracing::info!(
            at,
            offset = self.demuxer_offset,
            "[Player] The demuxer landed after the step's target; seeking further back"
        );
        vec![
            Op::Double("hr-seek-demuxer-offset", self.demuxer_offset),
            Op::Command("frame-back-step", Vec::new()),
        ]
    }

    fn video_ops(&mut self, filter: VideoFilter) -> Vec<Op> {
        if filter == self.vf {
            return Vec::new();
        }
        let mut ops = Vec::new();
        if filter.rotates_source != self.vf.rotates_source {
            ops.push(Op::Text("video-rotate", video::video_rotate_value(&filter).to_owned()));
        }
        if filter.lavfi != self.vf.lavfi {
            tracing::debug!(lavfi = %filter.lavfi, "[Player] Video filters");
            ops.push(Op::Text("vf", video::vf_value(&filter)));
        }
        self.vf = filter;
        ops
    }

    fn audio_ops(&mut self) -> Vec<Op> {
        let chain = audio::filter_chain(self.audio, self.duration);
        if chain == self.af {
            return Vec::new();
        }
        self.af.clone_from(&chain);
        tracing::debug!(chain = %chain, "[Player] Audio filters");
        vec![Op::Text("af", chain)]
    }
}

/// How far before its target mpv's step back seeks by itself (`playloop.c`: at least 0.5 s for a step).
const BACKSTEP_MARGIN: f64 = 0.5;
/// The demuxer offsets a failed step back tries: the first, doubled after each failure up to the last.
const FIRST_DEMUXER_OFFSET: f64 = 1.0;
const MAX_DEMUXER_OFFSET: f64 = 32.0;

/// mpv's limits of the `speed` property.
const MIN_SPEED: f64 = 0.01;
const MAX_SPEED: f64 = 100.0;

/// Drops commands that a later one in the same burst makes pointless: of consecutive seeks only the last runs (a drag on the
/// timeline, held arrow keys), likewise for volume changes (the mouse wheel), speed changes and video filters (typing a
/// number into a filter field).
pub(crate) fn coalesce(batch: Vec<Command>) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::with_capacity(batch.len());
    for command in batch {
        let replaces_last = matches!(
            (out.last(), &command),
            (Some(Command::Seek { .. }), Command::Seek { .. })
                | (Some(Command::SetVolume(_)), Command::SetVolume(_))
                | (Some(Command::SetSpeed(_)), Command::SetSpeed(_))
                | (Some(Command::SetVideoFilter(_)), Command::SetVideoFilter(_))
        );
        if replaces_last {
            out.pop();
        }
        out.push(command);
    }
    out
}

/// Runs until [`Command::Quit`] or until every sender is gone.
pub(crate) fn run(mpv: &Arc<Mpv>, commands: &Receiver<Command>, mut session: Session) {
    tracing::debug!("[Player] Command thread started");
    while let Ok(first) = commands.recv() {
        let mut batch = vec![first];
        batch.extend(commands.try_iter());
        for command in coalesce(batch) {
            let quit = command == Command::Quit;
            for op in session.handle(command) {
                execute(mpv, &op);
            }
            if quit {
                tracing::debug!("[Player] Command thread ended");
                return;
            }
        }
    }
    tracing::debug!("[Player] Command thread ended (no senders left)");
}

pub(crate) fn execute(mpv: &Mpv, op: &Op) {
    tracing::debug!(op = ?op, "[Player] mpv call");
    let result = match op {
        Op::Command(name, args) => {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            mpv.command(name, &args)
        }
        Op::Flag(name, value) => mpv.set_property(name, *value),
        Op::Double(name, value) => mpv.set_property(name, *value),
        Op::Text(name, value) => mpv.set_property(name, value.as_str()),
    };
    if let Err(e) = result {
        tracing::warn!(op = ?op, error = %e, "[Player] mpv refused a command");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(path: &str) -> Command {
        Command::Open {
            path: PathBuf::from(path),
            start: None,
            play: Some(true),
        }
    }

    fn loadfile(ops: &[Op]) -> Option<&Vec<String>> {
        ops.iter().find_map(|op| match op {
            Op::Command("loadfile", args) => Some(args),
            _ => None,
        })
    }

    #[test]
    fn a_failed_step_back_seeks_further_back_and_steps_again() {
        let mut s = Session::new(PreviewQuality::Auto, AudioOptions::default());
        let step = Op::Command("frame-back-step", vec![]);
        assert_eq!(
            s.handle(Command::BackstepFailed { at: 10.0 }),
            vec![Op::Double("hr-seek-demuxer-offset", 1.0), step.clone()]
        );
        assert_eq!(
            s.handle(Command::BackstepFailed { at: 10.0 }),
            vec![Op::Double("hr-seek-demuxer-offset", 2.0), step]
        );
        for _ in 0..4 {
            s.handle(Command::BackstepFailed { at: 10.0 });
        }
        assert_eq!(s.demuxer_offset, MAX_DEMUXER_OFFSET);
        // At the limit nothing is tried again (no endless loop).
        assert!(s.handle(Command::BackstepFailed { at: 10.0 }).is_empty());
        // A new file starts with mpv's default again.
        s.handle(Command::SurfaceReady { software: false });
        let ops = s.handle(open("/v/b.ts"));
        assert_eq!(ops.first(), Some(&Op::Double("hr-seek-demuxer-offset", 0.0)));
        assert_eq!(s.demuxer_offset, 0.0);
    }

    #[test]
    fn a_step_back_at_the_start_is_not_retried() {
        let mut s = Session::new(PreviewQuality::Auto, AudioOptions::default());
        assert!(s.handle(Command::BackstepFailed { at: 0.0 }).is_empty());
        assert!(s.handle(Command::BackstepFailed { at: 0.4 }).is_empty());
        assert_eq!(s.demuxer_offset, 0.0);
    }

    #[test]
    fn a_file_opened_before_the_surface_waits_for_it() {
        let mut s = Session::new(PreviewQuality::Auto, AudioOptions::default());
        assert!(s.handle(open("/v/a.mkv")).is_empty());
        let ops = s.handle(Command::SurfaceReady { software: false });
        assert_eq!(loadfile(&ops), Some(&vec!["/v/a.mkv".to_owned(), "replace".to_owned()]));
        assert_eq!(ops.first(), Some(&Op::Flag("pause", false)));
        // Once the surface exists, files load at once; a lost surface makes them wait again.
        assert!(loadfile(&s.handle(open("/v/b.mkv"))).is_some());
        assert!(s.handle(Command::SurfaceLost).is_empty());
        assert!(s.handle(open("/v/c.mkv")).is_empty());
    }

    #[test]
    fn software_gl_gets_the_fast_profile_once() {
        let mut s = Session::new(PreviewQuality::Auto, AudioOptions::default());
        let fast = Op::Command("apply-profile", vec!["fast".into()]);
        assert_eq!(s.handle(Command::SurfaceReady { software: true }), vec![fast]);
        s.handle(Command::SurfaceLost);
        assert!(s.handle(Command::SurfaceReady { software: true }).is_empty());
        let mut gpu = Session::new(PreviewQuality::Auto, AudioOptions::default());
        assert!(gpu.handle(Command::SurfaceReady { software: false }).is_empty());
        let mut high = Session::new(PreviewQuality::High, AudioOptions::default());
        assert!(high.handle(Command::SurfaceReady { software: true }).is_empty());
    }

    #[test]
    fn a_reload_keeps_the_pause_state_and_starts_at_the_position() {
        let mut s = Session::new(PreviewQuality::High, AudioOptions::default());
        s.handle(Command::SurfaceReady { software: false });
        let ops = s.handle(Command::Open {
            path: PathBuf::from("/v/a.mkv"),
            start: Some(12.5),
            play: None,
        });
        assert_eq!(
            ops,
            vec![Op::Command(
                "loadfile",
                vec![
                    "/v/a.mkv".into(),
                    "replace".into(),
                    "-1".into(),
                    "start=12.500000".into()
                ]
            )]
        );
    }

    #[test]
    fn the_end_mute_follows_the_duration_and_is_set_only_on_change() {
        let audio = AudioOptions {
            mute_last_second: true,
            normalization: audio::Normalization::None,
        };
        let mut s = Session::new(PreviewQuality::High, audio);
        s.handle(Command::SurfaceReady { software: false });
        // Nothing to set before the duration is known (the chain is still empty).
        assert!(!s.handle(open("/v/a.mkv")).iter().any(|op| matches!(op, Op::Text(..))));
        let ops = s.handle(Command::Duration(20.0));
        assert_eq!(
            ops,
            vec![Op::Text(
                "af",
                "@rcend:lavfi=[volume=volume=0:enable='gte(t,19.000)']".into()
            )]
        );
        assert!(s.handle(Command::Duration(20.0)).is_empty());
        // The next file clears the end mute until its own duration is known.
        assert!(s.handle(open("/v/b.mkv")).contains(&Op::Text("af", String::new())));
        assert_eq!(s.handle(Command::SetAudio(AudioOptions::default())), Vec::new());
    }

    #[test]
    fn seeks_and_steps_map_to_mpv_commands() {
        let mut s = Session::new(PreviewQuality::High, AudioOptions::default());
        assert_eq!(
            s.handle(Command::Seek {
                seconds: -3.0,
                exact: true
            }),
            vec![Op::Command("seek", vec!["0.000000".into(), "absolute+exact".into()])]
        );
        assert_eq!(
            s.handle(Command::FrameStep { back: true }),
            vec![Op::Command("frame-back-step", vec![])]
        );
        assert_eq!(s.handle(Command::SetVolume(400.0)), vec![Op::Double("volume", 100.0)]);
        assert_eq!(s.handle(Command::SetSpeed(4.0)), vec![Op::Double("speed", 4.0)]);
        assert_eq!(s.handle(Command::SetSpeed(0.0)), vec![Op::Double("speed", 0.01)]);
        assert_eq!(
            s.handle(Command::SetLoop(true)),
            vec![Op::Text("loop-file", "inf".to_owned())]
        );
        assert_eq!(
            s.handle(Command::SetLoop(false)),
            vec![Op::Text("loop-file", "no".to_owned())]
        );
    }

    #[test]
    fn play_pause_follows_the_observed_state_and_starts_over_at_the_end() {
        let mut s = Session::new(PreviewQuality::High, AudioOptions::default());
        let cycle = Op::Command("cycle", vec!["pause".into()]);
        let restart = Op::Command("seek", vec!["0".into(), "absolute+exact".into()]);
        let observed = |paused, at_end| Command::Observed { paused, at_end };
        assert!(
            s.handle(observed(Some(false), None)).is_empty(),
            "an observation is no mpv call"
        );
        assert_eq!(s.handle(Command::TogglePause), vec![cycle.clone()]);
        assert!(s.handle(observed(Some(true), Some(true))).is_empty());
        assert_eq!(
            s.handle(Command::TogglePause),
            vec![restart, cycle.clone()],
            "paused at the end"
        );
        assert!(s.handle(observed(None, Some(false))).is_empty());
        assert_eq!(s.handle(Command::TogglePause), vec![cycle]);
    }

    #[test]
    fn decoding_and_background_change_while_running() {
        let mut s = Session::new(PreviewQuality::Auto, AudioOptions::default());
        assert_eq!(
            s.handle(Command::SetHwdec("no".into())),
            vec![Op::Text("hwdec", "no".to_owned())]
        );
        assert_eq!(
            s.handle(Command::SetBackground("#102030".into())),
            vec![Op::Text("background-color", "#102030".to_owned())]
        );
        assert!(s.handle(Command::SetBackground("red".into())).is_empty());
    }

    #[test]
    fn bursts_keep_only_the_last_seek_and_volume() {
        let seek = |seconds| Command::Seek { seconds, exact: false };
        let batch = vec![
            seek(1.0),
            seek(2.0),
            seek(3.0),
            Command::SetPaused(true),
            seek(4.0),
            Command::SetVolume(10.0),
            Command::SetVolume(20.0),
        ];
        assert_eq!(
            coalesce(batch),
            vec![seek(3.0), Command::SetPaused(true), seek(4.0), Command::SetVolume(20.0)]
        );
    }

    #[test]
    fn video_filters_are_set_only_on_change_and_rotation_follows() {
        let mut s = Session::new(PreviewQuality::High, AudioOptions::default());
        let crop = VideoFilter {
            lavfi: "transpose=dir=clock,crop=w=iw-4:h=ih:x=0:y=0".into(),
            rotates_source: true,
        };
        assert_eq!(
            s.handle(Command::SetVideoFilter(crop.clone())),
            vec![
                Op::Text("video-rotate", "no".into()),
                Op::Text(
                    "vf",
                    "@rcvf:lavfi=graph=%44%transpose=dir=clock,crop=w=iw-4:h=ih:x=0:y=0".into()
                ),
            ]
        );
        assert!(s.handle(Command::SetVideoFilter(crop)).is_empty());
        assert_eq!(
            s.handle(Command::SetVideoFilter(VideoFilter::default())),
            vec![Op::Text("video-rotate", "0".into()), Op::Text("vf", String::new())]
        );
        let batch = vec![
            Command::SetVideoFilter(VideoFilter::default()),
            Command::SetVideoFilter(VideoFilter {
                lavfi: "hflip".into(),
                rotates_source: false,
            }),
        ];
        assert_eq!(coalesce(batch).len(), 1);
    }

    #[test]
    fn bursts_keep_only_the_last_speed() {
        let batch = vec![
            Command::SetSpeed(2.0),
            Command::SetSpeed(4.0),
            Command::SetPaused(false),
        ];
        assert_eq!(coalesce(batch), vec![Command::SetSpeed(4.0), Command::SetPaused(false)]);
    }
}
