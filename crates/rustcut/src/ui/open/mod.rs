//! Opening files: File > Open (the desktop's file dialog through the XDG portal), drag & drop onto the window (winit's file
//! events, with a drop hint over the video), the command line and a second start. Every way ends in [`FileOpener::open`]:
//! the path is checked on the runtime (never a file system call on the UI thread), then the player loads it, the Media
//! Info panel probes it, and the window title and the dialog's start folder follow.

mod drop;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

use slint::ComponentHandle as _;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor as _};

use self::drop::{DropAction, DropTracker};
use crate::app::window::title;
use crate::ui::editor::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::player::PlayerBridge;
use crate::ui::status::{self, Level};
use crate::{MainWindow, Playback};

/// File name extensions offered by the dialog's "Videos" filter (the "All files" filter shows everything).
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "mpg", "mpeg", "ts", "m2ts", "mts", "wmv", "flv", "ogv", "3gp", "vob",
];

pub struct FileOpener {
    window: slint::Weak<MainWindow>,
    player: Weak<PlayerBridge>,
    media: Weak<MediaInfoBridge>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    runtime: tokio::runtime::Handle,
    last_open_dir: RefCell<Option<PathBuf>>,
    drops: RefCell<DropTracker>,
}

impl FileOpener {
    /// Handles File > Open, `open-path` and the window's file drops from now on. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        player: &Rc<PlayerBridge>,
        media: &Rc<MediaInfoBridge>,
        editor: &Rc<Editor>,
        filters: &Rc<FiltersBridge>,
        runtime: tokio::runtime::Handle,
        last_open_dir: Option<PathBuf>,
    ) -> Rc<Self> {
        let opener = Rc::new(Self {
            window: window.as_weak(),
            player: Rc::downgrade(player),
            media: Rc::downgrade(media),
            editor: Rc::downgrade(editor),
            filters: Rc::downgrade(filters),
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

    /// The folder of the last opened file (the dialog starts there; saved in the UI state).
    pub fn last_open_dir(&self) -> Option<PathBuf> {
        self.last_open_dir.borrow().clone()
    }

    /// Checks `path` on the runtime, then opens it (UI thread).
    pub fn open(self: &Rc<Self>, path: PathBuf) {
        let check = self.runtime.spawn_blocking(move || {
            let kind = std::fs::metadata(&path).map(|m| m.is_file());
            (path, kind)
        });
        let this = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Ok((path, kind)) = check.await else { return };
            let Some(opener) = this.upgrade() else { return };
            match kind {
                Ok(true) => opener.open_checked(&path),
                Ok(false) => opener.report(Level::Warning, &format!("Not a file: {}", path.display())),
                Err(e) => opener.report(Level::Error, &format!("Cannot open {}: {e}", path.display())),
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Open] The event loop is gone; not opened");
        }
    }

    fn open_checked(&self, path: &Path) {
        let Some(player) = self.player.upgrade() else { return };
        // Before the player loads the file: the previous file's filters must not show on the new one.
        if let Some(filters) = self.filters.upgrade() {
            filters.file_opening(path);
        }
        if let Err(e) = player.open(path) {
            self.report(Level::Error, &format!("Cannot open {}: {e}", path.display()));
            return;
        }
        tracing::info!(path = %path.display(), "[Open] Opened");
        if let Some(media) = self.media.upgrade() {
            media.file_opened(path);
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.file_opened();
        }
        if let Some(folder) = path.parent() {
            *self.last_open_dir.borrow_mut() = Some(folder.to_path_buf());
        }
        if let Some(window) = self.window.upgrade() {
            window.set_window_title(title(Some(path)).as_str().into());
            status::show(&window, Level::Info, "Ready");
        }
    }

    /// File > Open: the portal's dialog runs on its own thread; the UI thread only awaits the answer.
    fn open_dialog(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else { return };
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title("Open Video")
            .add_filter("Videos", VIDEO_EXTENSIONS)
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
            self.report(Level::Error, &format!("The file dialog could not be opened: {e}"));
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

    fn report(&self, level: Level, message: &str) {
        match level {
            Level::Error => tracing::error!("[Open] {message}"),
            Level::Warning => tracing::warn!("[Open] {message}"),
            Level::Info | Level::Success => tracing::info!("[Open] {message}"),
        }
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }
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
