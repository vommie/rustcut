//! The mpv event thread: waits on its own client handle and hands typed events to the app.

use libmpv2::Mpv;
use libmpv2::events::Event;

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

/// What the player reports to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A file was opened; its properties (duration, tracks, video parameters) can be read now.
    FileLoaded,
    /// The video output was (re)configured (new size, format or rotation).
    VideoReconfigured,
    /// Playback (re)started after loading or seeking.
    PlaybackRestarted,
    /// Playback of the current file ended.
    Ended(EndReason),
    /// mpv reported an error (for example a file that cannot be decoded).
    Error(String),
    /// The core shut down; no further events follow.
    Shutdown,
}

/// Runs until mpv shuts down.
pub(super) fn run(client: &Mpv, on_event: &dyn Fn(PlayerEvent)) {
    tracing::debug!("[Player] Event thread started");
    loop {
        let event = match client.wait_event(-1.0) {
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
                super::log::forward(prefix, log_level, text);
                continue;
            }
            Some(Ok(Event::FileLoaded)) => PlayerEvent::FileLoaded,
            Some(Ok(Event::VideoReconfig)) => PlayerEvent::VideoReconfigured,
            Some(Ok(Event::PlaybackRestart)) => PlayerEvent::PlaybackRestarted,
            Some(Ok(Event::EndFile(reason))) => PlayerEvent::Ended(EndReason::from_raw(reason)),
            // Nothing (a spurious wakeup, a property without value) or an event RustCut does not use.
            None | Some(Ok(_)) => continue,
        };
        on_event(event);
    }
    tracing::debug!("[Player] Event thread ended");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_reasons_map_from_libmpv() {
        assert_eq!(EndReason::from_raw(0), EndReason::Eof);
        assert_eq!(EndReason::from_raw(2), EndReason::Stopped);
        assert_eq!(EndReason::from_raw(3), EndReason::Quit);
        assert_eq!(EndReason::from_raw(4), EndReason::Error);
        assert_eq!(EndReason::from_raw(5), EndReason::Other);
        assert_eq!(EndReason::from_raw(99), EndReason::Other);
    }
}
