//! The mpv event thread: waits on its own client handle and hands typed events to the app. The observed properties
//! (position, duration, pause, mute, volume, ...) arrive here as property changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};

use super::commands::Command;

/// Why playback of a file ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The end of the file was reached.
    Eof,
    /// Stopped by a command (another file loaded, `stop`).
    Stopped,
    /// mpv is shutting down.
    Quit,
    /// Playback failed (unreadable or broken file).
    Error,
    /// The video surface went away (the window closed): mpv stopped the video, and a file without audio ended with it.
    SurfaceLost,
    /// Anything newer than this list.
    Other,
}

impl EndReason {
    fn from_raw(reason: libmpv2::EndFileReason) -> Self {
        match reason {
            libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_EOF => Self::Eof,
            libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_STOP => Self::Stopped,
            libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_QUIT => Self::Quit,
            libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR => Self::Error,
            _ => Self::Other,
        }
    }
}

/// A change of an observed mpv property.
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    /// Playback position in seconds (`time-pos`).
    Position(f64),
    /// Length of the file in seconds.
    Duration(f64),
    Paused(bool),
    Muted(bool),
    /// Volume in percent.
    Volume(f64),
    /// The last frame is shown and playback cannot continue (`eof-reached`, with `keep-open`).
    EndReached(bool),
    /// The hardware decoder in use (`hwdec-current`); `no` or empty for software decoding.
    HwDecoder(String),
    /// Playback speed (1 = normal).
    Speed(f64),
}

/// What the player reports to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// Loading of a file began (the properties of the previous file no longer apply).
    FileStarted,
    /// A file was opened; its properties (duration, tracks, video parameters) can be read now.
    FileLoaded,
    /// The video output was (re)configured (new size, format or rotation).
    VideoReconfigured,
    /// Playback (re)started after loading or seeking.
    PlaybackRestarted,
    /// An observed property changed.
    Property(Property),
    /// Playback of the current file ended.
    Ended(EndReason),
    /// The file could not be played: `reason` says why in plain words, `detail` is mpv's last error message for it.
    PlaybackFailed { reason: String, detail: Option<String> },
    /// mpv reported another error.
    Error(String),
    /// The core shut down; no further events follow.
    Shutdown,
}

/// The observed properties: name, format and the id mpv returns with each change.
const OBSERVED: [(&str, Format); 8] = [
    ("time-pos", Format::Double),
    ("duration", Format::Double),
    ("pause", Format::Flag),
    ("mute", Format::Flag),
    ("volume", Format::Double),
    ("eof-reached", Format::Flag),
    ("hwdec-current", Format::String),
    ("speed", Format::Double),
];

/// Stops every event on a handle nobody reads: `Mpv::with_initializer`'s core handle only sends commands, and mpv queues
/// each event for every handle, so that queue would fill up with seeks until mpv logs "Too many events queued". The
/// numbers run through mpv's whole event mask; those without an event are refused, and so is the shutdown event
/// (libmpv2's `disable_all_events` stops at the first refusal).
pub(super) fn silence(core: &Mpv) {
    for id in 2..64 {
        core.disable_event(id).ok();
    }
}

/// Asks mpv to report changes of the [`OBSERVED`] properties on `client` (its current values arrive at once).
pub(super) fn observe(client: &Mpv) {
    for (id, (name, format)) in (1_u64..).zip(OBSERVED) {
        if let Err(e) = client.observe_property(name, format, id) {
            tracing::warn!(property = name, error = %e, "[Player] mpv property cannot be observed");
        }
    }
}

/// Runs until mpv shuts down. The file's duration also goes to the command thread (the end mute needs it). `surface` says
/// whether a render context exists (see [`failed_end`]).
pub(super) fn run(client: &Mpv, on_event: &dyn Fn(PlayerEvent), commands: &Sender<Command>, surface: &AtomicBool) {
    tracing::debug!("[Player] Event thread started");
    // mpv's first error message since the current file started: the detail of a failure (the first one names the cause,
    // later ones repeat it in general words).
    let mut first_error: Option<String> = None;
    // The last `time-pos`, for a failed step back (the command thread never reads a property).
    let mut position = 0.0;
    loop {
        let event = match client.wait_event(-1.0) {
            // libmpv2 turns an end of file with an error into `Err(Raw(code))`; this app makes no asynchronous requests,
            // so a raw error code always comes from there.
            Some(Err(libmpv2::Error::Raw(code))) => {
                failed_end(code, first_error.take(), surface.load(Ordering::Acquire))
            }
            Some(Err(e)) => PlayerEvent::Error(e.to_string()),
            Some(Ok(Event::Shutdown)) => {
                on_event(PlayerEvent::Shutdown);
                break;
            }
            Some(Ok(Event::LogMessage {
                prefix,
                text,
                log_level,
                ..
            })) => {
                if first_error.is_none()
                    && matches!(log_level, libmpv2::mpv_log_level::Error | libmpv2::mpv_log_level::Fatal)
                    && !is_hwdec_probe(prefix)
                {
                    first_error = Some(text.trim_end().to_owned());
                }
                if is_backstep_failure(prefix, text) {
                    commands.send(Command::BackstepFailed { at: position }).ok();
                }
                super::log::forward(prefix, log_level, text);
                continue;
            }
            Some(Ok(Event::StartFile)) => {
                first_error = None;
                PlayerEvent::FileStarted
            }
            Some(Ok(Event::FileLoaded)) => PlayerEvent::FileLoaded,
            Some(Ok(Event::VideoReconfig)) => PlayerEvent::VideoReconfigured,
            Some(Ok(Event::PlaybackRestart)) => PlayerEvent::PlaybackRestarted,
            Some(Ok(Event::EndFile(reason))) => PlayerEvent::Ended(EndReason::from_raw(reason)),
            // No resync: mpv keeps the observed properties as state and reports their latest values anyway, and `on_event`
            // never blocks, so this queue only fills if something is badly wrong. That is worth a line in the log.
            Some(Ok(Event::QueueOverflow)) => {
                tracing::warn!("[Player] mpv's event queue overflowed; some events were lost");
                continue;
            }
            Some(Ok(Event::PropertyChange { name, change, .. })) => {
                let Some(property) = property(name, &change) else {
                    continue;
                };
                // Fails only when the command thread is gone (shutdown).
                match property {
                    Property::Position(seconds) => position = seconds,
                    Property::Duration(seconds) => {
                        commands.send(Command::Duration(seconds)).ok();
                    }
                    Property::Paused(paused) => {
                        commands
                            .send(Command::Observed {
                                paused: Some(paused),
                                at_end: None,
                            })
                            .ok();
                    }
                    Property::EndReached(at_end) => {
                        commands
                            .send(Command::Observed {
                                paused: None,
                                at_end: Some(at_end),
                            })
                            .ok();
                    }
                    _ => {}
                }
                PlayerEvent::Property(property)
            }
            // Nothing (a spurious wakeup, a property without value) or an event RustCut does not use.
            None | Some(Ok(_)) => continue,
        };
        on_event(event);
    }
    tracing::debug!("[Player] Event thread ended");
}

/// What an end of file with mpv's error `code` means; `detail` is mpv's first error message since the file started,
/// `surface` whether a render context exists. Freeing the render context while a video plays makes mpv stop the video
/// with `MPV_ERROR_VO_INIT_FAILED` (`do_kill` in mpv's `player/client.c`), which ends a file without audio: without the
/// surface that is RustCut taking the output away (the window closed), not a failure of the file.
fn failed_end(code: i32, detail: Option<String>, surface: bool) -> PlayerEvent {
    if code == libmpv2_sys::mpv_error_MPV_ERROR_VO_INIT_FAILED && !surface {
        return PlayerEvent::Ended(EndReason::SurfaceLost);
    }
    PlayerEvent::PlaybackFailed {
        reason: describe_failure(code).to_owned(),
        detail,
    }
}

/// Hardware decoders that are not available log errors while mpv probes them (`hwdec=auto-copy`: Vulkan, CUDA, VAAPI in a
/// VM); those modules never explain why a file cannot be played.
fn is_hwdec_probe(module: &str) -> bool {
    matches!(module, "ffmpeg" | "vaapi" | "vulkan" | "cuda")
}

/// The typed form of a property change; `None` for unexpected names or formats.
/// mpv's warning that a step back found no frame before the current one (`video.c`).
fn is_backstep_failure(prefix: &str, text: &str) -> bool {
    prefix == "cplayer" && text.trim_end() == "Backstep failed."
}

fn property(name: &str, change: &PropertyData<'_>) -> Option<Property> {
    Some(match (name, change) {
        ("time-pos", PropertyData::Double(v)) => Property::Position(*v),
        ("duration", PropertyData::Double(v)) => Property::Duration(*v),
        ("pause", PropertyData::Flag(v)) => Property::Paused(*v),
        ("mute", PropertyData::Flag(v)) => Property::Muted(*v),
        ("volume", PropertyData::Double(v)) => Property::Volume(*v),
        ("eof-reached", PropertyData::Flag(v)) => Property::EndReached(*v),
        ("hwdec-current", PropertyData::Str(v)) => Property::HwDecoder((*v).to_owned()),
        ("speed", PropertyData::Double(v)) => Property::Speed(*v),
        _ => return None,
    })
}

/// Plain words for mpv's end-of-file error codes (`MPV_ERROR_*` in `client.h`).
fn describe_failure(code: i32) -> &'static str {
    match code {
        libmpv2_sys::mpv_error_MPV_ERROR_UNKNOWN_FORMAT => "The file format is not recognized.",
        libmpv2_sys::mpv_error_MPV_ERROR_LOADING_FAILED => "The file could not be loaded.",
        libmpv2_sys::mpv_error_MPV_ERROR_NOTHING_TO_PLAY => "The file has no video or audio that can be played.",
        libmpv2_sys::mpv_error_MPV_ERROR_UNSUPPORTED => "The file is not supported.",
        libmpv2_sys::mpv_error_MPV_ERROR_AO_INIT_FAILED => "The audio output could not be started.",
        libmpv2_sys::mpv_error_MPV_ERROR_VO_INIT_FAILED => "The video output could not be started.",
        _ => "Playback failed.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silenced_handle_queues_nothing_and_the_read_one_still_gets_events() {
        let core = Mpv::with_initializer(|_| Ok(())).unwrap();
        let client = core.create_client(Some("reader")).unwrap();
        silence(&core);
        // More than the 1000 events a handle's queue holds, broadcast to every handle.
        for _ in 0..1_100 {
            core.command("script-message", &["hello"]).unwrap();
        }
        assert!(
            core.wait_event(0.0).is_none(),
            "nothing was queued on the silenced handle"
        );
        let first = client.wait_event(5.0);
        assert!(matches!(first, Some(Ok(Event::ClientMessage(_)))), "{first:?}");
    }

    #[test]
    fn end_reasons_map_from_libmpv() {
        assert_eq!(EndReason::from_raw(0), EndReason::Eof);
        assert_eq!(EndReason::from_raw(2), EndReason::Stopped);
        assert_eq!(EndReason::from_raw(3), EndReason::Quit);
        assert_eq!(EndReason::from_raw(4), EndReason::Error);
        assert_eq!(EndReason::from_raw(5), EndReason::Other);
        assert_eq!(EndReason::from_raw(99), EndReason::Other);
    }

    #[test]
    fn a_failed_step_back_is_recognized() {
        assert!(is_backstep_failure("cplayer", "Backstep failed.\n"));
        assert!(!is_backstep_failure("ffmpeg/video", "Backstep failed.\n"));
        assert!(!is_backstep_failure("cplayer", "Backstep failed? no"));
    }

    #[test]
    fn property_changes_are_typed() {
        assert_eq!(
            property("time-pos", &PropertyData::Double(1.5)),
            Some(Property::Position(1.5))
        );
        assert_eq!(
            property("pause", &PropertyData::Flag(true)),
            Some(Property::Paused(true))
        );
        assert_eq!(
            property("hwdec-current", &PropertyData::Str("vaapi-copy")),
            Some(Property::HwDecoder("vaapi-copy".into()))
        );
        assert_eq!(property("pause", &PropertyData::Double(1.0)), None);
        assert_eq!(
            property("speed", &PropertyData::Double(2.0)),
            Some(Property::Speed(2.0))
        );
        assert_eq!(property("speed", &PropertyData::Flag(true)), None);
        assert_eq!(property("chapter", &PropertyData::Double(1.0)), None);
        // Every observed property has a mapping.
        for (name, format) in OBSERVED {
            let sample = match format {
                Format::Double => PropertyData::Double(0.0),
                Format::Flag => PropertyData::Flag(false),
                _ => PropertyData::Str(""),
            };
            assert!(property(name, &sample).is_some(), "{name}");
        }
    }

    #[test]
    fn hardware_decoder_probes_are_not_failure_details() {
        assert!(is_hwdec_probe("ffmpeg") && is_hwdec_probe("vaapi"));
        assert!(!is_hwdec_probe("ffmpeg/demuxer") && !is_hwdec_probe("lavf") && !is_hwdec_probe("cplayer"));
    }

    #[test]
    fn a_video_output_closed_with_the_surface_is_no_failure() {
        let vo = libmpv2_sys::mpv_error_MPV_ERROR_VO_INIT_FAILED;
        // The window closed: freeing the render context ended the file.
        assert_eq!(failed_end(vo, None, false), PlayerEvent::Ended(EndReason::SurfaceLost));
        // The same error while the surface exists is a real failure of the video output.
        assert_eq!(
            failed_end(vo, Some("vo/libmpv: failed".into()), true),
            PlayerEvent::PlaybackFailed {
                reason: "The video output could not be started.".into(),
                detail: Some("vo/libmpv: failed".into()),
            }
        );
        // Other errors stay failures without the surface too.
        let broken = libmpv2_sys::mpv_error_MPV_ERROR_LOADING_FAILED;
        assert_eq!(
            failed_end(broken, None, false),
            PlayerEvent::PlaybackFailed {
                reason: "The file could not be loaded.".into(),
                detail: None,
            }
        );
    }

    #[test]
    fn failures_have_plain_words() {
        assert_eq!(describe_failure(-17), "The file format is not recognized.");
        assert_eq!(describe_failure(-13), "The file could not be loaded.");
        assert_eq!(describe_failure(-999), "Playback failed.");
    }
}
