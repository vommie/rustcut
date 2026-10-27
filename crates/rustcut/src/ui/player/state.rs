//! What the UI knows about playback, built from the player's events (pure, tested). The event thread applies events; the
//! UI thread reads a [`PlaybackView`] snapshot and sends commands that need the state (seek target, reload position).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use az::CheckedAs as _;
use rustcut_core::time::Timestamp;
use rustcut_player::{PlayerEvent, Property};

use super::shuttle::{current_rate, rate_text};

/// Why the current file cannot be played (shown in the error overlay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    pub file: Option<PathBuf>,
    /// The file is loaded and plays (or shows its frame when paused).
    pub loaded: bool,
    pub position: Timestamp,
    pub duration: Option<Timestamp>,
    pub paused: bool,
    pub muted: bool,
    /// Volume in percent.
    pub volume: u8,
    pub failure: Option<Failure>,
    /// mpv's `hwdec-current`.
    pub decoder: Option<String>,
    /// mpv's playback speed (1 = normal).
    pub speed: f64,
    /// Where a seek that is still in flight goes: the playhead shows it at once.
    pub seek_target: Option<Timestamp>,
    /// Speed of the emulated backward playback (0 = none); its moving target stays the playhead until it stops.
    pub reverse_rate: i8,
    /// When mpv reported `position` (the timeline moves the playhead on from there between two reports).
    pub measured: Instant,
    /// The end is reached (`eof-reached` with keep-open): the playhead stands at the end, not at the last frame's start.
    pub at_end: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            file: None,
            loaded: false,
            position: Timestamp::ZERO,
            duration: None,
            paused: true,
            muted: false,
            volume: 100,
            failure: None,
            decoder: None,
            speed: 1.0,
            seek_target: None,
            reverse_rate: 0,
            measured: Instant::now(),
            at_end: false,
        }
    }
}

/// What the Slint `Playback` global shows (and the timeline follows).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackView {
    pub has_file: bool,
    pub has_video: bool,
    pub paused: bool,
    pub muted: bool,
    pub volume: i32,
    /// The playhead (a seek in flight counts as done).
    pub position: Timestamp,
    /// How fast the playhead moves on from `position` (media seconds per second; 0 while paused, seeking or in the
    /// emulated backward playback) and since when: the timeline draws it smoothly between mpv's reports.
    pub motion: Motion,
    pub duration: Option<Timestamp>,
    pub position_text: String,
    pub duration_text: String,
    /// The shuttle speed (`2×`, `◀ 4×`); empty at normal playback.
    pub speed_text: String,
    pub error_title: String,
    pub error_detail: String,
    pub decoder_text: String,
}

impl PlaybackState {
    /// A new file is being opened: everything about the previous one is forgotten.
    pub fn opening(&mut self, file: &Path) {
        self.file = Some(file.to_path_buf());
        self.loaded = false;
        self.position = Timestamp::ZERO;
        self.duration = None;
        self.failure = None;
        self.decoder = None;
        self.seek_target = None;
        self.reverse_rate = 0;
        self.at_end = false;
    }

    /// A seek to `t` was sent.
    pub fn seeking(&mut self, t: Timestamp) {
        self.seek_target = Some(t);
        // A seek leaves the end (mpv reports it again when the seek lands there).
        self.at_end = false;
    }

    /// The playhead: where a seek in flight goes, else mpv's position (at the end: the end itself, as far as the playhead
    /// glided; mpv's last position is the last frame's start, a step back from there was the jump at the end).
    pub fn playhead(&self) -> Timestamp {
        let here = match self.duration.filter(|_| self.at_end) {
            Some(end) => end.max(self.position),
            None => self.position,
        };
        self.seek_target.unwrap_or(here)
    }

    /// The file is loaded and playing forward.
    pub fn playing(&self) -> bool {
        self.loaded && !self.paused && self.failure.is_none()
    }

    /// Takes one player event into account.
    pub fn apply(&mut self, event: &PlayerEvent) {
        match event {
            PlayerEvent::FileStarted => {
                self.loaded = false;
                self.failure = None;
            }
            PlayerEvent::FileLoaded => {
                self.loaded = true;
                self.failure = None;
            }
            PlayerEvent::PlaybackRestarted => {
                if self.reverse_rate == 0 {
                    self.seek_target = None;
                }
            }
            PlayerEvent::PlaybackFailed { reason, detail } => {
                self.loaded = false;
                self.duration = None;
                self.position = Timestamp::ZERO;
                self.seek_target = None;
                self.reverse_rate = 0;
                self.failure = Some(Failure {
                    title: reason.clone(),
                    detail: detail.clone().unwrap_or_default(),
                });
            }
            PlayerEvent::Property(property) => self.apply_property(property),
            PlayerEvent::Shutdown => self.loaded = false,
            PlayerEvent::VideoReconfigured | PlayerEvent::Ended(_) | PlayerEvent::Error(_) => {}
        }
    }

    fn apply_property(&mut self, property: &Property) {
        match property {
            Property::Position(seconds) => {
                if let Some(t) = Timestamp::from_secs_f64(*seconds) {
                    self.position = t;
                    self.measured = Instant::now();
                }
            }
            Property::Duration(seconds) => self.duration = Timestamp::from_secs_f64(*seconds),
            Property::Paused(paused) => self.paused = *paused,
            Property::Muted(muted) => self.muted = *muted,
            Property::Volume(volume) => self.volume = percent(*volume),
            Property::HwDecoder(name) => self.decoder = Some(name.clone()),
            Property::Speed(speed) => self.speed = *speed,
            Property::EndReached(at_end) => self.at_end = *at_end,
        }
    }

    pub fn view(&self) -> PlaybackView {
        let failure = self.failure.as_ref();
        // mpv may report a moment past the duration at the end (a stream that starts late, MPEG-PS): never shown.
        let position = self
            .duration
            .map_or_else(|| self.playhead(), |d| self.playhead().min(d));
        PlaybackView {
            has_file: self.file.is_some(),
            has_video: self.loaded && failure.is_none(),
            // Without a loaded file nothing plays, whatever mpv's pause flag says.
            paused: self.paused || !self.loaded,
            muted: self.muted,
            volume: i32::from(self.volume),
            position,
            motion: Motion {
                rate: if self.playing() && self.seek_target.is_none() && self.reverse_rate == 0 {
                    self.speed.max(0.0)
                } else {
                    0.0
                },
                since: self.measured,
            },
            duration: self.duration,
            position_text: position.to_string(),
            duration_text: self.duration.unwrap_or(Timestamp::ZERO).to_string(),
            speed_text: rate_text(current_rate(self.reverse_rate, self.playing(), self.speed)),
            error_title: failure.map(|f| f.title.clone()).unwrap_or_default(),
            error_detail: failure.map(|f| f.detail.clone()).unwrap_or_default(),
            decoder_text: decoder_text(self.loaded, self.decoder.as_deref()),
        }
    }
}

/// How the playhead moves on from a reported position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Media seconds per second (0 = it stands).
    pub rate: f64,
    /// When the position was reported.
    pub since: Instant,
}

impl Motion {
    /// The longest time the playhead is moved on without a new report (a stalled decoder stops it soon).
    pub const MAX_AHEAD: Duration = Duration::from_millis(200);

    /// Where the playhead is `now`, from `position` (never more than [`Self::MAX_AHEAD`] ahead, never past `end`).
    pub fn at(&self, position: Timestamp, now: Instant, end: Option<Timestamp>) -> Timestamp {
        if self.rate <= 0.0 {
            return position;
        }
        let ahead = now.saturating_duration_since(self.since).min(Self::MAX_AHEAD);
        let moved = Timestamp::from_secs_f64(ahead.as_secs_f64() * self.rate).unwrap_or(Timestamp::ZERO);
        let t = position + moved;
        end.map_or(t, |end| t.min(end))
    }
}

/// mpv's volume (0-150, may be fractional) as whole percent.
fn percent(volume: f64) -> u8 {
    volume.clamp(0.0, 150.0).round().checked_as::<u8>().unwrap_or(0)
}

/// The status line's decoder text: empty without a playing file.
fn decoder_text(loaded: bool, decoder: Option<&str>) -> String {
    if !loaded {
        return String::new();
    }
    match decoder {
        None => String::new(),
        Some("" | "no") => "Software decoding".to_owned(),
        Some(name) => format!("Hardware decoding ({name})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_the_end_the_playhead_stands_at_the_end_not_at_the_last_frame() {
        let mut s = playing();
        s.apply(&PlayerEvent::Property(Property::Duration(8.008)));
        s.apply(&PlayerEvent::Property(Property::Position(7.96)));
        assert_eq!(s.view().position, Timestamp::from_millis(7_960));
        s.apply(&PlayerEvent::Property(Property::EndReached(true)));
        s.apply(&PlayerEvent::Property(Property::Paused(true)));
        let end = s.view();
        assert_eq!(
            end.position,
            Timestamp::from_millis(8_008),
            "the whole way to the end, no step back"
        );
        assert_eq!(end.motion.rate, 0.0);
        s.seeking(Timestamp::from_secs(2));
        assert_eq!(s.view().position, Timestamp::from_secs(2), "a seek leaves the end");
        s.apply(&PlayerEvent::PlaybackRestarted);
        s.apply(&PlayerEvent::Property(Property::Position(2.0)));
        assert_eq!(s.view().position, Timestamp::from_secs(2));
    }

    #[test]
    fn the_playhead_moves_on_between_reports_only_while_it_plays() {
        let mut s = playing();
        let view = s.view();
        assert!((view.motion.rate - 1.0).abs() < 1e-9, "plays at normal speed");
        let at = |ms: u64| {
            view.motion.at(
                view.position,
                view.motion.since + Duration::from_millis(ms),
                view.duration,
            )
        };
        assert_eq!(at(0), view.position);
        assert_eq!(at(100) - view.position, Timestamp::from_millis(100));
        assert_eq!(
            at(5_000) - view.position,
            Timestamp::from_millis(200),
            "never more than 200 ms ahead"
        );
        let end = view.position + Timestamp::from_millis(50);
        assert_eq!(
            view.motion
                .at(view.position, view.motion.since + Duration::from_millis(100), Some(end)),
            end,
            "never past the end"
        );
        s.seeking(Timestamp::from_secs(10));
        assert_eq!(s.view().motion.rate, 0.0, "a seek in flight stands");
        s.seek_target = None;
        s.apply(&PlayerEvent::Property(Property::Paused(true)));
        let paused = s.view();
        assert_eq!(paused.motion.rate, 0.0);
        assert_eq!(
            paused
                .motion
                .at(paused.position, paused.motion.since + Duration::from_secs(1), None),
            paused.position
        );
    }

    fn playing() -> PlaybackState {
        let mut s = PlaybackState::default();
        s.opening(Path::new("/v/a.mkv"));
        for event in [
            PlayerEvent::FileStarted,
            PlayerEvent::FileLoaded,
            PlayerEvent::Property(Property::Duration(600.0)),
            PlayerEvent::Property(Property::Position(83.4567)),
            PlayerEvent::Property(Property::Paused(false)),
            PlayerEvent::Property(Property::HwDecoder("no".into())),
        ] {
            s.apply(&event);
        }
        s
    }

    #[test]
    fn a_playing_file_shows_its_times_and_progress() {
        let view = playing().view();
        assert!(view.has_file && view.has_video && !view.paused);
        assert_eq!(view.position_text, "0:01:23.456");
        assert_eq!(view.duration_text, "0:10:00.000");
        assert_eq!(view.position, Timestamp::from_micros(83_456_700));
        assert_eq!(view.speed_text, "");
        assert_eq!(view.decoder_text, "Software decoding");
        assert_eq!(view.error_title, "");
    }

    #[test]
    fn a_failure_shows_the_overlay_until_the_next_load() {
        let mut s = playing();
        s.apply(&PlayerEvent::PlaybackFailed {
            reason: "The file format is not recognized.".into(),
            detail: Some("moov atom not found".into()),
        });
        let view = s.view();
        assert!(view.has_file && !view.has_video);
        assert_eq!(view.error_title, "The file format is not recognized.");
        assert_eq!(view.error_detail, "moov atom not found");
        assert_eq!(
            (view.position_text.as_str(), view.position),
            ("0:00:00.000", Timestamp::ZERO)
        );
        s.apply(&PlayerEvent::FileStarted);
        assert_eq!(s.view().error_title, "");
    }

    #[test]
    fn opening_forgets_the_previous_file() {
        let mut s = playing();
        s.opening(Path::new("/v/b.mkv"));
        let view = s.view();
        assert!(view.has_file && !view.has_video);
        assert_eq!(view.duration_text, "0:00:00.000");
        assert_eq!(view.decoder_text, "");
        // Nothing plays until the file is loaded; mpv's pause flag stays as it was (it belongs to the player).
        assert!(view.paused && !s.paused);
    }

    #[test]
    fn the_shown_position_never_passes_the_duration() {
        let mut s = playing();
        s.apply(&PlayerEvent::Property(Property::Duration(10.0)));
        s.apply(&PlayerEvent::Property(Property::Position(10.007)));
        let view = s.view();
        assert_eq!(
            (view.position_text.as_str(), view.duration_text.as_str()),
            ("0:00:10.000", "0:00:10.000")
        );
    }

    #[test]
    fn a_seek_in_flight_is_the_playhead_until_mpv_restarts() {
        let mut s = playing();
        s.seeking(Timestamp::from_secs(300));
        assert_eq!(s.view().position_text, "0:05:00.000");
        s.apply(&PlayerEvent::PlaybackRestarted);
        assert_eq!(s.view().position, Timestamp::from_micros(83_456_700));
        // The emulated backward playback keeps its moving target.
        s.reverse_rate = 2;
        s.seeking(Timestamp::from_secs(80));
        s.apply(&PlayerEvent::PlaybackRestarted);
        assert_eq!(s.playhead(), Timestamp::from_secs(80));
        assert_eq!(s.view().speed_text, "\u{25C0} 2\u{00D7}");
        s.opening(Path::new("/v/b.mkv"));
        assert_eq!((s.seek_target, s.reverse_rate), (None, 0));
    }

    #[test]
    fn the_speed_shows_while_playing_faster() {
        let mut s = playing();
        s.apply(&PlayerEvent::Property(Property::Speed(4.0)));
        assert_eq!(s.view().speed_text, "4\u{00D7}");
        s.apply(&PlayerEvent::Property(Property::Paused(true)));
        assert_eq!(s.view().speed_text, "");
    }

    #[test]
    fn volume_and_decoder_texts() {
        assert_eq!(percent(49.6), 50);
        assert_eq!(percent(400.0), 150);
        assert_eq!(percent(-3.0), 0);
        assert_eq!(percent(f64::NAN), 0);
        assert_eq!(decoder_text(true, Some("vaapi-copy")), "Hardware decoding (vaapi-copy)");
        assert_eq!(decoder_text(true, Some("")), "Software decoding");
        assert_eq!(decoder_text(false, Some("vaapi-copy")), "");
        assert_eq!(decoder_text(true, None), "");
    }
}
