//! The Logs panel's bridge (M4.1, user decision Q-Q1; M4.3; M6.5): one of four logs, filtered by the search and
//! "errors only" (`lines.rs`, kept in `UiState.logs` with the chosen log) and copied to the clipboard on its own thread:
//! - the stored log of the queue's current job, read on the runtime (`QueueCommand::Log`) when the job changes or a
//!   render of it ends (the log of a running render is stored when the render ends);
//! - RustCut's own log of this run (`app::log_book`, the lines the log file gets), shown again while it grows;
//! - its RustView part (the link and the Tag & Rate panel);
//! - the messages of this session (`ui::notices`' history; the bell's "Show all" opens it).

mod lines;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use rustcut_core::job::{JobId, JobStateKind};
use rustcut_core::queue::{JobSummary, QueueCommand, QueueError};
use rustcut_core::settings::{LogPanelState, LogSource};
use rustcut_core::store::LogLine;
use slint::{ComponentHandle as _, ModelRc, VecModel};

pub use lines::{LineLevel, ShownLine, book_shown, level, messages_shown, shown, summary, to_text};

use crate::app::log_book::LogBook;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::queue::{QueueBridge, file_label};
use crate::util::clipboard;
use crate::util::model::sync;
use crate::{LogLineView, Logs, MainWindow};

/// Most lines read from the store (a render keeps at most 2 200; several attempts add up).
const READ_LIMIT: usize = 20_000;
/// How often the app's log is looked at while it is shown (a new line shows within this time).
const BOOK_POLL: std::time::Duration = std::time::Duration::from_millis(300);

thread_local! {
    static BRIDGE: RefCell<Weak<LogsBridge>> = const { RefCell::new(Weak::new()) };
}

/// What the log was read for: a new read is due when the job, its attempt or its end changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadFor {
    id: JobId,
    attempt: u32,
    state: JobStateKind,
    finished_at: Option<i64>,
}

impl ReadFor {
    fn of(row: &JobSummary) -> Self {
        Self {
            id: row.id,
            attempt: row.attempt,
            state: row.state.kind(),
            finished_at: row.finished_at,
        }
    }
}

pub struct LogsBridge {
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    queue: Weak<QueueBridge>,
    state: RefCell<LogPanelState>,
    read_for: Cell<Option<ReadFor>>,
    lines: RefCell<Vec<LogLine>>,
    reads: Cell<u64>,
    model: Rc<VecModel<LogLineView>>,
    /// The app's log of this run, and the number of its newest line as last shown.
    book: LogBook,
    book_shown: Cell<u64>,
    book_timer: slint::Timer,
    /// The number of the newest message as last shown (`LogSource::Messages`).
    messages_shown: Cell<u64>,
}

impl LogsBridge {
    /// Takes over the Logs panel. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        queue: &Rc<QueueBridge>,
        saved: &LogPanelState,
        book: LogBook,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            queue: Rc::downgrade(queue),
            state: RefCell::new(saved.clone()),
            read_for: Cell::new(None),
            lines: RefCell::new(Vec::new()),
            reads: Cell::new(0),
            model: Rc::new(VecModel::default()),
            book,
            book_shown: Cell::new(0),
            book_timer: slint::Timer::default(),
            messages_shown: Cell::new(0),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        let l = window.global::<Logs<'_>>();
        l.set_lines(ModelRc::from(Rc::clone(&bridge.model)));
        l.set_search(saved.search.as_str().into());
        l.set_errors_only(saved.errors_only);
        l.set_shareable(saved.shareable);
        connect(&bridge, window);
        bridge.source_changed();
        bridge
    }

    fn source(&self) -> LogSource {
        self.state.borrow().source
    }

    /// A message's "Show log": RustCut's own log of this run.
    pub fn show_app_log(self: &Rc<Self>) {
        self.select_source(LogSource::App);
    }

    /// The bell's "Show all": the messages of this session.
    pub fn show_messages(self: &Rc<Self>) {
        self.select_source(LogSource::Messages);
    }

    /// Shows `source` from now on.
    fn select_source(self: &Rc<Self>, source: LogSource) {
        if self.source() == source {
            return;
        }
        self.state.borrow_mut().source = source;
        tracing::debug!(?source, "[Logs] Showing another log");
        self.source_changed();
    }

    /// The header and the lines of the chosen log; the app's log is looked at again while it is shown.
    fn source_changed(self: &Rc<Self>) {
        let source = self.source();
        if source == LogSource::Job {
            self.book_timer.stop();
        } else {
            let weak = Rc::downgrade(self);
            self.book_timer.start(slint::TimerMode::Repeated, BOOK_POLL, move || {
                if let Some(b) = weak.upgrade() {
                    let grown = if b.source() == LogSource::Messages {
                        notices::history_seq() != b.messages_shown.get()
                    } else {
                        b.book.latest() != b.book_shown.get()
                    };
                    if grown {
                        b.publish();
                    }
                }
            });
        }
        if let Some(window) = self.window.upgrade() {
            let l = window.global::<Logs<'_>>();
            l.set_source(match source {
                LogSource::Job => 0,
                LogSource::App => 1,
                LogSource::RustView => 2,
                LogSource::Messages => 3,
            });
            match source {
                LogSource::Job => {}
                LogSource::App => {
                    l.set_has_job(true);
                    l.set_title("RustCut · this run".into());
                    l.set_note("".into());
                }
                LogSource::RustView => {
                    l.set_has_job(true);
                    l.set_title("RustView link · this run".into());
                    l.set_note("".into());
                }
                LogSource::Messages => {
                    l.set_has_job(true);
                    l.set_title("Messages · this session".into());
                    l.set_note("".into());
                }
            }
        }
        if source == LogSource::Job {
            self.read_for.set(None);
            self.follow_queue();
        }
        self.publish();
    }

    /// What `ui-state.toml` keeps of the panel.
    pub fn save_into(&self, saved: &mut LogPanelState) {
        saved.clone_from(&self.state.borrow());
    }

    /// The queue panel refreshed: reads the log again when its job changed or a render of it ended.
    pub fn follow_queue(&self) {
        if self.source() != LogSource::Job {
            return;
        }
        let row = self.queue.upgrade().and_then(|q| q.current()).map(|(row, _, _)| row);
        let Some(window) = self.window.upgrade() else { return };
        let l = window.global::<Logs<'_>>();
        let Some(row) = row else {
            if self.read_for.take().is_some() {
                self.lines.borrow_mut().clear();
                self.publish();
            }
            l.set_has_job(false);
            l.set_note("Click a job in the queue to see its log here.".into());
            return;
        };
        l.set_has_job(true);
        l.set_title(format!("{} · job {}", file_label(&row), row.id).into());
        let wanted = ReadFor::of(&row);
        if self.read_for.get() != Some(wanted) {
            self.read_for.set(Some(wanted));
            self.read(row.id);
        }
        self.publish_note(&row);
    }

    fn publish_note(&self, row: &JobSummary) {
        let Some(window) = self.window.upgrade() else { return };
        let kind = row.state.kind();
        let empty = self.lines.borrow().is_empty();
        let note = if kind.is_active() {
            "The log of a running render is stored when it ends.".to_owned()
        } else if empty && row.started_at.is_none() {
            "The job has not rendered yet.".to_owned()
        } else if empty {
            "No log was stored for this job.".to_owned()
        } else {
            String::new()
        };
        window.global::<Logs<'_>>().set_note(note.into());
    }

    fn read(&self, id: JobId) {
        let Some(queue) = self.queue.upgrade() else { return };
        let generation = self.reads.get().wrapping_add(1);
        self.reads.set(generation);
        let handle = queue.handle().clone();
        let ui = self.ui.clone();
        if let Some(window) = self.window.upgrade() {
            window.global::<Logs<'_>>().set_loading(true);
        }
        self.runtime.spawn(async move {
            let answer = handle
                .request(|reply| QueueCommand::Log {
                    id,
                    limit: READ_LIMIT,
                    reply,
                })
                .await
                .unwrap_or_else(|_gone| Err(QueueError::Store("the queue has stopped".into())));
            ui.post(move || {
                BRIDGE.with(|b| {
                    let Some(bridge) = b.borrow().upgrade() else { return };
                    if bridge.reads.get() != generation {
                        return;
                    }
                    if let Some(window) = bridge.window.upgrade() {
                        window.global::<Logs<'_>>().set_loading(false);
                    }
                    match answer {
                        Ok(lines) => *bridge.lines.borrow_mut() = lines,
                        Err(e) => {
                            tracing::debug!(job = id.0, error = %e, "[Queue] The job's log was not read");
                            bridge.lines.borrow_mut().clear();
                        }
                    }
                    bridge.publish();
                    if let Some(row) = bridge.queue.upgrade().and_then(|q| q.row(id)) {
                        bridge.publish_note(&row);
                    }
                });
            });
        });
    }

    /// The lines of the chosen log that pass the filter (as the shareable copy shows them while it is chosen), and how
    /// many it has.
    fn current(&self) -> (Vec<ShownLine>, usize) {
        let (mut lines, total) = self.filtered();
        if self.state.borrow().shareable {
            for line in &mut lines {
                line.text = crate::app::redaction::redact(&line.text);
            }
        }
        (lines, total)
    }

    fn filtered(&self) -> (Vec<ShownLine>, usize) {
        let st = self.state.borrow();
        if st.source == LogSource::Job {
            let lines = self.lines.borrow();
            return (shown(&lines, &st.search, st.errors_only), lines.len());
        }
        if st.source == LogSource::Messages {
            let (seq, entries) = notices::history_since(0);
            self.messages_shown.set(seq);
            let total = entries.len();
            return (messages_shown(&entries, &st.search, st.errors_only), total);
        }
        let lines = self.book.lines();
        self.book_shown.set(lines.last().map_or(0, |l| l.seq));
        let total = book_shown(&lines, st.source, "", false).len();
        (book_shown(&lines, st.source, &st.search, st.errors_only), total)
    }

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let (shown, total) = self.current();
        let st = self.state.borrow();
        let l = window.global::<Logs<'_>>();
        l.set_summary(summary(total, &shown).into());
        l.set_follow(st.follow);
        sync(
            &self.model,
            shown
                .into_iter()
                .map(|line| LogLineView {
                    number: i32::try_from(line.number).unwrap_or(i32::MAX),
                    text: line.text.into(),
                    level: line.level as i32,
                })
                .collect(),
        );
    }

    fn copy(&self) {
        let text = to_text(&self.current().0);
        let lines = text.lines().count();
        if clipboard::copy(text) {
            notices::post(Notice::new(Kind::LogCopied, format!("{lines} log lines copied")));
        } else {
            tracing::warn!("[Logs] The clipboard cannot be used");
            notices::post(Notice::new(Kind::ClipboardFailed, "The clipboard cannot be used"));
        }
    }
}

fn connect(bridge: &Rc<LogsBridge>, window: &MainWindow) {
    let l = window.global::<Logs<'_>>();
    let weak = Rc::downgrade(bridge);
    l.on_search_edited(move |text| {
        if let Some(b) = weak.upgrade() {
            b.state.borrow_mut().search = text.to_string();
            b.publish();
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_toggle_errors_only(move || {
        if let Some(b) = weak.upgrade() {
            let on = !b.state.borrow().errors_only;
            b.state.borrow_mut().errors_only = on;
            if let Some(window) = b.window.upgrade() {
                window.global::<Logs<'_>>().set_errors_only(on);
            }
            b.publish();
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_toggle_shareable(move || {
        if let Some(b) = weak.upgrade() {
            let on = !b.state.borrow().shareable;
            b.state.borrow_mut().shareable = on;
            if let Some(window) = b.window.upgrade() {
                window.global::<Logs<'_>>().set_shareable(on);
            }
            b.publish();
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_toggle_follow(move || {
        if let Some(b) = weak.upgrade() {
            let on = !b.state.borrow().follow;
            b.state.borrow_mut().follow = on;
            b.publish();
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_select_source(move |index| {
        if let Some(b) = weak.upgrade() {
            b.select_source(match index {
                1 => LogSource::App,
                2 => LogSource::RustView,
                3 => LogSource::Messages,
                _ => LogSource::Job,
            });
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_copy(move || {
        if let Some(b) = weak.upgrade() {
            b.copy();
        }
    });
    let weak = Rc::downgrade(bridge);
    l.on_refresh(move || {
        if let Some(b) = weak.upgrade()
            && let Some(read) = b.read_for.get()
        {
            b.read(read.id);
        }
    });
}
