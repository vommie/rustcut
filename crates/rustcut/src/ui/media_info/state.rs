//! What the Media Info panel shows for the open file (pure, tested): the probe's sections and the source hash row. The
//! bridge feeds it probe results (runtime) and hash events (runtime); the UI thread reads views from it. Every open bumps a
//! generation, so a probe that finishes after the next file was opened is dropped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustcut_core::hashing::HashEvent;
use rustcut_core::media::{FrameTiming, InfoRow, InfoSection, MediaInfo, ProbeError, format_size, summarize};

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeStatus {
    Idle,
    Running,
    Ready {
        info: Arc<MediaInfo>,
        sections: Vec<InfoSection>,
    },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashStatus {
    /// No file is open.
    Idle,
    /// Not hashed automatically (the known-source warning is off); the user may start it.
    NotStarted,
    /// Requested or reading: `total` is 0 until the first progress arrives (or for a sidecar, which answers at once).
    Running {
        done: u64,
        total: u64,
    },
    Ready {
        md5: String,
        origin: HashOrigin,
    },
    Failed(String),
    Canceled,
}

/// Where the MD5 came from (ADR-6 order: RustView's record, the sidecar, reading the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashOrigin {
    /// RustView's record of the file (its size and time match, so the file was not read).
    Library,
    Sidecar,
    Computed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaPanelState {
    pub file: Option<PathBuf>,
    pub generation: u64,
    pub probe: ProbeStatus,
    pub hash: HashStatus,
    /// A job of this file was saved before its hash was known: register the hash in RustView once it is (bug 3: only
    /// sources of jobs are registered).
    pub register_hash: bool,
    /// RustCut opened the file itself (a loaded job): the known-source banner stays away.
    pub known_quiet: bool,
}

impl Default for MediaPanelState {
    fn default() -> Self {
        Self {
            file: None,
            generation: 0,
            probe: ProbeStatus::Idle,
            hash: HashStatus::Idle,
            register_hash: false,
            known_quiet: false,
        }
    }
}

/// The panel's content, as the `MediaInfoPanel` global takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionsView {
    pub state: PanelState,
    pub sections: Vec<InfoSection>,
}

/// What the panel body shows (the numbers are the Slint side's `state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    NoFile = 0,
    Sections = 1,
}

/// The MD5 row of the container section.
#[derive(Debug, Clone, PartialEq)]
pub struct HashView {
    pub text: String,
    pub note: String,
    /// 0 to 1 while reading with a known size, below 0 otherwise.
    pub progress: f32,
    /// The value is a digest (shown in a fixed-width font).
    pub is_digest: bool,
    pub can_compute: bool,
    pub can_cancel: bool,
}

impl MediaPanelState {
    /// A new file is opened: everything about the previous one is forgotten. Returns the new generation.
    pub fn opening(&mut self, file: &Path, hash_now: bool) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.file = Some(file.to_path_buf());
        self.register_hash = false;
        self.probe = ProbeStatus::Running;
        self.hash = if hash_now {
            HashStatus::Running { done: 0, total: 0 }
        } else {
            HashStatus::NotStarted
        };
        self.generation
    }

    /// The probe of `generation` finished. `false` when a newer file was opened meanwhile (the result is dropped).
    pub fn probe_finished(&mut self, generation: u64, result: Result<MediaInfo, ProbeError>) -> bool {
        if generation != self.generation || self.file.is_none() {
            return false;
        }
        self.probe = match result {
            Ok(info) => {
                let sections = summarize(&info);
                ProbeStatus::Ready {
                    info: Arc::new(info),
                    sections,
                }
            }
            Err(e) => ProbeStatus::Failed(e.to_string()),
        };
        true
    }

    /// The frame-timing sample of `generation` finished (after [`Self::probe_finished`]): the frame rate row learns
    /// whether the rate is variable. `false` when it is stale or there is nothing to update.
    pub fn timing_finished(&mut self, generation: u64, timing: Option<FrameTiming>) -> bool {
        if generation != self.generation {
            return false;
        }
        let ProbeStatus::Ready { info, sections } = &mut self.probe else {
            return false;
        };
        if timing.is_none() || info.frame_timing == timing {
            return false;
        }
        let mut updated = MediaInfo::clone(info);
        updated.frame_timing = timing;
        *sections = summarize(&updated);
        *info = Arc::new(updated);
        true
    }

    /// The user asked for the hash (or asked again after a failure or a cancel).
    pub fn hash_requested(&mut self) -> Option<PathBuf> {
        let file = self.file.clone()?;
        self.hash = HashStatus::Running { done: 0, total: 0 };
        Some(file)
    }

    /// Takes a hash event into account. `false` when it is about another file (a previous one, or another requester's).
    pub fn apply_hash(&mut self, event: &HashEvent) -> bool {
        let path = match event {
            HashEvent::Progress { path, .. }
            | HashEvent::Finished { path, .. }
            | HashEvent::Failed { path, .. }
            | HashEvent::Canceled { path } => path,
        };
        if self.file.as_deref() != Some(path.as_path()) {
            return false;
        }
        self.hash = match event {
            HashEvent::Progress { done, total, .. } => HashStatus::Running {
                done: *done,
                total: *total,
            },
            HashEvent::Finished { md5, from_sidecar, .. } => HashStatus::Ready {
                md5: md5.clone(),
                origin: if *from_sidecar {
                    HashOrigin::Sidecar
                } else {
                    HashOrigin::Computed
                },
            },
            HashEvent::Failed { error, .. } => HashStatus::Failed(error.clone()),
            HashEvent::Canceled { .. } => HashStatus::Canceled,
        };
        true
    }

    /// RustView's record of the open file gave its MD5 (only while that open is still current and the hash still
    /// wanted). `false` when it is about an older open.
    pub fn hash_from_library(&mut self, generation: u64, md5: &str) -> bool {
        if generation != self.generation || !matches!(self.hash, HashStatus::Running { .. }) {
            return false;
        }
        self.hash = HashStatus::Ready {
            md5: md5.to_owned(),
            origin: HashOrigin::Library,
        };
        true
    }

    /// The open file and its MD5, once known.
    pub fn ready_hash(&self) -> Option<(PathBuf, String)> {
        match (&self.file, &self.hash) {
            (Some(file), HashStatus::Ready { md5, .. }) => Some((file.clone(), md5.clone())),
            _ => None,
        }
    }

    /// The probed file, once known.
    pub fn info(&self) -> Option<Arc<MediaInfo>> {
        match &self.probe {
            ProbeStatus::Ready { info, .. } => Some(Arc::clone(info)),
            _ => None,
        }
    }

    pub fn sections_view(&self) -> SectionsView {
        match &self.probe {
            ProbeStatus::Idle => SectionsView {
                state: PanelState::NoFile,
                sections: Vec::new(),
            },
            // The container card already carries the MD5 row (its progress shows while ffprobe works).
            ProbeStatus::Running => SectionsView {
                state: PanelState::Sections,
                sections: vec![InfoSection {
                    title: "Container".into(),
                    rows: vec![InfoRow {
                        label: "Status",
                        value: "Reading media info...".into(),
                    }],
                }],
            },
            ProbeStatus::Ready { sections, .. } => SectionsView {
                state: PanelState::Sections,
                sections: sections.clone(),
            },
            // The hash row still has a place: the container section carries the problem.
            ProbeStatus::Failed(reason) => SectionsView {
                state: PanelState::Sections,
                sections: vec![InfoSection {
                    title: "Container".into(),
                    rows: vec![InfoRow {
                        label: "Problem",
                        value: reason.clone(),
                    }],
                }],
            },
        }
    }

    pub fn hash_view(&self) -> HashView {
        let plain = |text: &str, note: String, can_compute: bool| HashView {
            text: text.to_owned(),
            note,
            progress: -1.0,
            is_digest: false,
            can_compute,
            can_cancel: false,
        };
        match &self.hash {
            HashStatus::Idle => plain("", String::new(), false),
            HashStatus::NotStarted => plain("Not computed", String::new(), true),
            HashStatus::Running { done, total } if *total > 0 => {
                let fraction = (*done as f64 / *total as f64).clamp(0.0, 1.0);
                HashView {
                    text: format!("Computing, {:.0} %", fraction * 100.0),
                    note: format!("{} of {}", format_size(*done), format_size(*total)),
                    progress: az::Az::az::<f32>(fraction),
                    is_digest: false,
                    can_compute: false,
                    can_cancel: true,
                }
            }
            HashStatus::Running { .. } => HashView {
                can_cancel: true,
                ..plain("Computing...", String::new(), false)
            },
            HashStatus::Ready { md5, origin } => HashView {
                is_digest: true,
                ..plain(
                    md5,
                    match origin {
                        HashOrigin::Library => "From the RustView library".into(),
                        HashOrigin::Sidecar => "From the sidecar file".into(),
                        HashOrigin::Computed => String::new(),
                    },
                    false,
                )
            },
            HashStatus::Failed(error) => plain("Not available", error.clone(), true),
            HashStatus::Canceled => plain("Canceled", String::new(), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use rustcut_core::media::decode;

    use super::*;

    fn info() -> MediaInfo {
        decode(br#"{"streams": [{"index": 0, "codec_type": "video", "width": 640, "height": 360}], "format": {"duration": "8.0"}}"#)
            .unwrap()
    }

    #[test]
    fn a_probe_of_an_older_open_is_dropped() {
        let mut s = MediaPanelState::default();
        assert_eq!(s.sections_view().state, PanelState::NoFile);
        let first = s.opening(Path::new("/v/a.mp4"), true);
        let reading = s.sections_view();
        assert_eq!(reading.state, PanelState::Sections);
        assert_eq!(
            reading
                .sections
                .first()
                .and_then(|s| s.rows.first())
                .map(|r| r.value.as_str()),
            Some("Reading media info...")
        );
        let second = s.opening(Path::new("/v/b.mp4"), true);
        assert!(!s.probe_finished(first, Ok(info())));
        assert_eq!(s.probe, ProbeStatus::Running);
        assert!(s.probe_finished(second, Ok(info())));
        let view = s.sections_view();
        assert_eq!(view.state, PanelState::Sections);
        assert_eq!(view.sections.first().map(|s| s.title.as_str()), Some("Container"));
        assert_eq!(s.info().map(|i| i.streams.len()), Some(1));
    }

    #[test]
    fn the_timing_sample_refines_the_frame_rate_row() {
        let mut s = MediaPanelState::default();
        let g = s.opening(Path::new("/v/a.mkv"), false);
        let info = decode(
            br#"{"streams": [{"index": 0, "codec_type": "video", "r_frame_rate": "30/1", "avg_frame_rate": "30/1"}]}"#,
        )
        .unwrap();
        let frame_rate = |s: &MediaPanelState| {
            s.sections_view()
                .sections
                .iter()
                .flat_map(|section| section.rows.iter())
                .find(|row| row.label == "Frame rate")
                .map(|row| row.value.clone())
        };
        assert!(!s.timing_finished(g, None), "nothing to refine before the probe");
        assert!(s.probe_finished(g, Ok(info)));
        assert_eq!(frame_rate(&s).as_deref(), Some("30 fps"));
        let uneven = FrameTiming {
            frames: 80,
            span_secs: 3.9,
            min_fps: 10.0,
            max_fps: 30.0,
            variable: true,
        };
        assert!(!s.timing_finished(g + 1, Some(uneven)), "stale");
        assert!(!s.timing_finished(g, None), "a failed sample changes nothing");
        assert!(s.timing_finished(g, Some(uneven)));
        assert_eq!(frame_rate(&s).as_deref(), Some("Variable, 10 to 30 fps (base 30 fps)"));
        assert_eq!(s.info().and_then(|i| i.frame_timing), Some(uneven));
    }

    #[test]
    fn a_failed_probe_shows_the_problem_in_the_container_section() {
        let mut s = MediaPanelState::default();
        let g = s.opening(Path::new("/v/broken.mp4"), false);
        assert!(s.probe_finished(
            g,
            Err(ProbeError::Unreadable(
                "Invalid data found when processing input".into()
            ))
        ));
        let view = s.sections_view();
        assert_eq!(view.state, PanelState::Sections);
        assert_eq!(
            view.sections,
            vec![InfoSection {
                title: "Container".into(),
                rows: vec![InfoRow {
                    label: "Problem",
                    value: "Invalid data found when processing input".into()
                }]
            }]
        );
        assert_eq!(s.info(), None);
    }

    #[test]
    fn hash_events_of_the_open_file_drive_the_row() {
        let mut s = MediaPanelState::default();
        assert_eq!(s.hash_view().text, "");
        s.opening(Path::new("/v/a.mp4"), true);
        let view = s.hash_view();
        assert_eq!(view.text, "Computing...");
        assert!(view.can_cancel && !view.can_compute && view.progress < 0.0);

        let other = PathBuf::from("/v/other.mp4");
        assert!(!s.apply_hash(&HashEvent::Finished {
            path: other,
            md5: "x".into(),
            from_sidecar: false
        }));

        let path = PathBuf::from("/v/a.mp4");
        assert!(s.apply_hash(&HashEvent::Progress {
            path: path.clone(),
            done: 250_000_000,
            total: 1_000_000_000
        }));
        let view = s.hash_view();
        assert_eq!(view.text, "Computing, 25 %");
        assert_eq!(view.note, "250 MB of 1 GB");
        assert!((view.progress - 0.25).abs() < 1e-6);

        assert!(s.apply_hash(&HashEvent::Finished {
            path: path.clone(),
            md5: "8f618d1ebd606025257057ee18536e64".into(),
            from_sidecar: true
        }));
        let view = s.hash_view();
        assert_eq!(view.text, "8f618d1ebd606025257057ee18536e64");
        assert_eq!(view.note, "From the sidecar file");
        assert!(view.is_digest && !view.can_cancel && !view.can_compute);

        assert!(s.apply_hash(&HashEvent::Failed {
            path: path.clone(),
            error: "/v/a.mp4: Permission denied".into()
        }));
        let view = s.hash_view();
        assert_eq!(
            (view.text.as_str(), view.note.as_str()),
            ("Not available", "/v/a.mp4: Permission denied")
        );
        assert!(view.can_compute);

        assert!(s.apply_hash(&HashEvent::Canceled { path }));
        assert_eq!(s.hash_view().text, "Canceled");
        assert_eq!(s.hash_requested(), Some(PathBuf::from("/v/a.mp4")));
        assert_eq!(s.hash_view().text, "Computing...");
    }

    #[test]
    fn rustviews_hash_counts_only_for_the_current_open() {
        let mut s = MediaPanelState::default();
        let first = s.opening(Path::new("/v/a.mp4"), true);
        let second = s.opening(Path::new("/v/b.mp4"), true);
        assert!(!s.hash_from_library(first, "8f618d1ebd606025257057ee18536e64"));
        assert!(s.hash_from_library(second, "8f618d1ebd606025257057ee18536e64"));
        let view = s.hash_view();
        assert_eq!(view.note, "From the RustView library");
        assert_eq!(
            s.ready_hash(),
            Some((PathBuf::from("/v/b.mp4"), "8f618d1ebd606025257057ee18536e64".into()))
        );
        assert!(
            !s.hash_from_library(second, "00000000000000000000000000000000"),
            "a known hash is not replaced"
        );
        s.register_hash = true;
        s.opening(Path::new("/v/c.mp4"), false);
        assert!(!s.register_hash && s.ready_hash().is_none());
    }

    #[test]
    fn without_the_known_source_warning_the_hash_waits_for_the_user() {
        let mut s = MediaPanelState::default();
        assert_eq!(s.hash_requested(), None, "nothing to hash without a file");
        s.opening(Path::new("/v/a.mp4"), false);
        let view = s.hash_view();
        assert_eq!(view.text, "Not computed");
        assert!(view.can_compute && !view.can_cancel);
    }
}
