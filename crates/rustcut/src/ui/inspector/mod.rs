//! The Job panel's bridge (M4.1, user decision Q-Q1): it follows the queue's current job (after every refresh of the
//! queue panel), reads the job's spec on the runtime (`QueueCommand::Spec`) when the job or its row changed, and shows
//! the details (`details.rs`) with RustView's tag names from the Tag & Rate panel. Its buttons act on that job.

mod details;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};

use rustcut_core::job::{JobId, JobSpec, JobStateKind};
use rustcut_core::queue::{JobSummary, QueueCommand, QueueError};
use slint::{ComponentHandle as _, ModelRc, VecModel};

pub use details::{DetailRow, DetailSection, Extras, SECTION_IDS, Tone, details, filter_text};

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::queue::{QueueAction, QueueBridge, file_label, kind_number, title};
use crate::ui::tags::TagsBridge;
use crate::util::model::sync;
use crate::{Job, JobRowView, JobSectionView, MainWindow};

thread_local! {
    static BRIDGE: RefCell<Weak<InspectorBridge>> = const { RefCell::new(Weak::new()) };
}

/// What the panel shows now.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    row: JobSummary,
    size: Option<u64>,
    /// `None` while it is read or when it cannot be read.
    spec: Option<Box<JobSpec>>,
}

pub struct InspectorBridge {
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    queue: Weak<QueueBridge>,
    tags: Weak<TagsBridge>,
    collapsed: RefCell<BTreeSet<String>>,
    shown: RefCell<Option<Shown>>,
    /// Counts spec reads; an answer counts only when it is the newest.
    reads: Cell<u64>,
    /// The job loaded into the editor.
    editing: Cell<Option<JobId>>,
    sections: Rc<VecModel<JobSectionView>>,
}

impl InspectorBridge {
    /// Takes over the Job panel. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        queue: &Rc<QueueBridge>,
        tags: &Rc<TagsBridge>,
        collapsed: &[String],
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            queue: Rc::downgrade(queue),
            tags: Rc::downgrade(tags),
            collapsed: RefCell::new(
                collapsed
                    .iter()
                    .filter(|id| SECTION_IDS.contains(&id.as_str()))
                    .cloned()
                    .collect(),
            ),
            shown: RefCell::new(None),
            reads: Cell::new(0),
            editing: Cell::new(None),
            sections: Rc::new(VecModel::default()),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        let j = window.global::<Job<'_>>();
        j.set_sections(ModelRc::from(Rc::clone(&bridge.sections)));
        j.set_empty_text("Click a job in the queue to see it here.".into());
        connect(&bridge, window);
        bridge
    }

    /// The folded sections for `ui-state.toml`.
    pub fn collapsed(&self) -> Vec<String> {
        self.collapsed.borrow().iter().cloned().collect()
    }

    /// The job loaded into the editor (its edit button shows it).
    pub fn set_editing(&self, id: Option<JobId>) {
        self.editing.set(id);
        self.follow_queue();
    }

    /// The queue panel refreshed: shows its current job, reading the spec again when the row changed.
    pub fn follow_queue(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let current = self.queue.upgrade().and_then(|q| q.current());
        let j = window.global::<Job<'_>>();
        let Some((row, progress, size)) = current else {
            if self.shown.borrow_mut().take().is_some() {
                self.sections.set_vec(Vec::new());
            }
            j.set_has_job(false);
            return;
        };
        let kind = row.state.kind();
        j.set_has_job(true);
        j.set_title(file_label(&row).into());
        j.set_kind(kind_number(kind));
        j.set_progress(match kind {
            JobStateKind::Rendering | JobStateKind::Paused => az::Az::az::<f32>(progress.map_or(0.0, |p| p.fraction())),
            _ => -1.0,
        });
        j.set_state(
            match (kind, progress) {
                (JobStateKind::Rendering | JobStateKind::Paused, Some(p)) => {
                    format!("{} {:.0} %", title(kind), (p.fraction() * 100.0).floor())
                }
                _ => title(kind).to_owned(),
            }
            .into(),
        );
        j.set_editing(self.editing.get() == Some(row.id));
        // Loading is a copy (user decision Q-P8b): any job can be loaded.
        j.set_can_edit(true);
        j.set_can_play(kind == JobStateKind::Finished);
        j.set_can_retry(kind.is_done());
        j.set_can_log(kind.is_done() || row.started_at.is_some());
        let (same_job, same_row, same_size) = self.shown.borrow().as_ref().map_or((false, false, false), |s| {
            (s.row.id == row.id, s.row == row, s.size == size)
        });
        if same_row && same_size {
            return;
        }
        let spec = if same_job {
            self.shown.borrow().as_ref().and_then(|s| s.spec.clone())
        } else {
            None
        };
        *self.shown.borrow_mut() = Some(Shown {
            row: row.clone(),
            size,
            spec,
        });
        self.publish_sections();
        if !same_row {
            self.read_spec(row.id);
        }
    }

    fn read_spec(&self, id: JobId) {
        let Some(queue) = self.queue.upgrade() else { return };
        let generation = self.reads.get().wrapping_add(1);
        self.reads.set(generation);
        let handle = queue.handle().clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = handle
                .request(|reply| QueueCommand::Spec { id, reply })
                .await
                .unwrap_or_else(|_gone| Err(QueueError::Store("the queue has stopped".into())));
            ui.post(move || {
                BRIDGE.with(|b| {
                    let Some(bridge) = b.borrow().upgrade() else { return };
                    if bridge.reads.get() != generation {
                        return;
                    }
                    match answer {
                        Ok(spec) => {
                            if let Some(shown) = bridge.shown.borrow_mut().as_mut().filter(|s| s.row.id == id) {
                                shown.spec = Some(spec);
                            }
                            bridge.publish_sections();
                        }
                        Err(e) => tracing::debug!(job = id.0, error = %e, "[Queue] The job's spec was not read"),
                    }
                });
            });
        });
    }

    fn publish_sections(&self) {
        let shown = self.shown.borrow();
        let Some(shown) = shown.as_ref() else { return };
        let tags = self.tags.upgrade();
        let name = |id: u32| tags.as_ref().and_then(|t| t.tag_name(id));
        let all = details(
            &shown.row,
            shown.spec.as_deref(),
            &Extras {
                size: shown.size,
                tag_name: Some(&name),
            },
        );
        let collapsed = self.collapsed.borrow();
        let views = all
            .into_iter()
            .map(|s| JobSectionView {
                id: s.id.into(),
                title: s.title.into(),
                collapsed: collapsed.contains(s.id),
                rows: ModelRc::new(VecModel::from(
                    s.rows
                        .into_iter()
                        .map(|r| JobRowView {
                            label: r.label.into(),
                            value: r.value.into(),
                            tooltip: r.tooltip.into(),
                            tone: match r.tone {
                                Tone::Normal => 0,
                                Tone::Muted => 1,
                                Tone::Error => 2,
                            },
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        sync(&self.sections, views);
    }

    fn current(&self) -> Option<JobId> {
        self.shown.borrow().as_ref().map(|s| s.row.id)
    }

    fn act(&self, action: &QueueAction) {
        if let (Some(id), Some(queue)) = (self.current(), self.queue.upgrade()) {
            queue.run_on(action, id);
        }
    }
}

fn connect(bridge: &Rc<InspectorBridge>, window: &MainWindow) {
    let j = window.global::<Job<'_>>();
    let weak = Rc::downgrade(bridge);
    j.on_toggle_section(move |id| {
        if let Some(b) = weak.upgrade() {
            {
                let mut collapsed = b.collapsed.borrow_mut();
                if !collapsed.remove(id.as_str()) {
                    collapsed.insert(id.to_string());
                }
            }
            b.publish_sections();
        }
    });
    let bind = |action: QueueAction| {
        let weak = Rc::downgrade(bridge);
        move || {
            if let Some(b) = weak.upgrade() {
                b.act(&action);
            }
        }
    };
    j.on_edit(bind(QueueAction::Edit));
    j.on_play(bind(QueueAction::Play));
    j.on_reveal(bind(QueueAction::Reveal));
    j.on_retry(bind(QueueAction::Retry));
    j.on_show_log(bind(QueueAction::ShowLog));
    j.on_edit_tags(bind(QueueAction::EditTags));
}
