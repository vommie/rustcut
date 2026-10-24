//! The Media Info panel and the source hash: when a file opens, ffprobe runs on the runtime (the previous probe is
//! aborted, which kills its ffprobe; the frame-timing sample follows as a second call), and when the known-source warning
//! needs the hash it is taken from RustView's record of the file (ADR-6, no disk read), else from the hash service
//! (sidecar first, else MD5 in the background); otherwise the panel offers "Compute". Once the hash is known, RustView
//! is asked whether the file was cut before (the banner). Results land in the shared
//! [`MediaPanelState`] under a short lock; the UI thread is only told to refresh, through the [`UiDispatcher`] at
//! `rate::PROGRESS`, and reads the current views when the refresh runs, so a late result can never show for the wrong file.
//! No file system call happens on the UI thread.

pub mod known_source;
mod state;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use rustcut_core::hashing::{HashCommand, HashEvent, HashService};
use rustcut_core::media::{InfoSection, MediaInfo, ProbeOptions, probe, probe_frame_timing};
use rustcut_core::rustview::{SyncCommand, SyncHandle, trusted_hash};
use rustcut_core::settings::Settings;
use slint::{ComponentHandle as _, ModelRc, VecModel};
use tokio::sync::broadcast::error::RecvError;

pub use state::{HashOrigin, HashStatus, HashView, MediaPanelState, PanelState, ProbeStatus, SectionsView};

use crate::ui::dispatcher::{CoalesceKey, UiDispatcher, rate};
use crate::{MainWindow, MediaInfoPanel, MediaInfoRow, MediaInfoSection};

/// The function that sees the probe result of the open file.
type ProbeHook = Box<dyn Fn(Option<&Arc<MediaInfo>>)>;

thread_local! {
    /// Called on the UI thread whenever the probe of the open file has news (the filters need the picture's size and
    /// rotation, the export card the audio tracks). Results are posted from the runtime as `Send` closures, which
    /// cannot carry UI-thread objects.
    static PROBE_HOOK: RefCell<Vec<ProbeHook>> = const { RefCell::new(Vec::new()) };
}

/// Adds a function that sees the probe results (UI thread).
pub fn add_probe_hook(hook: impl Fn(Option<&Arc<MediaInfo>>) + 'static) {
    PROBE_HOOK.with(|h| h.borrow_mut().push(Box::new(hook)));
}

fn notify_probe(info: Option<&Arc<MediaInfo>>) {
    PROBE_HOOK.with(|hooks| {
        for hook in hooks.borrow().iter() {
            hook(info);
        }
    });
}

/// Coalesced refreshes of the panel body and of the MD5 row.
const SECTIONS_KEY: CoalesceKey = CoalesceKey::new("media-info", 0);
const HASH_KEY: CoalesceKey = CoalesceKey::new("media-hash", 0);

pub struct MediaInfoBridge {
    state: Arc<Mutex<MediaPanelState>>,
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    hashing: HashService,
    /// The RustView sync actor (lookups of the source's record and hash); `None` without RustView.
    sync: Option<SyncHandle>,
    probe_options: RefCell<ProbeOptions>,
    /// Hash every opened file and look it up in RustView (the known-source warning).
    auto_hash: Arc<AtomicBool>,
    probe_task: RefCell<Option<tokio::task::AbortHandle>>,
    events_task: tokio::task::AbortHandle,
}

impl MediaInfoBridge {
    /// Whether the next opened video is hashed and looked up in RustView (the known-source notice is not routed to
    /// "nothing", changed in the Settings window).
    pub fn set_known_source(&self, on: bool) {
        self.auto_hash.store(on, Ordering::Relaxed);
    }

    /// The ffprobe that reads the next opened video (changed in the Settings window).
    pub fn set_ffprobe(&self, program: Option<std::path::PathBuf>) {
        self.probe_options.borrow_mut().program = program;
    }

    /// Whether a hash RustCut computes goes into a new sidecar next to the video (settings `hashing.write_sidecar`).
    pub fn set_write_sidecar(&self, on: bool) {
        if self.hashing.handle.send(HashCommand::SetWriteSidecar(on)).is_err() {
            tracing::warn!("[Media] The hash service is gone; the sidecar setting is not applied");
        }
    }

    /// Handles the panel's callbacks and the hash service's events from now on. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        hashing: HashService,
        sync: Option<SyncHandle>,
        settings: &Settings,
    ) -> Rc<Self> {
        let state = Arc::new(Mutex::new(MediaPanelState::default()));
        let known_source = Arc::new(AtomicBool::new(known_source::wanted(settings)));
        let events_task = runtime
            .spawn(forward_hash_events(
                hashing.clone(),
                Lookup {
                    sync: sync.clone(),
                    known_source: Arc::clone(&known_source),
                    state: Arc::clone(&state),
                    ui: ui.clone(),
                    window: window.as_weak(),
                },
            ))
            .abort_handle();
        let bridge = Rc::new(Self {
            state,
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            hashing,
            sync,
            probe_options: RefCell::new(ProbeOptions {
                program: settings.advanced.ffprobe.clone(),
                ..ProbeOptions::default()
            }),
            auto_hash: known_source,
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
        bridge.refresh();
        bridge
    }

    /// A file was opened (UI thread; the path was checked on the runtime): probe it, and hash it when wanted.
    /// `by_rustcut`: RustCut opened it itself (a job loaded into the editor, the way back from the probe): its source
    /// is known, no banner says so.
    pub fn file_opened(&self, path: &Path, by_rustcut: bool) {
        let (generation, previous) = {
            let mut state = self.state.lock();
            let previous = state.file.clone();
            let generation = state.opening(path, self.auto_hash.load(Ordering::Relaxed));
            state.known_quiet = by_rustcut;
            (generation, previous)
        };
        if let Some(previous) = previous.filter(|p| p != path) {
            self.send_hash(HashCommand::Cancel { path: previous });
        }
        known_source::show(None);
        if let Some(task) = self.probe_task.borrow_mut().take() {
            task.abort();
        }
        let file = path.to_path_buf();
        let options = self.probe_options.borrow().clone();
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
        if self.auto_hash.load(Ordering::Relaxed) {
            self.request_hash(path.to_path_buf(), generation);
        }
        self.refresh();
    }

    /// The open file's MD5, once known (a job of it carries it to RustView's hash list).
    pub fn source_hash(&self, source: &Path) -> Option<String> {
        self.state
            .lock()
            .ready_hash()
            .filter(|(file, _)| file == source)
            .map(|(_, md5)| md5)
    }

    /// A job of `source` was queued. When it went without the hash (not known yet at saving), the hash is registered
    /// in RustView as soon as it is known (only sources of jobs are registered, bug 3).
    pub fn job_saved(&self, source: &Path, with_hash: bool) {
        if with_hash {
            return;
        }
        let ready = {
            let mut state = self.state.lock();
            if state.file.as_deref() != Some(source) {
                return;
            }
            let ready = state.ready_hash().map(|(_, md5)| md5);
            state.register_hash = ready.is_none();
            ready
        };
        if let Some(md5) = ready {
            register_hash(self.sync.as_ref(), md5);
        }
    }

    /// Asks RustView for the file's record first (its hash when the record is current), else the hash service.
    fn request_hash(&self, path: std::path::PathBuf, generation: u64) {
        let Some(sync) = self.sync.clone() else {
            self.send_hash(HashCommand::Hash { path, reply: None });
            return;
        };
        let lookup = Lookup {
            sync: Some(sync.clone()),
            known_source: Arc::clone(&self.auto_hash),
            state: Arc::clone(&self.state),
            ui: self.ui.clone(),
            window: self.window.clone(),
        };
        let hashing = self.hashing.handle.clone();
        self.runtime.spawn(async move {
            match library_hash(&sync, &path).await {
                Some(md5) => {
                    if lookup.state.lock().hash_from_library(generation, &md5) {
                        tracing::info!(path = %path.display(), "[Media] Hash taken from the RustView library");
                        post_hash(&lookup.ui, &lookup.state, &lookup.window);
                        lookup.hash_known(&path, &md5).await;
                    }
                }
                None => {
                    if hashing.send(HashCommand::Hash { path, reply: None }).is_err() {
                        tracing::warn!("[Media] The hash service has stopped");
                    }
                }
            }
        });
    }

    /// What the probe found about the open file, once it is known.
    pub fn current(&self) -> Option<Arc<MediaInfo>> {
        self.state.lock().info()
    }

    fn compute_hash(&self) {
        let (file, generation) = {
            let mut state = self.state.lock();
            (state.hash_requested(), state.generation)
        };
        if let Some(path) = file {
            tracing::info!(path = %path.display(), "[Media] Hash requested");
            self.request_hash(path, generation);
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

/// The MD5 from RustView's record of `path` when that record describes the file as it is now (runtime).
async fn library_hash(sync: &SyncHandle, path: &Path) -> Option<String> {
    let answer = sync
        .request(|reply| SyncCommand::GetMedia {
            path: path.to_path_buf(),
            reply,
        })
        .await;
    let media = match answer {
        Ok(Ok(found)) if found.found => found.media?,
        Ok(Ok(_)) => return None,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "[Media] RustView not asked for the hash");
            return None;
        }
        Err(_gone) => return None,
    };
    let meta = tokio::fs::metadata(path).await.ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())?;
    let hash = trusted_hash(&media, meta.len(), modified);
    if hash.is_none() {
        tracing::debug!(path = %path.display(), "[Media] RustView's record of the file is not current; reading it");
    }
    hash
}

/// Sends the hash of a job's source to RustView's hash list.
fn register_hash(sync: Option<&SyncHandle>, md5: String) {
    if let Some(sync) = sync
        && sync.send(SyncCommand::InsertHash { md5 }).is_err()
    {
        tracing::debug!("[Media] The RustView sync has stopped");
    }
}

/// What the known-source check needs on the runtime.
struct Lookup {
    sync: Option<SyncHandle>,
    known_source: Arc<AtomicBool>,
    state: Arc<Mutex<MediaPanelState>>,
    ui: UiDispatcher,
    window: slint::Weak<MainWindow>,
}

impl Lookup {
    /// The open file's MD5 is known: register it if a job was saved without it, and ask RustView whether the file was
    /// cut before (the banner shows only while that file is still open).
    async fn hash_known(&self, path: &Path, md5: &str) {
        let register = std::mem::take(&mut self.state.lock().register_hash);
        if register {
            register_hash(self.sync.as_ref(), md5.to_owned());
        }
        let Some(sync) = self.sync.as_ref().filter(|_| self.known_source.load(Ordering::Relaxed)) else {
            return;
        };
        let generation = {
            let state = self.state.lock();
            if state.file.as_deref() != Some(path) {
                return;
            }
            state.generation
        };
        let view = known_source::lookup(sync, path, md5).await;
        let state = Arc::clone(&self.state);
        self.ui.post(move || {
            let (current, quiet) = {
                let state = state.lock();
                (state.generation == generation, state.known_quiet)
            };
            if current && !quiet {
                known_source::show(view.as_ref());
            }
        });
    }
}

/// Applies the hash service's events to the panel state and refreshes the MD5 row (runtime).
async fn forward_hash_events(hashing: HashService, lookup: Lookup) {
    let Lookup { state, ui, window, .. } = &lookup;
    let mut events = hashing.events.subscribe();
    loop {
        match events.recv().await {
            Ok(event) => {
                if !state.lock().apply_hash(&event) {
                    continue;
                }
                post_hash(ui, state, window);
                if let HashEvent::Finished { path, md5, .. } = &event {
                    lookup.hash_known(path, md5).await;
                }
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
            let (view, info) = {
                let state = state.lock();
                (state.sections_view(), state.info())
            };
            show_sections(&window, &view);
            notify_probe(info.as_ref());
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
