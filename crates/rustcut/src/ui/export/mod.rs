//! The export flow (M2.6): the Job Finalization card (target folder, name, counter, the live conflict check) and Save,
//! which builds the job from the editor's sections, the filter chain, the Render Settings panel's working copy and the
//! probed source, checks the target once more and either queues the job or opens the conflict dialog (`save.rs`).
//!
//! Nothing here touches the drive on the UI thread: the folder is read on the runtime (`scan_folder`), the results come
//! back through `slint::spawn_local` and count only for the folder they were read for.

mod browse;
mod bubbles;
mod check;
mod dialog;
#[cfg(test)]
mod dialog_tests;
mod dialog_view;
mod editing;
mod job_check;
mod known;
mod metadata;
mod previews;
mod save;
mod state;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use rustcut_core::media::{PreviewOptions, PreviewPrograms, PreviewService};
use rustcut_core::naming::{FolderScan, scan_folder};
use rustcut_core::settings::{AutoSection, ConflictDialogSettings, ExportPanelState, Settings, WarningSettings};
use rustcut_core::time::Timestamp;
use slint::ComponentHandle as _;

pub use check::{At, CardCheck, CardInput, Level as CheckLevel, card_check, earlier_in_series};
pub use dialog::{
    Choice, ConflictDialog, DialogInput, DialogOption, KnownFile, KnownFiles, Outcome, Tab, silent_outcome,
};
pub use known::KnownList;
pub use metadata::{MetadataFacts, Missing, missing};
pub use state::{ExportDraft, ExportState, JobFacts, MAX_COUNTER, SOURCE_FOLDER, codec_label};

use self::job_check::JobProblem;
use self::metadata::MetaPending;
use self::previews::DialogPreviews;
use self::save::Pending;
use crate::app::SettingsWriter;
use crate::ui::editing::JobEditor;
use crate::ui::editor::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::jump::JumpTarget;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::player::PlayerBridge;
use crate::ui::queue::QueueBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::settings::page::Page;
use crate::ui::tags::TagsBridge;
use crate::{Export, MainWindow};

/// The folder list of Settings > Export & Naming (its jump mark, `settings_folders.slint`).
pub const FOLDERS_KEY: &str = "export.folders";

/// The parts the export flow reads the job from.
pub struct ExportParts<'a> {
    pub player: &'a Rc<PlayerBridge>,
    pub editor: &'a Rc<Editor>,
    pub filters: &'a Rc<FiltersBridge>,
    pub media: &'a Rc<MediaInfoBridge>,
    pub render: &'a Rc<RenderSettingsBridge>,
    pub queue: &'a Rc<QueueBridge>,
    pub tags: &'a Rc<TagsBridge>,
}

pub struct ExportBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    writer: SettingsWriter,
    player: Weak<PlayerBridge>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    media: Weak<MediaInfoBridge>,
    render: Weak<RenderSettingsBridge>,
    queue: Weak<QueueBridge>,
    tags: Weak<TagsBridge>,
    state: RefCell<ExportState>,
    warnings: RefCell<WarningSettings>,
    auto_section: Cell<AutoSection>,
    /// The last scan and the folder it is of.
    scan: RefCell<Option<(PathBuf, FolderScan)>>,
    /// Counts scans; a result counts only when it is the newest.
    scans: Cell<u64>,
    /// A save waits for the folder check or the queue (no second save meanwhile).
    saving: Cell<bool>,
    /// The open conflict dialog with the job it is about.
    pending: RefCell<Option<Pending>>,
    /// The open question about missing tags or rating, with the job it is about.
    meta: RefCell<Option<MetaPending>>,
    /// The editing of a queued job (set after it is installed; it needs the export first).
    editing: RefCell<Weak<JobEditor>>,
    /// How the conflict dialog behaves (`[dialogs.conflict]`).
    dialog_settings: RefCell<ConflictDialogSettings>,
    /// Pictures and descriptions of the dialog's files (shared cache for the run).
    preview_service: PreviewService,
    previews: RefCell<DialogPreviews>,
    /// A refresh of the card is scheduled (`job_changed`).
    refresh: Cell<bool>,
    /// The job problem the banner shows (M6.5).
    shown_problem: RefCell<Option<JobProblem>>,
    /// The check's bubble as it was posted last (M6.5c).
    shown_check: RefCell<Option<bubbles::CheckBubble>>,
    /// The known-source dialog paused the video: it plays on when the dialog closes (M6.5d).
    resume_after_dialog: Cell<bool>,
}

/// The key of the banner about the job problem.
const SAVE_BLOCKED: &str = "save_blocked";

/// What the previews run with, from the settings.
fn preview_options(settings: &Settings) -> PreviewOptions {
    let conflict = &settings.dialogs.conflict;
    PreviewOptions {
        programs: PreviewPrograms {
            ffmpeg: settings.advanced.ffmpeg.clone(),
            ffprobe: settings.advanced.ffprobe.clone(),
        },
        picture_at_percent: conflict.picture_at_percent,
        parallel: usize::from(conflict.parallel_pictures),
    }
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
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime,
            writer,
            player: Rc::downgrade(parts.player),
            editor: Rc::downgrade(parts.editor),
            filters: Rc::downgrade(parts.filters),
            media: Rc::downgrade(parts.media),
            render: Rc::downgrade(parts.render),
            queue: Rc::downgrade(parts.queue),
            tags: Rc::downgrade(parts.tags),
            state: RefCell::new(ExportState::new(&settings.export, panel)),
            warnings: RefCell::new(settings.warnings.clone()),
            auto_section: Cell::new(settings.editor.auto_section),
            scan: RefCell::new(None),
            scans: Cell::new(0),
            saving: Cell::new(false),
            pending: RefCell::new(None),
            meta: RefCell::new(None),
            editing: RefCell::new(Weak::new()),
            dialog_settings: RefCell::new(settings.dialogs.conflict.clone()),
            preview_service: PreviewService::new(preview_options(settings)),
            previews: RefCell::new(DialogPreviews::default()),
            refresh: Cell::new(false),
            shown_problem: RefCell::new(None),
            shown_check: RefCell::new(None),
            resume_after_dialog: Cell::new(false),
        });
        connect(&bridge, window);
        save::connect_dialog(&bridge, window);
        metadata::connect(&bridge, window);
        let weak = Rc::downgrade(&bridge);
        parts.queue.hooks().job_ended = Some(Box::new(move || {
            if let Some(b) = weak.upgrade() {
                b.rescan();
            }
        }));
        bridge.publish();
        bridge
    }

    /// The settings changed (the Settings window, or this card's own folders and "Don't ask again"): the folders, the
    /// template, the warnings and the automatic section follow; the card keeps the selected folder when it is still
    /// listed.
    pub fn settings_changed(&self, settings: &Settings) {
        let changed = self.state.borrow_mut().set_settings(&settings.export);
        self.warnings.replace(settings.warnings.clone());
        self.dialog_settings.replace(settings.dialogs.conflict.clone());
        let options = preview_options(settings);
        if self.preview_service.options() != options {
            self.preview_service.set_options(options);
        }
        self.auto_section.set(settings.editor.auto_section);
        if changed {
            self.rescan();
        }
        self.publish();
    }

    /// Another video was opened: the name and the counter start over, the source folder may be another one.
    pub fn file_opened(&self, path: &Path) {
        self.state.borrow_mut().file_opened(path);
        self.rescan();
    }

    /// The sections, the render settings or the source's description changed: the card checks the job again once the
    /// current event is handled (the caller may still hold its own state).
    pub fn job_changed(&self) {
        // A "not saved yet" banner of an earlier Save is out of date now.
        notices::clear_kind(Kind::SaveRefused);
        if self.refresh.replace(true) {
            return;
        }
        let bridge = BRIDGE.with(|b| b.borrow().clone());
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            if let Some(bridge) = bridge.upgrade() {
                bridge.refresh.set(false);
                bridge.publish();
            }
        });
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
            .and_then(|e| e.sections_for_save(self.auto_section.get()).ok())
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
        let job = self.job_problem(facts.as_ref().map_or(0, |f| f.sections));
        // Auto-iterate saves each job with the next number: the earlier ones of the series are the jobs just saved.
        let quiet = match (&facts, &scan, st.active_counter()) {
            (Some(facts), Some(scan), Some(current)) if st.auto_increment && st.template_uses_counter() => {
                earlier_in_series(&scan.names, current, &st.name, |n| st.stem_with(facts, Some(n)).ok())
            }
            _ => Vec::new(),
        };
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
            quiet: &quiet,
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
        e.set_name(st.name.as_str().into());
        e.set_counter_enabled(st.counter_enabled);
        e.set_counter(format!("{:02}", st.counter).into());
        e.set_auto_increment(st.auto_increment);
        e.set_clear_sections(st.clear_sections);
        let file_name = file
            .as_ref()
            .and_then(|f| f.as_ref().ok())
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        e.set_extension(
            Path::new(&file_name)
                .extension()
                .map(|x| format!(".{}", x.to_string_lossy()))
                .unwrap_or_default()
                .into(),
        );
        let fix = match (&line.level, folder.as_deref(), scan.as_ref(), facts.as_ref()) {
            (check::Level::Conflict, Some(folder), Some(scan), Some(facts)) => {
                bubbles::conflict_fix(&st, facts, folder, scan, &queue)
            }
            _ => None,
        };
        self.show_check(&line, fix);
        // What keeps the job itself from being saved: the banner names it, Save's tooltip too.
        self.show_job_problem(job.as_ref());
        let saving = self.saving.get();
        e.set_can_save(line.can_save() && job.is_none() && !saving);
        e.set_save_tooltip(
            if saving {
                "Saving...".to_owned()
            } else if let Some(job) = &job {
                format!("Cannot save: {}", job.text)
            } else if !line.can_save() {
                format!("Cannot save: {}", line.text)
            } else if self.editing.borrow().upgrade().is_some_and(|ed| ed.info().is_some()) {
                format!("Save as a new job: {file_name} (the loaded job stays as it is)")
            } else {
                format!("Save the job into the queue: {file_name}")
            }
            .into(),
        );
    }

    /// What keeps the job itself from being saved, with the option that solves it.
    fn job_problem(&self, sections: usize) -> Option<JobProblem> {
        let profile = self.render.upgrade()?.working_profile();
        let source = self.media.upgrade().and_then(|m| m.current());
        job_check::job_problem(&profile, sections, source.as_deref())
    }

    fn tell(notice: Notice) {
        if notice.level == Level::Error {
            tracing::warn!(message = %notice.text(), "[Export] Not saved");
        } else {
            tracing::info!(message = %notice.text(), "[Export]");
        }
        notices::post(notice);
    }

    /// The job itself cannot be saved (Q-N8 #5): a banner over the video names it and leads to the option, while a
    /// video is open; it goes when the problem is solved.
    fn show_job_problem(&self, problem: Option<&JobProblem>) {
        let problem = problem.filter(|_| self.state.borrow().source.is_some());
        if self.shown_problem.borrow().as_ref() == problem {
            return;
        }
        self.shown_problem.replace(problem.cloned());
        match problem {
            Some(problem) => {
                tracing::info!(problem = %problem.text, "[Export] The job cannot be saved");
                let jump = problem.jump;
                notices::post(
                    Notice::new(Kind::SaveBlocked, problem.text.as_str())
                        .detail(problem.tooltip.as_str())
                        .key(SAVE_BLOCKED)
                        .jump(jump)
                        .jump_action("Show the option", jump),
                );
            }
            None => notices::clear(SAVE_BLOCKED),
        }
    }

    /// Persists the target folders.
    fn store_folders(&self) {
        let folders = self.state.borrow().folders.clone();
        if !self.writer.update(|s| s.export.target_folders = folders) {
            Self::tell(
                Notice::new(Kind::SettingsReadOnly, "The folder list is not saved")
                    .detail("config.toml is from a newer RustCut."),
            );
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
    e.on_edit_folders(|| {
        crate::ui::jump::go(JumpTarget::Settings {
            page: Page::Export,
            key: FOLDERS_KEY,
        });
    });
    e.on_name_edited({
        let weak = Rc::downgrade(bridge);
        move |text| {
            if let Some(b) = weak.upgrade() {
                b.state.borrow_mut().name = text.to_string();
                b.publish();
            }
        }
    });
    // An empty counter field means no counter (P9.7, Q-P9): the counter's own switch is gone.
    e.on_counter_committed({
        let weak = Rc::downgrade(bridge);
        move |text| {
            let Some(b) = weak.upgrade() else { return false };
            if !b.state.borrow_mut().type_counter(&text) {
                return false;
            }
            b.publish();
            true
        }
    });
    e.on_counter_stepped({
        let weak = Rc::downgrade(bridge);
        move |steps| {
            if let Some(b) = weak.upgrade() {
                let mut st = b.state.borrow_mut();
                if st.counter_enabled {
                    st.step_counter(steps);
                } else {
                    st.counter_enabled = true;
                }
                drop(st);
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
