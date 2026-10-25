//! A queued job loaded into the editor (M4.1, user decisions Q-Q3, Q-N10, Q-P8b): the job's spec is read on the
//! runtime, the current session is replaced or asked about first (`editor.on_job_load`), the job's source opens, and
//! its sections, filters, render settings, target and tags fill the panels. Loading always makes a copy (Q-P8b): the
//! export's Save makes a new job and the loaded one stays as it is, whatever its state (it is neither held nor
//! updated; replacing it is the user's own step). The link ends with a save, "Stop" or when the user opens another
//! file; the queue highlights the loaded job meanwhile.

mod session;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};

use rustcut_core::job::{JobId, JobSpec};
use rustcut_core::queue::{QueueCommand, QueueError, QueueHandle};
use rustcut_core::settings::OnJobLoad;
use slint::ComponentHandle as _;

pub use session::JobEdit;

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

/// What the export needs of the loaded job when it saves.
#[derive(Debug, Clone)]
pub struct EditInfo {
    pub id: JobId,
    /// The job as it was loaded (facts the panels may not know yet: the probe's summary, the source hash).
    pub spec: Box<JobSpec>,
}

/// A job whose spec was read, waiting for the answer to the "ask" question.
struct Asking {
    id: JobId,
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

    /// The loaded job, for the export's Save.
    pub fn info(&self) -> Option<EditInfo> {
        self.session.borrow().as_ref().map(|s| EditInfo {
            id: s.id,
            spec: s.spec.clone(),
        })
    }

    /// The card's banner text, when a job is edited.
    pub fn banner(&self) -> Option<String> {
        self.session.borrow().as_ref().map(JobEdit::banner)
    }

    /// What happens to the current session at the next load (`editor.on_job_load`, changed in the Settings window).
    pub fn set_on_job_load(&self, mode: OnJobLoad) {
        self.mode.set(mode);
    }

    /// "Load into the editor" of job `id`: reads its spec on the runtime first.
    pub fn load(&self, id: JobId) {
        if self.loading.get().is_some() {
            Self::tell(
                Level::Info,
                "A job is being loaded into the editor; try again in a moment.",
            );
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
        // A copy is loaded, so a job in any state can be (also one that renders now).
        if self.queue.upgrade().and_then(|q| q.row(id)).is_none() {
            return;
        }
        let editing = self.session.borrow().is_some();
        let has_work = self.editor.upgrade().is_some_and(|e| e.has_work());
        match self.mode.get() {
            OnJobLoad::Ask if has_work && !editing => self.ask(Asking { id, spec }),
            OnJobLoad::Replace | OnJobLoad::Ask => self.begin(id, spec),
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
            self.begin(asking.id, asking.spec);
        } else {
            Self::tell(Level::Info, "The job was not loaded; the editor keeps the open video.");
        }
    }

    /// Opens the job's source (the job itself stays as it is: a copy is edited).
    fn begin(&self, id: JobId, spec: Box<JobSpec>) {
        let Some(opener) = self.opener.upgrade() else { return };
        // Another job was loaded: the link to it ends.
        self.session.borrow_mut().take();
        tracing::info!(job = id.0, "[Queue] Job loaded into the editor as a copy");
        let source = spec.source.clone();
        // The player starts at the job's first section (PyCutRv, user decision Q-P8).
        let start = spec.sections.sections().next().map(|s| StartAt {
            position: s.start(),
            paused: None,
        });
        *self.session.borrow_mut() = Some(JobEdit::new(id, spec));
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
        Self::tell(
            Level::Info,
            &format!("Job {id} is in the editor. Saving makes a new job; job {id} stays as it is."),
        );
    }

    /// The export saved the copy as a new job: the link to the loaded job ends.
    pub fn job_saved(&self) {
        self.end();
    }

    /// "Stop" on the card's banner: the link to the loaded job ends (the editor keeps what it has).
    pub fn stop(&self) {
        if let Some(id) = self.session.borrow().as_ref().map(|s| s.id) {
            Self::tell(Level::Info, &format!("Job {id} is no longer linked to the editor."));
        }
        self.end();
    }

    /// The user opened another file: the link ends there.
    pub fn user_opened(&self, path: &Path) {
        let Some(edit) = self.session.borrow_mut().take() else {
            return;
        };
        self.mark(None);
        tracing::info!(job = edit.id.0, file = %path.display(), "[Queue] Loaded job unlinked: another file was opened");
    }

    fn end(&self) {
        if self.session.borrow_mut().take().is_some() {
            self.mark(None);
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
