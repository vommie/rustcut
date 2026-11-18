//! A plugin's folder view in the main window (plugin API 1.4, Q-NF2): where the open video stands in its folder (the
//! bar between the plugin's buttons) and the list of its neighbors (a right click on them).
//!
//! The position is read on the runtime with the plugin's request through the plugins' one folder cache, when a video
//! opens and when the watched folder changes; the bar's properties change in place (no element is made anew, a shown
//! tooltip stays and follows), and a read that takes longer than 0.3 s lets the last state pulse.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustcut_core::folder_walk::dolphin::props::DolphinPaths;
use rustcut_core::folder_walk::order::SortSpec;
use rustcut_core::folder_walk::{self, OrderSource};
use rustcut_core::plugins::{FolderView, walk_request};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::progress_text as text;
use super::{BRIDGE, CommandsBridge};
use crate::{PluginCommands, PluginProgressView, TipRow};

/// How long a read may take before the bar pulses.
const PULSE_AFTER: Duration = Duration::from_millis(300);
/// Changes of the folder that come together are read once.
const SETTLE: Duration = Duration::from_millis(250);

/// What the navigator takes from the last listing.
pub(super) struct Snapshot {
    pub files: Arc<Vec<PathBuf>>,
    pub current: Option<usize>,
    pub folder: PathBuf,
    pub order: String,
}

/// Where the open video stands, as last read.
struct Located {
    files: Arc<Vec<PathBuf>>,
    current: Option<usize>,
    sort: SortSpec,
    source: OrderSource,
    folder: PathBuf,
}

#[derive(Default)]
pub(super) struct State {
    /// The plugin whose view is shown, and the view.
    owner: Option<(String, FolderView)>,
    video: Option<PathBuf>,
    /// Counts the reads: an answer of an older one is dropped.
    generation: u64,
    located: Option<Located>,
    error: Option<String>,
    loading: bool,
}

#[derive(Default)]
pub(super) struct Progress {
    state: RefCell<State>,
    pulse: slint::Timer,
}

impl CommandsBridge {
    /// The folder view of the plugins (the first running plugin with one); a change reads again.
    pub(super) fn set_owner(&self, owner: Option<(String, FolderView)>) {
        let changed = self.progress.state.borrow().owner != owner;
        if owner.as_ref().is_none_or(|(_, v)| !v.neighbors)
            && let Some(window) = self.window.upgrade()
            && window.global::<crate::FolderNavigator<'_>>().get_open()
        {
            window.global::<crate::FolderNavigator<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
        if changed {
            self.progress.state.borrow_mut().owner = owner;
            self.refresh_progress();
        }
    }

    pub(super) fn video_opened(&self, path: &Path) {
        self.progress.state.borrow_mut().video = Some(path.to_path_buf());
        self.refresh_progress();
    }

    /// The cache's watcher saw a change: the open video's folder is read again (only that folder is watched).
    pub(super) fn folder_changed(&self) {
        let folder = self
            .progress
            .state
            .borrow()
            .video
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        if folder.is_some() && folder == self.folder_cache.watched_folder() {
            self.refresh_progress();
        }
    }

    /// Follows the cache's changes on the runtime, settled, onto the UI thread.
    pub(super) fn follow_folder(&self) {
        let mut changes = self.folder_cache.changes();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            while changes.changed().await.is_ok() {
                tokio::time::sleep(SETTLE).await;
                changes.borrow_and_update();
                ui.post(|| {
                    if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) {
                        bridge.folder_changed();
                    }
                });
            }
        });
    }

    fn refresh_progress(&self) {
        let (owner, video, generation) = {
            let mut st = self.progress.state.borrow_mut();
            st.generation = st.generation.wrapping_add(1);
            (st.owner.clone(), st.video.clone(), st.generation)
        };
        let Some((_, view)) = owner.filter(|(_, v)| v.progress || v.neighbors) else {
            self.publish_progress();
            return;
        };
        let Some(video) = video else {
            self.publish_progress();
            return;
        };
        let weak = std::rc::Weak::clone(&self.me);
        self.progress
            .pulse
            .start(slint::TimerMode::SingleShot, PULSE_AFTER, move || {
                if let Some(bridge) = weak.upgrade() {
                    bridge.progress.state.borrow_mut().loading = true;
                    bridge.publish_progress();
                }
            });
        let request = walk_request(&video, view.request);
        let cache = Arc::clone(&self.folder_cache);
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let paths = DolphinPaths::of_user();
            let result = folder_walk::list(&request, paths.as_ref(), Some(&cache)).await;
            let folder = request.current.parent().map(Path::to_path_buf).unwrap_or_default();
            ui.post(move || {
                let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) else {
                    return;
                };
                bridge.located(generation, folder, result);
            });
        });
    }

    fn located(&self, generation: u64, folder: PathBuf, result: Result<folder_walk::Listing, String>) {
        {
            let mut st = self.progress.state.borrow_mut();
            if st.generation != generation {
                return;
            }
            st.loading = false;
            match result {
                Ok(listing) => {
                    st.error = None;
                    st.located = Some(Located {
                        current: listing.current,
                        files: Arc::new(listing.files),
                        sort: listing.sort,
                        source: listing.source,
                        folder,
                    });
                }
                Err(error) => {
                    tracing::info!(
                        "[NextInFolder] The folder view cannot read {}: {error}",
                        folder.display()
                    );
                    st.error = Some(error);
                    st.located = None;
                }
            }
        }
        self.progress.pulse.stop();
        self.publish_progress();
        if self
            .window
            .upgrade()
            .is_some_and(|w| w.global::<crate::FolderNavigator<'_>>().get_open())
        {
            self.navigator_listing();
        }
    }

    /// The last listing for the folder navigator (Q-NF3).
    pub(super) fn located_snapshot(&self) -> Option<Snapshot> {
        let st = self.progress.state.borrow();
        let located = st.located.as_ref()?;
        Some(Snapshot {
            files: Arc::clone(&located.files),
            current: located.current,
            folder: located.folder.clone(),
            order: text::order_chip(&located.sort, &located.source),
        })
    }

    fn publish_progress(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let global = window.global::<PluginCommands<'_>>();
        let st = self.progress.state.borrow();
        let shown = st.owner.as_ref().is_some_and(|(_, v)| v.progress);
        // The last fraction stays while a new read runs or when the position is unknown: nothing jumps.
        let mut view = PluginProgressView {
            shown,
            fraction: global.get_progress().fraction,
            ..PluginProgressView::default()
        };
        let row = |label: &str, value: String| TipRow {
            label: label.into(),
            value: value.into(),
            heading: false,
        };
        let rows = |items: Vec<TipRow>| ModelRc::new(VecModel::from(items));
        view.loading = st.loading;
        match (&st.video, &st.located, &st.error) {
            (None, _, _) => {
                view.dim = true;
                view.title = "No video is open".into();
            }
            (Some(_), _, Some(error)) => {
                view.dim = true;
                view.title = "The folder cannot be read".into();
                view.rows = rows(vec![row("Why", error.clone())]);
            }
            (Some(_), Some(located), None) => {
                let total = located.files.len();
                let order = row("Order", text::order_words(&located.sort, &located.source));
                let folder = row("Folder", located.folder.display().to_string());
                view.muted = located.source != OrderSource::FileManager;
                if let Some(index) = located.current {
                    let position = index + 1;
                    view.fraction = text::fraction(position, total);
                    view.texts = ModelRc::new(VecModel::from(
                        text::candidates(position, total)
                            .into_iter()
                            .map(Into::into)
                            .collect::<Vec<slint::SharedString>>(),
                    ));
                    view.title = text::title(position, total).into();
                    view.rows = rows(vec![order, folder]);
                } else {
                    view.dim = true;
                    view.title = "This video is not in its folder".into();
                    view.rows = rows(vec![row("Why", "It was moved, renamed or deleted".to_owned()), folder]);
                }
            }
            (Some(_), None, None) => {
                // The first read runs.
                view.title = "Reading the folder ...".into();
            }
        }
        drop(st);
        let same = {
            let old = global.get_progress();
            old.shown == view.shown
                && (old.fraction - view.fraction).abs() < f32::EPSILON
                && old.title == view.title
                && old.dim == view.dim
                && old.muted == view.muted
                && old.loading == view.loading
                && same_texts(&old.texts, &view.texts)
                && same_rows(&old.rows, &view.rows)
        };
        if !same {
            global.set_progress(view);
        }
    }

    /// The folder navigator under the element at `anchor` (window coordinates; Q-NF3), or closed when it is open.
    pub(super) fn open_neighbors(&self, anchor: [f32; 4]) {
        let rows = {
            let st = self.progress.state.borrow();
            match st.owner.as_ref() {
                Some((_, view)) if view.neighbors => view.rows,
                _ => return,
            }
        };
        self.toggle_navigator(anchor, usize::try_from(rows).unwrap_or(15));
    }
}

fn same_texts(a: &ModelRc<slint::SharedString>, b: &ModelRc<slint::SharedString>) -> bool {
    use slint::Model as _;
    a.row_count() == b.row_count() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

fn same_rows(a: &ModelRc<TipRow>, b: &ModelRc<TipRow>) -> bool {
    use slint::Model as _;
    a.row_count() == b.row_count() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}
