//! The Media Info panel and the source hash: when a file opens, ffprobe runs on the runtime (the previous probe is
//! aborted, which kills its ffprobe; the frame-timing sample follows as a second call) and the hash service gets the file (sidecar first, else MD5 in the background) when
//! the known-source warning needs it; otherwise the panel offers "Compute". Results land in the shared
//! [`MediaPanelState`] under a short lock; the UI thread is only told to refresh, through the [`UiDispatcher`] at
//! `rate::PROGRESS`, and reads the current views when the refresh runs, so a late result can never show for the wrong file.
//! No file system call happens on the UI thread.

pub mod known_source;
mod state;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use parking_lot::Mutex;
use rustcut_core::hashing::{HashCommand, HashEvent, HashService};
use rustcut_core::media::{InfoSection, MediaInfo, ProbeOptions, probe, probe_frame_timing};
use rustcut_core::settings::Settings;
use slint::{ComponentHandle as _, ModelRc, VecModel};
use tokio::sync::broadcast::error::RecvError;

pub use state::{HashStatus, HashView, MediaPanelState, PanelState, ProbeStatus, SectionsView};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::{KnownSource, MainWindow, MediaInfoPanel, MediaInfoRow, MediaInfoSection};

/// Coalesced refreshes of the panel body and of the MD5 row.
const SECTIONS_KEY: CoalesceKey = CoalesceKey::new("media-info", 0);
const HASH_KEY: CoalesceKey = CoalesceKey::new("media-hash", 0);

pub struct MediaInfoBridge {
    state: Arc<Mutex<MediaPanelState>>,
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    hashing: HashService,
    probe_options: ProbeOptions,
    /// Hash every opened file (the known-source warning needs the hash).
    auto_hash: bool,
    probe_task: RefCell<Option<tokio::task::AbortHandle>>,
    events_task: tokio::task::AbortHandle,
}

impl MediaInfoBridge {
    /// Handles the panel's callbacks and the hash service's events from now on. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        hashing: HashService,
        settings: &Settings,
    ) -> Rc<Self> {
        let state = Arc::new(Mutex::new(MediaPanelState::default()));
        let events_task = runtime
            .spawn(forward_hash_events(
                hashing.clone(),
                Arc::clone(&state),
                ui.clone(),
                window.as_weak(),
            ))
            .abort_handle();
        let bridge = Rc::new(Self {
            state,
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            hashing,
            probe_options: ProbeOptions {
                program: settings.advanced.ffprobe.clone(),
                ..ProbeOptions::default()
            },
            auto_hash: settings.warnings.known_source,
            probe_task: RefCell::new(None),
            events_task,
        });
        let panel = window.global::<MediaInfoPanel<'_>>();
        let this = Rc::downgrade(&bridge);
        panel.on_compute_hash(move || {
            if let Some(bridge) = this.upgrade() {
                bridge.compute_hash();
            }
        });
        let this = Rc::downgrade(&bridge);
        panel.on_cancel_hash(move || {
            if let Some(bridge) = this.upgrade() {
                bridge.cancel_hash();
            }
        });
        let banner = window.as_weak();
        window.global::<KnownSource<'_>>().on_dismiss(move || {
            if let Some(window) = banner.upgrade() {
                known_source::show(&window, None);
            }
        });
        bridge.refresh();
        bridge
    }

    /// A file was opened (UI thread; the path was checked on the runtime): probe it, and hash it when wanted.
    pub fn file_opened(&self, path: &Path) {
        let (generation, previous) = {
            let mut state = self.state.lock();
            let previous = state.file.clone();
            (state.opening(path, self.auto_hash), previous)
        };
        if let Some(previous) = previous.filter(|p| p != path) {
            self.send_hash(HashCommand::Cancel { path: previous });
        }
        if let Some(window) = self.window.upgrade() {
            known_source::show(&window, None);
        }
        if let Some(task) = self.probe_task.borrow_mut().take() {
            task.abort();
        }
        let file = path.to_path_buf();
        let options = self.probe_options.clone();
        let state = Arc::clone(&self.state);
        let ui = self.ui.clone();
        let window = self.window.clone();
        let task = self.runtime.spawn(async move {
            let result = probe(&file, &options).await;
            let primary = match &result {
                Ok(info) => {
                    tracing::info!(path = %file.display(), streams = info.streams.len(), "[Media] Probed");
                    info.primary_video().map(|(stream, _)| stream.index)
                }
                Err(e) => {
                    tracing::warn!(path = %file.display(), error = %e, "[Media] Probe failed");
                    None
                }
            };
            if !state.lock().probe_finished(generation, result) {
                return;
            }
            post_sections(&ui, &state, &window);
            // The variable-frame-rate check comes second: its demuxing may be slow, the description is shown already.
            let Some(index) = primary else { return };
            match probe_frame_timing(&file, index, &options).await {
                Ok(timing) => {
                    if state.lock().timing_finished(generation, timing) {
                        post_sections(&ui, &state, &window);
                    }
                }
                Err(e) => tracing::warn!(path = %file.display(), error = %e, "[Media] Frame timing not sampled"),
            }
        });
        *self.probe_task.borrow_mut() = Some(task.abort_handle());
        if self.auto_hash {
            self.send_hash(HashCommand::Hash {
                path: path.to_path_buf(),
                reply: None,
            });
        }
        self.refresh();
    }

    /// What the probe found about the open file, once it is known.
    pub fn current(&self) -> Option<Arc<MediaInfo>> {
        self.state.lock().info()
    }

    fn compute_hash(&self) {
        let file = self.state.lock().hash_requested();
        if let Some(path) = file {
            tracing::info!(path = %path.display(), "[Media] Hash requested");
            self.send_hash(HashCommand::Hash { path, reply: None });
        }
        self.refresh_hash();
    }

    fn cancel_hash(&self) {
        let file = self.state.lock().file.clone();
        if let Some(path) = file {
            self.send_hash(HashCommand::Cancel { path });
        }
    }

    fn send_hash(&self, command: HashCommand) {
        if self.hashing.handle.send(command).is_err() {
            tracing::warn!("[Media] The hash service has stopped");
        }
    }

    /// Shows the current state at once (UI thread).
    fn refresh(&self) {
        if let Some(window) = self.window.upgrade() {
            let (sections, hash) = {
                let state = self.state.lock();
                (state.sections_view(), state.hash_view())
            };
            show_sections(&window, &sections);
            show_hash(&window, &hash);
        }
    }

    fn refresh_hash(&self) {
        if let Some(window) = self.window.upgrade() {
            let hash = self.state.lock().hash_view();
            show_hash(&window, &hash);
        }
    }
}

impl Drop for MediaInfoBridge {
    fn drop(&mut self) {
        self.events_task.abort();
        if let Some(task) = self.probe_task.get_mut().take() {
            task.abort();
        }
    }
}

/// Applies the hash service's events to the panel state and refreshes the MD5 row (runtime).
async fn forward_hash_events(
    hashing: HashService,
    state: Arc<Mutex<MediaPanelState>>,
    ui: UiDispatcher,
    window: slint::Weak<MainWindow>,
) {
    let mut events = hashing.events.subscribe();
    loop {
        match events.recv().await {
            Ok(event) => {
                if !state.lock().apply_hash(&event) {
                    continue;
                }
                if let HashEvent::Finished { path, md5, .. } = &event {
                    known_source::source_hash_ready(path, md5);
                }
                post_hash(&ui, &state, &window);
            }
            Err(RecvError::Lagged(missed)) => {
                // A missed Finished would leave the row computing forever: ask again (answered from memory).
                tracing::warn!(missed, "[Media] Hash events missed; asking again");
                let pending = {
                    let state = state.lock();
                    matches!(state.hash, HashStatus::Running { .. })
                        .then(|| state.file.clone())
                        .flatten()
                };
                if let Some(path) = pending {
                    let _ = hashing.handle.send(HashCommand::Hash { path, reply: None });
                }
            }
            Err(RecvError::Closed) => break,
        }
    }
}

fn post_sections(ui: &UiDispatcher, state: &Arc<Mutex<MediaPanelState>>, window: &slint::Weak<MainWindow>) {
    let state = Arc::clone(state);
    let window = window.clone();
    ui.post_latest(SECTIONS_KEY, rate::PROGRESS, move || {
        if let Some(window) = window.upgrade() {
            let view = state.lock().sections_view();
            show_sections(&window, &view);
        }
    });
}

fn post_hash(ui: &UiDispatcher, state: &Arc<Mutex<MediaPanelState>>, window: &slint::Weak<MainWindow>) {
    let state = Arc::clone(state);
    let window = window.clone();
    ui.post_latest(HASH_KEY, rate::PROGRESS, move || {
        if let Some(window) = window.upgrade() {
            let view = state.lock().hash_view();
            show_hash(&window, &view);
        }
    });
}

/// Puts the panel body into the `MediaInfoPanel` global (UI thread).
fn show_sections(window: &MainWindow, view: &SectionsView) {
    let panel = window.global::<MediaInfoPanel<'_>>();
    panel.set_state(view.state as i32);
    let sections: Vec<MediaInfoSection> = view.sections.iter().map(to_slint).collect();
    panel.set_sections(ModelRc::new(VecModel::from(sections)));
}

fn to_slint(section: &InfoSection) -> MediaInfoSection {
    let rows: Vec<MediaInfoRow> = section
        .rows
        .iter()
        .map(|row| MediaInfoRow {
            label: row.label.into(),
            value: row.value.as_str().into(),
        })
        .collect();
    MediaInfoSection {
        title: section.title.as_str().into(),
        rows: ModelRc::new(VecModel::from(rows)),
    }
}

/// Puts the MD5 row into the `MediaInfoPanel` global (UI thread).
fn show_hash(window: &MainWindow, view: &HashView) {
    let panel = window.global::<MediaInfoPanel<'_>>();
    panel.set_hash_text(view.text.as_str().into());
    panel.set_hash_note(view.note.as_str().into());
    panel.set_hash_progress(view.progress);
    panel.set_hash_is_digest(view.is_digest);
    panel.set_hash_can_compute(view.can_compute);
    panel.set_hash_can_cancel(view.can_cancel);
}
