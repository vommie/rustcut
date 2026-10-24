//! A queued job loaded into the editor (M4.1, user decisions Q-Q2, Q-Q3, Q-N10): the job's spec is read on the runtime,
//! a waiting job is held meanwhile, the current session is replaced or asked about first (`editor.on_job_load`), the
//! job's source opens, and its sections, filters, render settings, target and tags fill the panels. The export's Save
//! then updates the job (same target file) or makes a new one (`session.rs`); the editing ends there, with "Stop
//! editing" or when the user opens another file. The queue highlights the job while it is edited.

mod session;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};

use rustcut_core::job::{JobId, JobSpec, JobStateKind};
use rustcut_core::queue::{QueueCommand, QueueError, QueueHandle};
use rustcut_core::settings::OnJobLoad;
use slint::ComponentHandle as _;

pub use session::{JobEdit, SaveAs};

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::filters::FiltersBridge;
use crate::ui::inspector::InspectorBridge;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::open::{FileOpener, StartAt};
use crate::ui::queue::QueueBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::tags::{Choice, TagsBridge};
use crate::{JobLoad, MainWindow};

thread_local! {
    static BRIDGE: RefCell<Weak<JobEditor>> = const { RefCell::new(Weak::new()) };
}

/// The parts a loaded job fills.
pub struct EditingParts<'a> {
    pub editor: &'a Rc<Editor>,
    pub filters: &'a Rc<FiltersBridge>,
    pub render: &'a Rc<RenderSettingsBridge>,
    pub export: &'a Rc<ExportBridge>,
    pub tags: &'a Rc<TagsBridge>,
    pub queue: &'a Rc<QueueBridge>,
    pub inspector: &'a Rc<InspectorBridge>,
    pub opener: &'a Rc<FileOpener>,
}

/// What the export needs of the editing when it saves.
#[derive(Debug, Clone)]
pub struct EditInfo {
    pub id: JobId,
    pub target: std::path::PathBuf,
    pub overwrite: bool,
    pub updatable: bool,
    /// The job as it was loaded (facts the panels may not know yet: the probe's summary, the source hash).
    pub spec: Box<JobSpec>,
}

/// A job whose spec was read, waiting for the answer to the "ask" question.
struct Asking {
    id: JobId,
    state: JobStateKind,
    spec: Box<JobSpec>,
}

pub struct JobEditor {
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    handle: QueueHandle,
    mode: Cell<OnJobLoad>,
    editor: Weak<Editor>,
    filters: Weak<FiltersBridge>,
    render: Weak<RenderSettingsBridge>,
    export: Weak<ExportBridge>,
    tags: Weak<TagsBridge>,
    queue: Weak<QueueBridge>,
    inspector: Weak<InspectorBridge>,
    opener: Weak<FileOpener>,
    session: RefCell<Option<JobEdit>>,
    /// A job's source is opening.
    loading: Cell<Option<JobId>>,
    asking: RefCell<Option<Asking>>,
}

impl JobEditor {
    /// The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        parts: &EditingParts<'_>,
        mode: OnJobLoad,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            handle: parts.queue.handle().clone(),
            mode: Cell::new(mode),
            editor: Rc::downgrade(parts.editor),
            filters: Rc::downgrade(parts.filters),
            render: Rc::downgrade(parts.render),
            export: Rc::downgrade(parts.export),
            tags: Rc::downgrade(parts.tags),
            queue: Rc::downgrade(parts.queue),
            inspector: Rc::downgrade(parts.inspector),
            opener: Rc::downgrade(parts.opener),
            session: RefCell::new(None),
            loading: Cell::new(None),
            asking: RefCell::new(None),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        connect(&bridge, window);
        bridge
    }

    /// The edited job, for the export's Save.
    pub fn info(&self) -> Option<EditInfo> {
        self.session.borrow().as_ref().map(|s| EditInfo {
            id: s.id,
            target: s.target.clone(),
            overwrite: s.overwrite,
            updatable: s.updatable,
            spec: s.spec.clone(),
        })
    }

    /// What a save writing `target` does now.
    pub fn save_as(&self, target: &Path, force_new: bool) -> Option<SaveAs> {
        self.session.borrow().as_ref().map(|s| s.save_as(target, force_new))
    }

    /// The card's banner text, when a job is edited.
    pub fn banner(&self) -> Option<String> {
        self.session.borrow().as_ref().map(JobEdit::banner)
    }

    /// "Edit in the editor" or "Load as a new job" of job `id`: reads its spec on the runtime first.
    /// What happens to the current session at the next load (`editor.on_job_load`, changed in the Settings window).
    pub fn set_on_job_load(&self, mode: OnJobLoad) {
        self.mode.set(mode);
    }

    pub fn load(&self, id: JobId) {
        if self.loading.get().is_some() {
            Self::tell(
                Level::Info,
                "A job is being loaded into the editor; try again in a moment.",
            );
            return;
        }
        if self.session.borrow().as_ref().is_some_and(|s| s.id == id) {
            Self::tell(Level::Info, &format!("Job {id} is in the editor already."));
            return;
        }
        let handle = self.handle.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = handle
                .request(|reply| QueueCommand::Spec { id, reply })
                .await
                .unwrap_or_else(|_gone| Err(QueueError::Store("the queue has stopped".into())));
            ui.post(move || {
                BRIDGE.with(|b| {
                    if let Some(bridge) = b.borrow().upgrade() {
                        bridge.spec_read(id, answer);
                    }
                });
            });
        });
    }

    fn spec_read(&self, id: JobId, answer: Result<Box<JobSpec>, QueueError>) {
        let spec = match answer {
            Ok(spec) => spec,
            Err(e) => {
                Self::tell(Level::Error, &format!("The job cannot be loaded into the editor: {e}"));
                return;
            }
        };
        let Some(state) = self.queue.upgrade().and_then(|q| q.row(id)).map(|r| r.state.kind()) else {
            return;
        };
        if state.is_active() {
            Self::tell(
                Level::Warning,
                &format!("Job {id} is rendering; it can be loaded when it ends."),
            );
            return;
        }
        let editing = self.session.borrow().is_some();
        let has_work = self.editor.upgrade().is_some_and(|e| e.has_work());
        match self.mode.get() {
            OnJobLoad::Ask if has_work && !editing => self.ask(Asking { id, state, spec }),
            OnJobLoad::Replace | OnJobLoad::Ask => self.begin(id, state, spec),
        }
    }

    fn ask(&self, asking: Asking) {
        let Some(window) = self.window.upgrade() else { return };
        let sections = self.editor.upgrade().map_or(0, |e| e.section_count());
        let j = window.global::<JobLoad<'_>>();
        j.set_title(format!("Load job {} into the editor?", asking.id).into());
        j.set_text(
            match sections {
                0 => "The editor has marks that are not saved in a job.".to_owned(),
                1 => "The editor has a section that is not saved in a job.".to_owned(),
                n => format!("The editor has {n} sections that are not saved in a job."),
            }
            .into(),
        );
        *self.asking.borrow_mut() = Some(asking);
        j.set_open(true);
    }

    fn answered(&self, replace: bool) {
        if let Some(window) = self.window.upgrade() {
            window.global::<JobLoad<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
        let Some(asking) = self.asking.borrow_mut().take() else {
            return;
        };
        if replace {
            self.begin(asking.id, asking.state, asking.spec);
        } else {
            Self::tell(Level::Info, "The job was not loaded; the editor keeps the open video.");
        }
    }

    /// Holds a waiting job and opens the job's source.
    fn begin(&self, id: JobId, state: JobStateKind, spec: Box<JobSpec>) {
        let Some(opener) = self.opener.upgrade() else { return };
        // Another job was edited: it is left as it is.
        if let Some(previous) = self.session.borrow_mut().take() {
            self.release(&previous);
        }
        if state == JobStateKind::Waiting && self.handle.send(QueueCommand::Hold { ids: vec![id] }).is_err() {
            tracing::warn!("[Queue] The queue engine has stopped; the job is not held");
        }
        tracing::info!(job = id.0, "[Queue] Job loaded into the editor");
        let source = spec.source.clone();
        // The player starts at the job's first section (PyCutRv, user decision Q-P8).
        let start = spec.sections.sections().next().map(|s| StartAt {
            position: s.start(),
            paused: None,
        });
        *self.session.borrow_mut() = Some(JobEdit::new(id, state, spec));
        self.loading.set(Some(id));
        let weak = BRIDGE.with(|b| b.borrow().clone());
        opener.open_with(
            source,
            start,
            Some(Box::new(move |opened| {
                if let Some(bridge) = weak.upgrade() {
                    bridge.opened(id, opened);
                }
            })),
        );
    }

    /// The job's source opened (or not): the panels take the job.
    fn opened(&self, id: JobId, opened: bool) {
        self.loading.set(None);
        let Some(spec) = self
            .session
            .borrow()
            .as_ref()
            .filter(|s| s.id == id)
            .map(|s| s.spec.clone())
        else {
            return;
        };
        if !opened {
            Self::tell(
                Level::Error,
                &format!("Job {id} cannot be edited: its source does not open."),
            );
            self.end();
            return;
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.load_sections(spec.sections.clone());
        }
        if let Some(filters) = self.filters.upgrade() {
            filters.load_chain(spec.filters.clone());
        }
        if let Some(render) = self.render.upgrade() {
            render.load_profile(&spec.profile);
        }
        if let Some(tags) = self.tags.upgrade() {
            tags.load_draft(Choice {
                tags: spec.metadata.tags.clone(),
                rating: spec.metadata.rating,
            });
        }
        if let Some(export) = self.export.upgrade() {
            export.load_target(&spec.target);
        }
        self.mark(Some(id));
        let updatable = self.session.borrow().as_ref().is_some_and(|s| s.updatable);
        let key = self.export.upgrade().map(|e| e.save_key()).unwrap_or_default();
        let how = match (updatable, key.is_empty()) {
            (true, true) => "Save updates it.".to_owned(),
            (true, false) => format!("Save ({key}) updates it."),
            (false, _) => "Saving makes a new job.".to_owned(),
        };
        Self::tell(Level::Info, &format!("Job {id} is in the editor. {how}"));
    }

    /// The export saved the edited job (updated, or as a new job): the editing ends.
    pub fn job_saved(&self) {
        self.end();
    }

    /// "Stop editing": the job stays as it is.
    pub fn stop(&self) {
        if let Some(id) = self.session.borrow().as_ref().map(|s| s.id) {
            Self::tell(Level::Info, &format!("Stopped editing job {id}; it stays as it was."));
        }
        self.end();
    }

    /// The user opened another file: the editing ends there.
    pub fn user_opened(&self, path: &Path) {
        let Some(edit) = self.session.borrow_mut().take() else {
            return;
        };
        self.release(&edit);
        self.mark(None);
        tracing::info!(job = edit.id.0, file = %path.display(), "[Queue] Editing ended: another file was opened");
    }

    /// Ends the editing: the job waits again when RustCut held it.
    fn end(&self) {
        let Some(edit) = self.session.borrow_mut().take() else {
            return;
        };
        self.release(&edit);
        self.mark(None);
    }

    fn release(&self, edit: &JobEdit) {
        if edit.release && self.handle.send(QueueCommand::Release { ids: vec![edit.id] }).is_err() {
            tracing::warn!("[Queue] The queue engine has stopped; the job is not released");
        }
    }

    /// Marks the edited job in the queue, the Job panel and the export card.
    fn mark(&self, id: Option<JobId>) {
        if let Some(queue) = self.queue.upgrade() {
            queue.set_editing(id);
            if let Some(id) = id {
                queue.set_current(id);
            }
        }
        if let Some(inspector) = self.inspector.upgrade() {
            inspector.set_editing(id);
        }
        if let Some(export) = self.export.upgrade() {
            export.editing_changed();
        }
    }

    /// Loading and editing say nothing by default (Q-N8 #33/#34: the job is highlighted instead); a job that cannot be
    /// loaded is an error toast.
    fn tell(level: Level, message: &str) {
        tracing::info!(message, "[Queue]");
        let kind = if level >= Level::Warning {
            Kind::JobLoadFailed
        } else {
            Kind::Editing
        };
        notices::post(Notice::new(kind, message).level(level));
    }
}

fn connect(bridge: &Rc<JobEditor>, window: &MainWindow) {
    let j = window.global::<JobLoad<'_>>();
    let weak = Rc::downgrade(bridge);
    j.on_replace(move || {
        if let Some(b) = weak.upgrade() {
            b.answered(true);
        }
    });
    let weak = Rc::downgrade(bridge);
    j.on_cancel(move || {
        if let Some(b) = weak.upgrade() {
            b.answered(false);
        }
    });
}
