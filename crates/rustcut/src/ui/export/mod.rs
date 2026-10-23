//! The export flow (M2.6): the Job Finalization card (target folder, name, counter, the live conflict check) and Save,
//! which builds the job from the editor's sections, the filter chain, the Render Settings panel's working copy and the
//! probed source, checks the target once more and either queues the job or opens the conflict dialog (`save.rs`).
//!
//! Nothing here touches the drive on the UI thread: the folder is read on the runtime (`scan_folder`), the results come
//! back through `slint::spawn_local` and count only for the folder they were read for.

mod check;
mod dialog;
mod save;
mod state;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use rustcut_core::naming::{FolderScan, scan_folder};
use rustcut_core::settings::{AutoSection, ExportPanelState, Settings, WarningSettings};
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

pub use check::{CardCheck, CardInput, Level as CheckLevel, card_check};
pub use dialog::{Choice, ConflictDialog, DialogInput, DialogOption, Outcome, silent_outcome};
pub use state::{ExportState, JobFacts, MAX_COUNTER, SOURCE_FOLDER, codec_label};

use self::save::Pending;
use crate::app::SettingsWriter;
use crate::ui::editor::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::player::PlayerBridge;
use crate::ui::queue::QueueBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::status::{self, Level};
use crate::{Export, MainWindow};

/// The parts the export flow reads the job from.
pub struct ExportParts<'a> {
    pub player: &'a Rc<PlayerBridge>,
    pub editor: &'a Rc<Editor>,
    pub filters: &'a Rc<FiltersBridge>,
    pub media: &'a Rc<MediaInfoBridge>,
    pub render: &'a Rc<RenderSettingsBridge>,
    pub queue: &'a Rc<QueueBridge>,
}

pub struct ExportBridge {
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    writer: SettingsWriter,
    player: Weak<PlayerBridge>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    media: Weak<MediaInfoBridge>,
    render: Weak<RenderSettingsBridge>,
    queue: Weak<QueueBridge>,
    state: RefCell<ExportState>,
    warnings: RefCell<WarningSettings>,
    auto_section: AutoSection,
    /// The last scan and the folder it is of.
    scan: RefCell<Option<(PathBuf, FolderScan)>>,
    /// Counts scans; a result counts only when it is the newest.
    scans: Cell<u64>,
    /// A save waits for the folder check or the queue (no second save meanwhile).
    saving: Cell<bool>,
    /// The open conflict dialog with the job it is about.
    pending: RefCell<Option<Pending>>,
}

impl ExportBridge {
    /// Takes over the Job Finalization card and the conflict dialog. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        parts: &ExportParts<'_>,
        runtime: tokio::runtime::Handle,
        settings: &Settings,
        panel: &ExportPanelState,
        writer: SettingsWriter,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            runtime,
            writer,
            player: Rc::downgrade(parts.player),
            editor: Rc::downgrade(parts.editor),
            filters: Rc::downgrade(parts.filters),
            media: Rc::downgrade(parts.media),
            render: Rc::downgrade(parts.render),
            queue: Rc::downgrade(parts.queue),
            state: RefCell::new(ExportState::new(&settings.export, panel)),
            warnings: RefCell::new(settings.warnings.clone()),
            auto_section: settings.editor.auto_section,
            scan: RefCell::new(None),
            scans: Cell::new(0),
            saving: Cell::new(false),
            pending: RefCell::new(None),
        });
        connect(&bridge, window);
        save::connect_dialog(&bridge, window);
        let weak = Rc::downgrade(&bridge);
        parts.queue.set_on_job_ended(move || {
            if let Some(b) = weak.upgrade() {
                b.rescan();
            }
        });
        bridge.publish();
        bridge
    }

    /// Another video was opened: the name and the counter start over, the source folder may be another one.
    pub fn file_opened(&self, path: &Path) {
        self.state.borrow_mut().file_opened(path);
        self.rescan();
    }

    /// What `ui-state.toml` keeps of the card.
    pub fn save_into(&self, panel: &mut ExportPanelState) {
        self.state.borrow().save_into(panel);
    }

    /// The template's facts of the job as it is set up now (for the card's file name).
    fn facts(&self) -> Option<JobFacts> {
        let profile = self.render.upgrade()?.working_profile();
        let (sections, duration) = self
            .editor
            .upgrade()
            .and_then(|e| e.sections_for_save(self.auto_section).ok())
            .map_or((0, Timestamp::ZERO), |list| (list.len(), list.total_duration()));
        Some(JobFacts::new(&profile, sections, duration))
    }

    /// Reads the target folder again on the runtime (after a change of folder or source, a save, a finished job).
    fn rescan(&self) {
        let generation = self.scans.get().wrapping_add(1);
        self.scans.set(generation);
        let Some(folder) = self.state.borrow().target_folder() else {
            *self.scan.borrow_mut() = None;
            self.publish();
            return;
        };
        if self
            .scan
            .borrow()
            .as_ref()
            .is_some_and(|(f, _)| f.as_path() != folder.as_path())
        {
            *self.scan.borrow_mut() = None;
        }
        self.publish();
        let task = self.runtime.spawn({
            let folder = folder.clone();
            async move { scan_folder(&folder).await }
        });
        let this = self.window.clone();
        let bridge = BRIDGE.with(|b| b.borrow().clone());
        let spawned = slint::spawn_local(async move {
            let Ok(found) = task.await else { return };
            let Some(bridge) = bridge.upgrade() else { return };
            if bridge.scans.get() != generation || this.upgrade().is_none() {
                return;
            }
            *bridge.scan.borrow_mut() = Some((folder, found));
            bridge.publish();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; the folder is not checked");
        }
    }

    /// The scan of `folder`, when the last one is of it.
    fn scan_of(&self, folder: &Path) -> Option<FolderScan> {
        self.scan
            .borrow()
            .as_ref()
            .filter(|(f, _)| f.as_path() == folder)
            .map(|(_, s)| s.clone())
    }

    /// Shows the card.
    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let facts = self.facts();
        let st = self.state.borrow();
        let folder = st.target_folder();
        let file = facts.as_ref().map(|f| st.file(f));
        let scan = folder.as_deref().and_then(|f| self.scan_of(f));
        let queue = self.queue.upgrade().map(|q| q.targets()).unwrap_or_default();
        let warnings = self.warnings.borrow();
        let fallback = rustcut_core::naming::NameError::Empty;
        let line = card_check(&CardInput {
            source: st.source.as_deref(),
            folder: folder.as_deref(),
            file: match &file {
                Some(Ok((stem, name))) => Ok((stem.as_str(), name.as_str())),
                Some(Err(e)) => Err(e),
                None => Err(&fallback),
            },
            base: &st.name,
            scan: scan.as_ref(),
            queue: &queue,
            warnings: &warnings,
        });
        let e = window.global::<Export<'_>>();
        e.set_has_source(st.source.is_some());
        e.set_folders(slint::ModelRc::new(slint::VecModel::from(
            st.folder_items()
                .into_iter()
                .map(Into::into)
                .collect::<Vec<slint::SharedString>>(),
        )));
        e.set_folder_index(i32::try_from(st.folder_index()).unwrap_or_default());
        e.set_folder_path(
            folder
                .as_deref()
                .map_or_else(
                    || "The folder of the source video".to_owned(),
                    |f| f.display().to_string(),
                )
                .into(),
        );
        e.set_can_remove_folder(st.folder.is_some());
        e.set_name(st.name.as_str().into());
        e.set_counter_enabled(st.counter_enabled);
        e.set_counter(format!("{:02}", st.counter).into());
        e.set_auto_increment(st.auto_increment);
        e.set_clear_sections(st.clear_sections);
        e.set_file_name(
            file.and_then(Result::ok)
                .map(|(_, name)| name)
                .unwrap_or_default()
                .into(),
        );
        e.set_check_level(line.level as i32);
        e.set_check_text(line.text.as_str().into());
        e.set_check_tooltip(line.tooltip.as_str().into());
        let saving = self.saving.get();
        e.set_can_save(line.can_save() && !saving);
        e.set_save_tooltip(
            if saving {
                "Saving...".to_owned()
            } else if line.can_save() {
                "Save the job into the queue".to_owned()
            } else {
                format!("Cannot save: {}", line.text)
            }
            .into(),
        );
    }

    fn status(&self, level: Level, message: &str) {
        if level == Level::Error {
            tracing::warn!(message, "[Export] Not saved");
        } else {
            tracing::info!(message, "[Export]");
        }
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }

    /// Persists the target folders.
    fn store_folders(&self) {
        let folders = self.state.borrow().folders.clone();
        if !self.writer.update(|s| s.export.target_folders = folders) {
            self.status(
                Level::Warning,
                "config.toml is from a newer RustCut: the folder list is not saved.",
            );
        }
    }

    /// File > Add folder: the portal's folder dialog runs on its own thread; the UI thread only awaits the answer.
    fn add_folder(self: &Rc<Self>) {
        let start = self.state.borrow().target_folder();
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Add a Target Folder");
        if let Some(start) = start {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = self.window.upgrade() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = dialog.pick_folder();
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Some(folder) = picked.await else { return };
            let Some(bridge) = weak.upgrade() else { return };
            let path = folder.path().to_path_buf();
            if bridge.state.borrow_mut().add_folder(&path) {
                tracing::info!(folder = %path.display(), "[Export] Target folder added");
                bridge.store_folders();
            }
            bridge.rescan();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; no folder dialog");
        }
    }
}

thread_local! {
    /// The bridge, for the continuations of runtime work on the UI thread.
    static BRIDGE: RefCell<Weak<ExportBridge>> = const { RefCell::new(Weak::new()) };
}

fn connect(bridge: &Rc<ExportBridge>, window: &MainWindow) {
    BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(bridge));
    let e = window.global::<Export<'_>>();
    let on = |f: fn(&Rc<ExportBridge>)| {
        let weak = Rc::downgrade(bridge);
        move || {
            if let Some(b) = weak.upgrade() {
                f(&b);
            }
        }
    };
    e.on_select_folder({
        let weak = Rc::downgrade(bridge);
        move |index| {
            if let Some(b) = weak.upgrade() {
                b.state
                    .borrow_mut()
                    .select_folder(usize::try_from(index).unwrap_or_default());
                b.rescan();
            }
        }
    });
    e.on_add_folder(on(ExportBridge::add_folder));
    e.on_remove_folder(on(|b| {
        if b.state.borrow_mut().remove_folder() {
            b.store_folders();
        }
        b.rescan();
    }));
    e.on_name_edited({
        let weak = Rc::downgrade(bridge);
        move |text| {
            if let Some(b) = weak.upgrade() {
                b.state.borrow_mut().name = text.to_string();
                b.publish();
            }
        }
    });
    e.on_toggle_counter(on(|b| {
        let mut st = b.state.borrow_mut();
        st.counter_enabled = !st.counter_enabled;
        drop(st);
        b.publish();
    }));
    e.on_counter_committed({
        let weak = Rc::downgrade(bridge);
        move |text| {
            let Some(b) = weak.upgrade() else { return false };
            let Ok(value) = text.trim().parse::<u32>() else {
                return false;
            };
            b.state.borrow_mut().set_counter(value);
            b.publish();
            true
        }
    });
    e.on_counter_stepped({
        let weak = Rc::downgrade(bridge);
        move |steps| {
            if let Some(b) = weak.upgrade() {
                b.state.borrow_mut().step_counter(steps);
                b.publish();
            }
        }
    });
    e.on_toggle_auto_increment(on(|b| {
        let mut st = b.state.borrow_mut();
        st.auto_increment = !st.auto_increment;
        drop(st);
        b.publish();
    }));
    e.on_toggle_clear_sections(on(|b| {
        let mut st = b.state.borrow_mut();
        st.clear_sections = !st.clear_sections;
        drop(st);
        b.publish();
    }));
    e.on_save(on(ExportBridge::save));
}
