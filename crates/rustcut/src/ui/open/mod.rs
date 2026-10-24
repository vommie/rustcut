//! Opening files: File > Open (the desktop's file dialog through the XDG portal), drag & drop onto the window (winit's file
//! events, with a drop hint over the video), the command line and a second start. Every way ends in [`FileOpener::open`]:
//! the path is checked on the runtime (never a file system call on the UI thread), then the player loads it, the Media
//! Info panel probes it, and the window title and the dialog's start folder follow.

mod drop;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

use rustcut_core::media::video_extensions;
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor as _};

use self::drop::{DropAction, DropTracker};
use crate::app::window::title;
use crate::ui::editing::JobEditor;
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::filters::FiltersBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::player::PlayerBridge;
use crate::ui::probe::ProbeBridge;
use crate::{MainWindow, Playback};

pub struct FileOpener {
    window: slint::Weak<MainWindow>,
    player: Weak<PlayerBridge>,
    media: Weak<MediaInfoBridge>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    probe: Weak<ProbeBridge>,
    export: Weak<ExportBridge>,
    /// The editing of a queued job (set after it is installed; it needs the opener first).
    editing: RefCell<Weak<JobEditor>>,
    runtime: tokio::runtime::Handle,
    last_open_dir: RefCell<Option<PathBuf>>,
    drops: RefCell<DropTracker>,
}

/// Where a file opened by RustCut itself starts (a job loaded into the editor, a draft put back).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartAt {
    pub position: Timestamp,
    pub paused: bool,
}

/// What runs after a file opened by RustCut itself was opened (`true`) or could not be (`false`).
pub type Opened = Box<dyn FnOnce(bool)>;

/// The bridges an opened file reaches.
pub struct OpenerParts<'a> {
    pub player: &'a Rc<PlayerBridge>,
    pub media: &'a Rc<MediaInfoBridge>,
    pub editor: &'a Rc<Editor>,
    pub filters: &'a Rc<FiltersBridge>,
    pub probe: &'a Rc<ProbeBridge>,
    pub export: &'a Rc<ExportBridge>,
}

impl FileOpener {
    /// Handles File > Open, `open-path` and the window's file drops from now on. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        parts: &OpenerParts<'_>,
        runtime: tokio::runtime::Handle,
        last_open_dir: Option<PathBuf>,
    ) -> Rc<Self> {
        let opener = Rc::new(Self {
            window: window.as_weak(),
            player: Rc::downgrade(parts.player),
            media: Rc::downgrade(parts.media),
            editor: Rc::downgrade(parts.editor),
            filters: Rc::downgrade(parts.filters),
            probe: Rc::downgrade(parts.probe),
            export: Rc::downgrade(parts.export),
            editing: RefCell::new(Weak::new()),
            runtime,
            last_open_dir: RefCell::new(last_open_dir),
            drops: RefCell::new(DropTracker::default()),
        });
        let this = Rc::downgrade(&opener);
        window.on_open_file(move || {
            if let Some(opener) = this.upgrade() {
                opener.open_dialog();
            }
        });
        let this = Rc::downgrade(&opener);
        window.on_open_path(move |path| {
            if let Some(opener) = this.upgrade() {
                opener.open(PathBuf::from(path.as_str()));
            }
        });
        let this = Rc::downgrade(&opener);
        window.window().on_winit_window_event(move |_, event| {
            if let Some(opener) = this.upgrade() {
                opener.winit_event(event);
            }
            EventResult::Propagate
        });
        opener
    }

    /// Connects the editing of queued jobs (a file the user opens ends it).
    pub fn set_editing(&self, editing: &Rc<JobEditor>) {
        *self.editing.borrow_mut() = Rc::downgrade(editing);
    }

    /// The folder of the last opened file (the dialog starts there; saved in the UI state).
    pub fn last_open_dir(&self) -> Option<PathBuf> {
        self.last_open_dir.borrow().clone()
    }

    /// Checks `path` on the runtime, then opens it (UI thread).
    pub fn open(self: &Rc<Self>, path: PathBuf) {
        self.open_with(path, None, None);
    }

    /// [`FileOpener::open`] for RustCut itself: the file starts at `start`, and `then` learns whether it opened (the
    /// editing of a queued job continues there).
    pub fn open_with(self: &Rc<Self>, path: PathBuf, start: Option<StartAt>, then: Option<Opened>) {
        let check = self.runtime.spawn_blocking(move || {
            let kind = std::fs::metadata(&path).map(|m| m.is_file());
            (path, kind)
        });
        let this = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Ok((path, kind)) = check.await else { return };
            let Some(opener) = this.upgrade() else { return };
            let opened = match kind {
                Ok(true) => opener.open_checked(&path, start, then.is_some()),
                Ok(false) => {
                    Self::report(Level::Warning, format!("Not a file: {}", path.display()), String::new());
                    false
                }
                Err(e) => {
                    Self::report(Level::Error, format!("Cannot open {}", file_name(&path)), e.to_string());
                    false
                }
            };
            if let Some(then) = then {
                then(opened);
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Open] The event loop is gone; not opened");
        }
    }

    /// Opens a checked file; `by_rustcut` = the editing of a job asked for it (else the user opened it).
    fn open_checked(&self, path: &Path, start: Option<StartAt>, by_rustcut: bool) -> bool {
        let Some(player) = self.player.upgrade() else {
            return false;
        };
        if !by_rustcut && let Some(editing) = self.editing.borrow().upgrade() {
            editing.user_opened(path);
        }
        // The probe render of the previous file stops, and the player leaves it.
        if let Some(probe) = self.probe.upgrade() {
            probe.file_opening();
        }
        // Before the player loads the file: the previous file's filters must not show on the new one.
        if let Some(filters) = self.filters.upgrade() {
            filters.file_opening(path);
        }
        let result = match start {
            Some(at) => player.return_to(path, at.position, at.paused),
            None => player.open(path),
        };
        if let Err(e) = result {
            Self::report(Level::Error, format!("Cannot open {}", file_name(path)), e.to_string());
            return false;
        }
        tracing::info!(path = %path.display(), "[Open] Opened");
        if let Some(media) = self.media.upgrade() {
            media.file_opened(path, by_rustcut);
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.file_opened();
        }
        if let Some(export) = self.export.upgrade() {
            export.file_opened(path);
        }
        if let Some(folder) = path.parent() {
            *self.last_open_dir.borrow_mut() = Some(folder.to_path_buf());
        }
        if let Some(window) = self.window.upgrade() {
            window.set_window_title(title(Some(path)).as_str().into());
        }
        // The file opens: an earlier file's problems are over.
        notices::clear_kind(Kind::OpenFailed);
        notices::clear_kind(Kind::PlaybackFailed);
        notices::post(Notice::new(Kind::Ready, "Ready"));
        true
    }

    /// File > Open: the portal's dialog runs on its own thread; the UI thread only awaits the answer.
    fn open_dialog(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else { return };
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title("Open Video")
            // The video formats ffmpeg reads (`media::VIDEO_FORMATS`, the one list RustCut has).
            .add_filter("Videos", &video_extensions().collect::<Vec<_>>())
            .add_filter("All files", &["*"]);
        let start = self.last_open_dir().or_else(|| {
            self.player
                .upgrade()
                .and_then(|p| p.current_file()?.parent().map(Path::to_path_buf))
        });
        if let Some(folder) = start {
            dialog = dialog.set_directory(folder);
        }
        let handle = window.window().window_handle();
        dialog = dialog.set_parent(&handle);
        let pick = dialog.pick_file();
        let this = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let picked = pick.await;
            let Some(opener) = this.upgrade() else { return };
            if let Some(file) = picked {
                opener.open(file.path().to_path_buf());
            } else {
                tracing::debug!("[Open] File dialog canceled");
            }
        });
        if let Err(e) = spawned {
            Self::report(
                Level::Error,
                "The file dialog could not be opened".to_owned(),
                e.to_string(),
            );
        }
    }

    fn winit_event(self: &Rc<Self>, event: &WindowEvent) {
        let action = match event {
            WindowEvent::HoveredFile(_) => self.drops.borrow_mut().hovered(),
            WindowEvent::HoveredFileCancelled => self.drops.borrow_mut().cancelled(),
            WindowEvent::DroppedFile(path) => self.drops.borrow_mut().dropped(path, Instant::now()),
            _ => return,
        };
        if let Some(window) = self.window.upgrade() {
            let playback = window.global::<Playback<'_>>();
            match &action {
                DropAction::Hint { files } => {
                    playback.set_drop_hover(true);
                    playback.set_drop_count(i32::try_from(*files).unwrap_or(i32::MAX));
                }
                DropAction::Open(_) | DropAction::Ignore | DropAction::Clear => playback.set_drop_hover(false),
            }
        }
        if let DropAction::Open(path) = action {
            tracing::info!(path = %path.display(), "[Open] File dropped");
            self.open(path);
        }
    }

    /// A file that does not open: a banner with the way to the log, where the details are (Q-N8 #12).
    fn report(level: Level, title: String, detail: String) {
        match level {
            Level::Error => tracing::error!(detail, "[Open] {title}"),
            Level::Warning => tracing::warn!(detail, "[Open] {title}"),
            Level::Info | Level::Success => tracing::info!(detail, "[Open] {title}"),
        }
        notices::post(
            Notice::new(Kind::OpenFailed, title)
                .detail(detail)
                .level(level)
                .action("Show log", notices::show_app_log),
        );
    }
}

/// "clip.mp4" of a path (the whole path when it has no name).
fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Opens the first of `files` forwarded by a second start and brings the window to the front (UI thread).
pub fn open_forwarded(window: &MainWindow, files: &[PathBuf]) {
    if let Some(first) = files.first() {
        window.invoke_open_path(first.to_string_lossy().as_ref().into());
        if files.len() > 1 {
            tracing::info!(count = files.len(), "[Open] Only the first forwarded file is opened");
        }
    }
    window.window().set_minimized(false);
    window
        .window()
        .with_winit_window(slint::winit_030::winit::window::Window::focus_window);
}
