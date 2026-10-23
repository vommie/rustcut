//! What the UI knows about playback, built from the player's events (pure, tested). The event thread applies events; the
//! UI thread reads a [`PlaybackView`] snapshot and sends commands that need the state (seek target, reload position).

use std::path::{Path, PathBuf};

use az::{Az as _, CheckedAs as _};
use rustcut_core::time::Timestamp;
use rustcut_player::{PlayerEvent, Property};

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
        }
    }
}

/// What the Slint `Playback` global shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackView {
    pub has_file: bool,
    pub has_video: bool,
    pub paused: bool,
    pub muted: bool,
    pub volume: i32,
    pub progress: f32,
    pub position_text: String,
    pub duration_text: String,
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
            PlayerEvent::PlaybackFailed { reason, detail } => {
                self.loaded = false;
                self.duration = None;
                self.position = Timestamp::ZERO;
                self.failure = Some(Failure {
                    title: reason.clone(),
                    detail: detail.clone().unwrap_or_default(),
                });
            }
            PlayerEvent::Property(property) => self.apply_property(property),
            PlayerEvent::Shutdown => self.loaded = false,
            PlayerEvent::VideoReconfigured
            | PlayerEvent::PlaybackRestarted
            | PlayerEvent::Ended(_)
            | PlayerEvent::Error(_) => {}
        }
    }

    fn apply_property(&mut self, property: &Property) {
        match property {
            Property::Position(seconds) => {
                if let Some(t) = Timestamp::from_secs_f64(*seconds) {
                    self.position = t;
                }
            }
            Property::Duration(seconds) => self.duration = Timestamp::from_secs_f64(*seconds),
            Property::Paused(paused) => self.paused = *paused,
            Property::Muted(muted) => self.muted = *muted,
            Property::Volume(volume) => self.volume = percent(*volume),
            Property::HwDecoder(name) => self.decoder = Some(name.clone()),
            Property::EndReached(_) => {}
        }
    }

    /// The position for `fraction` (0-1) of the duration, in seconds.
    pub fn seek_target(&self, fraction: f32) -> Option<f64> {
        let duration = self.duration?.as_secs_f64();
        Some(duration * f64::from(fraction.clamp(0.0, 1.0)))
    }

    pub fn view(&self) -> PlaybackView {
        let progress = match self.duration {
            Some(d) if d.as_micros() > 0 => (self.position.as_secs_f64() / d.as_secs_f64()).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let failure = self.failure.as_ref();
        PlaybackView {
            has_file: self.file.is_some(),
            has_video: self.loaded && failure.is_none(),
            // Without a loaded file nothing plays, whatever mpv's pause flag says.
            paused: self.paused || !self.loaded,
            muted: self.muted,
            volume: i32::from(self.volume),
            progress: progress.az::<f32>(),
            position_text: self.position.to_string(),
            duration_text: self.duration.unwrap_or(Timestamp::ZERO).to_string(),
            error_title: failure.map(|f| f.title.clone()).unwrap_or_default(),
            error_detail: failure.map(|f| f.detail.clone()).unwrap_or_default(),
            decoder_text: decoder_text(self.loaded, self.decoder.as_deref()),
        }
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
        assert!((view.progress - 0.139_09).abs() < 1e-4, "{}", view.progress);
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
        assert_eq!((view.position_text.as_str(), view.progress), ("0:00:00.000", 0.0));
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
    fn seek_targets_follow_the_duration() {
        let s = playing();
        assert_eq!(s.seek_target(0.5), Some(300.0));
        assert_eq!(s.seek_target(2.0), Some(600.0));
        assert_eq!(s.seek_target(-1.0), Some(0.0));
        assert_eq!(PlaybackState::default().seek_target(0.5), None);
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
