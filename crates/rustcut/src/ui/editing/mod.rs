//! A queued job loaded into the editor (M4.1, user decisions Q-Q2, Q-Q3): the job's spec is read on the runtime, a
//! waiting job is held meanwhile, the editor's draft is replaced, set aside or asked about (`editor.draft_on_load`),
//! the job's source opens, and its sections, filters, render settings, target and tags fill the panels. The export's
//! Save then updates the job (same target file) or makes a new one (`session.rs`); the editing ends there, with "Stop
//! editing" or when the user opens another file, and a draft set aside comes back.

mod session;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};

use rustcut_core::job::{JobId, JobSpec, JobStateKind};
use rustcut_core::queue::{QueueCommand, QueueError, QueueHandle};
use rustcut_core::settings::DraftOnLoad;
use slint::ComponentHandle as _;

pub use session::{Draft, JobEdit, SaveAs};

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::editor::Editor;
use crate::ui::export::ExportBridge;
use crate::ui::filters::FiltersBridge;
use crate::ui::inspector::InspectorBridge;
use crate::ui::open::{FileOpener, StartAt};
use crate::ui::player::PlayerBridge;
use crate::ui::queue::QueueBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::status::{self, Level};
use crate::ui::tags::{Choice, TagsBridge};
use crate::{JobLoad, MainWindow};

thread_local! {
    static BRIDGE: RefCell<Weak<JobEditor>> = const { RefCell::new(Weak::new()) };
}

/// The parts a loaded job fills.
pub struct EditingParts<'a> {
    pub player: &'a Rc<PlayerBridge>,
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
    mode: Cell<DraftOnLoad>,
    player: Weak<PlayerBridge>,
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
        mode: DraftOnLoad,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            handle: parts.queue.handle().clone(),
            mode: Cell::new(mode),
            player: Rc::downgrade(parts.player),
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
        self.session.borrow().as_ref().map(|s| {
            let mut text = s.banner();
            if s.set_aside.is_some() {
                text.push_str(" · your draft comes back afterwards");
            }
            text
        })
    }

    /// "Edit in the editor" or "Load as a new job" of job `id`: reads its spec on the runtime first.
    pub fn load(&self, id: JobId) {
        if self.loading.get().is_some() {
            self.status(
                Level::Info,
                "A job is being loaded into the editor; try again in a moment.",
            );
            return;
        }
        if self.session.borrow().as_ref().is_some_and(|s| s.id == id) {
            self.status(Level::Info, &format!("Job {id} is in the editor already."));
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
                self.status(Level::Error, &format!("The job cannot be loaded into the editor: {e}"));
                return;
            }
        };
        let Some(state) = self.queue.upgrade().and_then(|q| q.row(id)).map(|r| r.state.kind()) else {
            return;
        };
        if state.is_active() {
            self.status(
                Level::Warning,
                &format!("Job {id} is rendering; it can be loaded when it ends."),
            );
            return;
        }
        let editing = self.session.borrow().is_some();
        let has_work = self.editor.upgrade().is_some_and(|e| e.has_work());
        match self.mode.get() {
            DraftOnLoad::Ask if has_work && !editing => self.ask(Asking { id, state, spec }),
            DraftOnLoad::SetAside => self.begin(id, state, spec, true),
            DraftOnLoad::Replace | DraftOnLoad::Ask => self.begin(id, state, spec, false),
        }
    }

    fn ask(&self, asking: Asking) {
        let Some(window) = self.window.upgrade() else { return };
        let sections = self.editor.upgrade().map_or(0, |e| e.sections_draft().list.len());
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

    fn answered(&self, set_aside: Option<bool>) {
        if let Some(window) = self.window.upgrade() {
            window.global::<JobLoad<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
        let Some(asking) = self.asking.borrow_mut().take() else {
            return;
        };
        match set_aside {
            Some(aside) => self.begin(asking.id, asking.state, asking.spec, aside),
            None => self.status(Level::Info, "The job was not loaded; the editor keeps its draft."),
        }
    }

    /// Holds a waiting job, sets the draft aside when asked to, and opens the job's source.
    fn begin(&self, id: JobId, state: JobStateKind, spec: Box<JobSpec>, set_aside: bool) {
        let Some(opener) = self.opener.upgrade() else { return };
        // Another job was edited: it is left as it is, and a draft set aside for it stays aside.
        let carried = self.session.borrow_mut().take().and_then(|previous| {
            self.release(&previous);
            previous.set_aside
        });
        let draft = carried.or_else(|| if set_aside { self.capture() } else { None });
        if state == JobStateKind::Waiting && self.handle.send(QueueCommand::Hold { ids: vec![id] }).is_err() {
            tracing::warn!("[Queue] The queue engine has stopped; the job is not held");
        }
        tracing::info!(
            job = id.0,
            set_aside = draft.is_some(),
            "[Queue] Job loaded into the editor"
        );
        let source = spec.source.clone();
        *self.session.borrow_mut() = Some(JobEdit::new(id, state, spec, draft));
        self.loading.set(Some(id));
        let weak = BRIDGE.with(|b| b.borrow().clone());
        opener.open_with(
            source,
            None,
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
            self.status(
                Level::Error,
                &format!("Job {id} cannot be edited: its source does not open."),
            );
            self.end(true);
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
        self.status(Level::Info, &format!("Job {id} is in the editor. {how}"));
    }

    /// The export saved the edited job (updated, or as a new job): the editing ends.
    pub fn job_saved(&self) {
        self.end(true);
    }

    /// "Stop editing": the job stays as it is.
    pub fn stop(&self) {
        if let Some(id) = self.session.borrow().as_ref().map(|s| s.id) {
            self.status(Level::Info, &format!("Stopped editing job {id}; it stays as it was."));
        }
        self.end(true);
    }

    /// The user opened another file: the editing ends there, and a draft set aside is dropped (its video would replace
    /// the file just opened).
    pub fn user_opened(&self, path: &Path) {
        let Some(edit) = self.session.borrow_mut().take() else {
            return;
        };
        self.release(&edit);
        self.mark(None);
        tracing::info!(job = edit.id.0, file = %path.display(), "[Queue] Editing ended: another file was opened");
        if edit.set_aside.is_some() {
            self.status(
                Level::Warning,
                &format!("Editing of job {} ended; the draft set aside was dropped.", edit.id),
            );
        }
    }

    /// Ends the editing: the job waits again when RustCut held it, and a draft set aside comes back (`restore`).
    fn end(&self, restore: bool) {
        let Some(edit) = self.session.borrow_mut().take() else {
            return;
        };
        self.release(&edit);
        self.mark(None);
        if let (true, Some(draft)) = (restore, edit.set_aside) {
            self.put_back(*draft);
        }
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

    /// The editor's draft as it is (nothing when no video is open).
    fn capture(&self) -> Option<Box<Draft>> {
        let player = self.player.upgrade()?;
        let source = player.current_file()?;
        Some(Box::new(Draft {
            source,
            position: player.position(),
            paused: player.is_paused(),
            sections: self.editor.upgrade()?.sections_draft(),
            filters: self.filters.upgrade()?.chain(),
            render: self.render.upgrade()?.selection(),
            export: self.export.upgrade()?.draft_state(),
            tags: self.tags.upgrade()?.draft(),
        }))
    }

    /// Opens the draft's video where it was and puts its panels back.
    fn put_back(&self, draft: Draft) {
        let Some(opener) = self.opener.upgrade() else { return };
        let start = StartAt {
            position: draft.position,
            paused: draft.paused,
        };
        let weak = BRIDGE.with(|b| b.borrow().clone());
        opener.open_with(
            draft.source.clone(),
            Some(start),
            Some(Box::new(move |opened| {
                let Some(bridge) = weak.upgrade() else { return };
                if !opened {
                    bridge.status(
                        Level::Warning,
                        "The draft set aside could not be restored: its video does not open.",
                    );
                    return;
                }
                if let Some(editor) = bridge.editor.upgrade() {
                    editor.put_sections(draft.sections);
                }
                if let Some(filters) = bridge.filters.upgrade() {
                    filters.load_chain(draft.filters);
                }
                if let Some(render) = bridge.render.upgrade() {
                    render.put_selection(draft.render);
                }
                if let Some(tags) = bridge.tags.upgrade() {
                    tags.load_draft(draft.tags);
                }
                if let Some(export) = bridge.export.upgrade() {
                    export.put_draft_state(draft.export);
                }
                tracing::info!("[Queue] The draft set aside is back in the editor");
            })),
        );
    }

    fn status(&self, level: Level, message: &str) {
        tracing::info!(message, "[Queue]");
        if let Some(window) = self.window.upgrade() {
            status::show(&window, level, message);
        }
    }
}

fn connect(bridge: &Rc<JobEditor>, window: &MainWindow) {
    let j = window.global::<JobLoad<'_>>();
    let weak = Rc::downgrade(bridge);
    j.on_replace(move || {
        if let Some(b) = weak.upgrade() {
            b.answered(Some(false));
        }
    });
    let weak = Rc::downgrade(bridge);
    j.on_set_aside(move || {
        if let Some(b) = weak.upgrade() {
            b.answered(Some(true));
        }
    });
    let weak = Rc::downgrade(bridge);
    j.on_cancel(move || {
        if let Some(b) = weak.upgrade() {
            b.answered(None);
        }
    });
}
