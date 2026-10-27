//! The Tag & Rate panel (M3.2): RustView's tag tree, rating bar, tag filter and tag history (copies, ADR-10) for the
//! next job, or for a queued job whose RustView entry is edited (user decision Q-G3). The sync actor sends the tag tree
//! and the connection state (`SyncEvent::TagTreeChanged`, `SyncEvent::Connection`); a runtime task follows its events
//! and hands them to the UI thread with `UiDispatcher::post`. The tags and the rating go into `JobMetadata` when the
//! export saves the job (`draft_metadata`), and the sync actor writes them; an edited job gets them through
//! `QueueCommand::UpdateMetadata`. The row under the history holds PyCutRv's switches (user decision Q-P8b): "Active"
//! (off = the next jobs are saved without RustView, kept in `ui-state.toml`) and "Warn" (`warnings.missing_metadata`,
//! changed through the settings writer); when either is still off as RustView answers for the first time in a run, a
//! message says so once. Nothing here waits on the UI thread.

mod filter_window;
mod hidden;
mod history;
mod state;
mod switches;
mod sync_notice;
mod tree;
mod view;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use rustcut_core::job::JobId;
use rustcut_core::queue::{QueueCommand, QueueError, QueueHandle};
use rustcut_core::rustview::{ConnectionState, RustViewError, SyncCommand, SyncEvent, SyncService, TagTree};
use rustcut_core::settings::TagPanelState;
use slint::{ComponentHandle as _, Model as _, ModelRc, VecModel};
use tokio::sync::broadcast::error::RecvError;

pub use hidden::{Check, FilterRow};
pub use history::{TagHistoryItem, TagHistoryLayoutEngine, TagHistoryManager, TagHistoryRow};
pub use state::{Choice, Editing, TagPanel};
pub use tree::{
    MAX_TAG_ITEM_WIDTH, MIN_TAG_ITEM_WIDTH, TAG_COLUMN_SPACING, TAG_ITEM_HEIGHT, TAG_PADDING_H, TAG_PADDING_V,
    TAG_ROW_SPACING, TagFlattenContext, TagTreeBuilder, TagTreeNode,
};

use crate::app::SettingsWriter;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::jump::JumpTarget;
use crate::ui::notices::{self, Kind, Notice};
use crate::util::model::sync_keyed;
use crate::{
    MainWindow, TagColumnView, TagFilterRowView, TagFilterWindow, TagHistoryItemModel, TagHistoryRowModel, TagNodeView,
    TagRate,
};

/// The tree area's height until the panel reports it, in logical pixels: RustView's start value
/// (`AppState::tag_tree_available_height`), so the first columns are laid out like RustView's.
const INITIAL_TREE_HEIGHT: f32 = 450.0;

thread_local! {
    /// The bridge, for what the runtime task hands to the UI thread.
    static BRIDGE: RefCell<Weak<TagsBridge>> = const { RefCell::new(Weak::new()) };
}

/// Runs `job` with the bridge on the UI thread.
fn with_bridge(ui: &UiDispatcher, job: impl FnOnce(&Rc<TagsBridge>) + Send + 'static) {
    ui.post(move || {
        BRIDGE.with(|b| {
            if let Some(bridge) = b.borrow().upgrade() {
                job(&bridge);
            }
        });
    });
}

/// Hears the "Active" switch.
type ActiveHook = Box<dyn Fn(bool)>;

pub struct TagsBridge {
    window: slint::Weak<MainWindow>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    sync: SyncService,
    queue: QueueHandle,
    state: RefCell<TagPanel>,
    connection: RefCell<ConnectionState>,
    /// The setting the header's jump mark leads to (Q-N6).
    connection_jump: Cell<Option<JumpTarget>>,
    /// The tree area's height and how many rows fit into one of its columns.
    tree_height: Cell<f32>,
    rows_per_column: Cell<usize>,
    history_width: Cell<f32>,
    columns: Rc<VecModel<TagColumnView>>,
    /// One row model per column, kept so Slint keeps the items of rows that did not change.
    column_rows: RefCell<Vec<Rc<VecModel<TagNodeView>>>>,
    history_rows: Rc<VecModel<TagHistoryRowModel>>,
    /// An Apply waits for the queue.
    applying: Cell<bool>,
    /// Shows the panel in the dock (it may sit in a collapsed sidebar or behind another tab).
    on_reveal: RefCell<Option<Box<dyn Fn()>>>,
    /// Told whenever "Active" changes (the media panel registers opened videos in RustView only while it is on).
    on_active: RefCell<Option<ActiveHook>>,
    /// The "Warn" switch is a setting (`warnings.missing_metadata`).
    writer: SettingsWriter,
    warn: Cell<bool>,
    /// RustView answered once in this run: the start message about switches left off was decided.
    start_checked: Cell<bool>,
    /// The tag filter window (made when it is opened first) and its rows.
    filter_window: RefCell<Option<TagFilterWindow>>,
    filter_rows: Rc<VecModel<TagFilterRowView>>,
}

impl TagsBridge {
    /// Whether RustView can be reached (the Diagnostics page).
    pub fn connection_state(&self) -> ConnectionState {
        self.connection.borrow().clone()
    }

    /// The names of the tags `ids` as the tree has them ("Travel, Iceland").
    pub fn names_of(&self, ids: &[u32]) -> String {
        view::tag_names(&self.state.borrow(), ids)
    }

    /// The RustView link (lookups of other panels: the known-source dialog's ratings and tags).
    pub fn sync_handle(&self) -> rustcut_core::rustview::SyncHandle {
        self.sync.handle.clone()
    }

    /// Takes over the Tag & Rate panel. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        services: (&UiDispatcher, tokio::runtime::Handle, &SyncService, QueueHandle),
        writer: SettingsWriter,
        enabled: bool,
        saved: &TagPanelState,
    ) -> Rc<Self> {
        let (ui, runtime, sync, queue) = services;
        let warn = writer.current().warnings.missing_metadata;
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            ui: ui.clone(),
            runtime,
            sync: sync.clone(),
            queue,
            state: RefCell::new(TagPanel::new(saved)),
            connection: RefCell::new(if enabled {
                ConnectionState::Unknown
            } else {
                ConnectionState::Disabled
            }),
            tree_height: Cell::new(INITIAL_TREE_HEIGHT),
            connection_jump: Cell::new(None),
            rows_per_column: Cell::new(1),
            history_width: Cell::new(0.0),
            columns: Rc::new(VecModel::default()),
            column_rows: RefCell::new(Vec::new()),
            history_rows: Rc::new(VecModel::default()),
            applying: Cell::new(false),
            on_reveal: RefCell::new(None),
            on_active: RefCell::new(None),
            writer,
            warn: Cell::new(warn),
            start_checked: Cell::new(false),
            filter_window: RefCell::new(None),
            filter_rows: Rc::new(VecModel::default()),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        let t = window.global::<TagRate<'_>>();
        t.set_columns(ModelRc::from(Rc::clone(&bridge.columns)));
        t.set_history_rows(ModelRc::from(Rc::clone(&bridge.history_rows)));
        connect(&bridge, window);
        bridge.runtime.spawn(follow(bridge.sync.clone(), ui.clone()));
        if enabled {
            bridge.load_tree();
        }
        bridge.publish();
        bridge
    }

    /// How many tags the history keeps (`rustview.tag_history_size`, also while RustCut runs).
    pub fn set_history_size(&self, size: u8) {
        self.state.borrow_mut().set_history_size(usize::from(size));
        self.publish();
    }

    pub fn set_on_reveal(&self, hook: impl Fn() + 'static) {
        *self.on_reveal.borrow_mut() = Some(Box::new(hook));
    }

    /// `hook` hears the "Active" switch now and after every change.
    pub fn set_on_active(&self, hook: impl Fn(bool) + 'static) {
        hook(self.active());
        *self.on_active.borrow_mut() = Some(Box::new(hook));
    }

    /// What `ui-state.toml` keeps of the panel.
    pub fn save_into(&self, saved: &mut TagPanelState) {
        self.state.borrow().save_into(saved);
    }

    /// The next job's tags and rating (the export puts them into `JobMetadata`).
    pub fn draft_metadata(&self) -> (Vec<u32>, Option<u8>) {
        self.state.borrow().draft_metadata()
    }

    /// Whether RustView offers tags to choose (the save warns about missing tags only then).
    pub fn can_choose_tags(&self) -> bool {
        self.state.borrow().tag_count() > 0
    }

    /// Whether the RustView link is on (the save warns about missing tags or rating only then).
    pub fn enabled(&self) -> bool {
        *self.connection.borrow() != ConnectionState::Disabled
    }

    /// The export queued a job with these values: they become "last", the draft starts again with the locked tags.
    pub fn job_saved(&self, tags: &[u32], rating: Option<u8>) {
        self.state.borrow_mut().saved(tags, rating);
        self.publish();
    }

    pub fn editing(&self) -> bool {
        self.state.borrow().editing.is_some()
    }

    /// RustView's name of tag `id` (the Job panel lists a job's tags by name).
    pub fn tag_name(&self, id: u32) -> Option<String> {
        self.state.borrow().name(id).map(str::to_owned)
    }

    /// A job loaded into the editor: its tags and rating become the panel's choice.
    pub fn load_draft(&self, choice: Choice) {
        self.state.borrow_mut().load_draft(choice);
        self.publish();
    }

    /// Shows the panel (the save dialog's "Back to editing").
    pub fn reveal(&self) {
        if let Some(hook) = self.on_reveal.borrow().as_ref() {
            hook();
        }
    }

    /// The queue's "Edit tags and rating": reads the job's metadata on the runtime, then the panel edits it.
    pub fn edit_job(&self, id: JobId, name: String) {
        let queue = self.queue.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = queue
                .request(|reply| QueueCommand::Metadata { id, reply })
                .await
                .unwrap_or_else(|_gone| Err(QueueError::Store("the queue has stopped".into())));
            with_bridge(&ui, move |b| match answer {
                Ok(metadata) if metadata.without_rustview => {
                    Self::tell(
                        Notice::new(Kind::TagsFailed, "This job has no RustView entry")
                            .detail("It was saved while Active was off: RustView knows nothing of it.")
                            .level(notices::Level::Info),
                    );
                }
                Ok(metadata) => {
                    tracing::info!(job = id.0, "[Tags] Editing the RustView entry of a job");
                    b.state.borrow_mut().begin_edit(id, name, metadata);
                    b.reveal();
                    b.publish();
                }
                Err(e) => {
                    Self::tell(Notice::new(Kind::TagsFailed, "The job's tags cannot be edited").detail(e.to_string()));
                }
            });
        });
    }

    /// Escape or Cancel: back to the draft.
    pub fn cancel_edit(&self) {
        if self.state.borrow_mut().end_edit().is_some() {
            // Nothing changed: no message (Q-N8 #36), only the history notes it.
            Self::tell(Notice::new(
                Kind::Editing,
                "The job's tags and rating were left as they were",
            ));
        }
        self.publish();
    }

    fn apply_edit(&self) {
        if self.applying.get() {
            return;
        }
        let Some((id, metadata)) = self.state.borrow().edited() else {
            self.cancel_edit();
            return;
        };
        self.applying.set(true);
        self.publish();
        let queue = self.queue.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = queue
                .request(|reply| QueueCommand::UpdateMetadata { id, metadata, reply })
                .await
                .unwrap_or_else(|_gone| Err(QueueError::Store("the queue has stopped".into())));
            with_bridge(&ui, move |b| {
                b.applying.set(false);
                match answer {
                    Ok(()) => {
                        let name = b.state.borrow_mut().end_edit().map(|e| e.name).unwrap_or_default();
                        Self::tell(Notice::new(
                            Kind::TagsUpdated,
                            format!("Tags and rating of \"{name}\" updated"),
                        ));
                    }
                    Err(e) => Self::tell(
                        Notice::new(Kind::TagsFailed, "The job's tags were not changed").detail(e.to_string()),
                    ),
                }
                b.publish();
            });
        });
    }

    /// Asks the sync actor for the tag tree (cached there; loaded again after a reconnect).
    fn load_tree(&self) {
        let handle = self.sync.handle.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let answer = handle
                .request(|reply| SyncCommand::TagTree { reply })
                .await
                .unwrap_or_else(|_gone| Err(RustViewError::Unavailable("the RustView sync has stopped".into())));
            with_bridge(&ui, move |b| b.tree_answer(answer));
        });
    }

    fn tree_answer(&self, answer: Result<std::sync::Arc<TagTree>, RustViewError>) {
        match answer {
            Ok(tree) => {
                if *self.connection.borrow() == ConnectionState::Unknown {
                    *self.connection.borrow_mut() = ConnectionState::Connected;
                }
                self.tree_changed(tree);
                if *self.connection.borrow() == ConnectionState::Connected {
                    self.connected();
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "[Tags] No tag tree yet");
                if *self.connection.borrow() == ConnectionState::Unknown {
                    *self.connection.borrow_mut() = ConnectionState::Unavailable {
                        reason: e.to_string(),
                        retry_in_secs: 0,
                    };
                }
                self.publish();
            }
        }
    }

    fn tree_changed(&self, tree: std::sync::Arc<TagTree>) {
        // The shareable log hides the tag names (P9.17).
        for tag in &tree.tags {
            crate::app::redaction::learn_tag(&tag.name);
        }
        tracing::debug!(tags = tree.tags.len(), "[Tags] Tag tree loaded");
        self.state.borrow_mut().set_tree(tree);
        self.publish();
    }

    fn event(&self, event: SyncEvent) {
        match event {
            SyncEvent::Connection(state) => {
                let connected = state == ConnectionState::Connected;
                let reconnected = connected && !self.state.borrow().has_tree();
                *self.connection.borrow_mut() = state;
                if reconnected {
                    self.load_tree();
                }
                self.publish();
                if connected {
                    self.connected();
                }
            }
            SyncEvent::TagTreeChanged(tree) => self.tree_changed(tree),
            SyncEvent::JobSynced { id, record } => sync_notice::job_synced(id, &record),
        }
    }

    fn retry(&self) {
        tracing::info!("[Tags] Retry: RustView is asked now");
        if self.sync.handle.send(SyncCommand::Probe).is_err() {
            tracing::warn!("[Tags] The RustView sync has stopped");
        }
        if !self.state.borrow().has_tree() {
            self.load_tree();
        }
    }

    /// Changes the state and shows it.
    fn change(&self, change: impl FnOnce(&mut TagPanel)) {
        change(&mut self.state.borrow_mut());
        self.publish();
    }

    fn tell(notice: Notice) {
        tracing::info!("[Tags] {}", notice.text());
        notices::post(notice);
    }

    /// Shows the whole panel.
    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let t = window.global::<TagRate<'_>>();
        let state = self.state.borrow();
        let connection = self.connection.borrow();
        let header = view::connection(&connection);
        t.set_connection_text(header.text.into());
        t.set_connection_error(header.error.into());
        t.set_connection_kind(header.kind);
        t.set_can_retry(header.can_retry);
        self.connection_jump.set(header.jump);

        let editing = state.editing.as_ref();
        t.set_editing_job(editing.is_some());
        t.set_editing_title(
            editing
                .map(|e| format!("Editing: {}", e.name))
                .unwrap_or_default()
                .into(),
        );
        t.set_editing_tooltip(
            editing
                .map(|e| {
                    format!(
                        "The tags and the rating of job {} ({}); Apply writes them.",
                        e.id, e.name
                    )
                })
                .unwrap_or_default()
                .into(),
        );
        t.set_can_apply(!self.applying.get() && state.edited().is_some());

        let choice = state.choice();
        t.set_rating(choice.rating.map_or(-1, i32::from));
        t.set_has_last_tags(!state.last_tags.is_empty());
        t.set_last_tags_tooltip(view::tag_names(&state, &state.last_tags).into());
        t.set_last_rating(state.last_rating.map_or(-1, i32::from));
        t.set_can_clear(state.can_clear());
        t.set_active(state.active);
        t.set_warn(self.warn.get());
        t.set_has_tree(state.tag_count() > 0);
        t.set_empty_text(view::empty_text(&state, &connection).into());
        t.set_show_hidden(state.show_hidden);
        t.set_eye_tooltip(view::eye_tooltip(&state).into());
        t.set_hidden_count(i32::try_from(state.hidden_count()).unwrap_or(i32::MAX));
        if t.get_filter_text() != state.filter.as_str() {
            t.set_filter_text(state.filter.as_str().into());
        }
        t.set_filter_regex(state.regex);
        t.set_filter_invalid(state.filter_regex().is_err());
        t.set_history_count(i32::try_from(state.history_items().len()).unwrap_or(i32::MAX));
        t.set_history_collapsed(state.history_collapsed);
        t.set_history_height(az::cast::<u32, f32>(state.history_height));
        drop(connection);
        drop(state);
        self.publish_tree();
        self.publish_history();
        self.publish_filter();
    }

    /// The rows in columns, synced in place.
    fn publish_tree(&self) {
        let rows = self.state.borrow().rows();
        let (chunks, per) = TagTreeBuilder::chunk_into_columns(&rows, self.tree_height.get());
        self.rows_per_column.set(per);
        let mut models = self.column_rows.borrow_mut();
        let count = chunks.len();
        for (index, chunk) in chunks.into_iter().enumerate() {
            if let Some(model) = models.get(index) {
                // Keyed by tag and expansion: RustView's `NodeButton` (a copy, ADR-10) assigns its own `is-expanded`
                // on a click, which ends its binding to the row; a row reused for another tag or state then showed a
                // stale "+" or "-". A row whose tag or expansion changes gets a new element (bound again).
                sync_keyed(model, chunk, |row| (row.id, row.is_expanded));
            } else {
                let model = Rc::new(VecModel::from(chunk));
                self.columns.push(tree::column_view(&model));
                models.push(model);
            }
        }
        models.truncate(count);
        while self.columns.row_count() > count {
            self.columns.remove(self.columns.row_count() - 1);
        }
    }

    fn publish_history(&self) {
        let items = self.state.borrow().history_items();
        let (rows, _) = TagHistoryLayoutEngine::pack_into_rows(items, self.history_width.get());
        let rows: Vec<TagHistoryRowModel> = rows
            .into_iter()
            .map(|row| TagHistoryRowModel {
                items: ModelRc::new(VecModel::from(
                    row.items.into_iter().map(history_item_view).collect::<Vec<_>>(),
                )),
            })
            .collect();
        self.history_rows.set_vec(rows);
    }

    fn tree_height_changed(&self, height: f32) {
        tracing::debug!(height, "[Tags] Tag tree height");
        self.tree_height.set(height);
        if TagTreeBuilder::rows_per_column(height) != self.rows_per_column.get() {
            self.publish_tree();
        }
    }

    fn history_width_changed(&self, width: f32) {
        tracing::debug!(width, "[Tags] Tag history width");
        if (width - self.history_width.get()).abs() > 0.5 {
            self.history_width.set(width);
            self.publish_history();
        }
    }
}

fn history_item_view(item: TagHistoryItem) -> TagHistoryItemModel {
    TagHistoryItemModel {
        id: i32::try_from(item.id).unwrap_or(i32::MAX),
        name: item.name.into(),
        display_text: item.display_text.into(),
        is_assigned: item.is_assigned,
        is_partial: item.is_partial,
        is_bold: item.is_bold,
        is_italic: item.is_italic,
        width: item.width,
        tooltip: item.tooltip.into(),
    }
}

/// A tag id from Slint (negative = none).
fn tag_id(id: i32) -> Option<u32> {
    u32::try_from(id).ok()
}

fn connect(bridge: &Rc<TagsBridge>, window: &MainWindow) {
    let t = window.global::<TagRate<'_>>();
    macro_rules! on {
        ($setter:ident, |$b:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = Rc::downgrade(bridge);
            t.$setter(move |$($arg),*| {
                if let Some($b) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_retry, |b| b.retry());
    on!(on_connection_jump_clicked, |b| crate::ui::jump::go(
        b.connection_jump.get().unwrap_or(JumpTarget::Settings {
            page: crate::ui::settings::page::Page::RustView,
            key: "rustview.enabled",
        })
    ));
    on!(on_apply_edit, |b| b.apply_edit());
    on!(on_cancel_edit, |b| b.cancel_edit());
    on!(on_rating_clicked, |b, rating| b.change(|s| s.click_rating(rating)));
    on!(on_tag_clicked, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.click_tag(id));
    });
    on!(on_tag_right_clicked, |b, id| if let Some(id) = tag_id(id) {
        let locked = b.state.borrow_mut().toggle_lock(id);
        if locked {
            b.publish();
        }
    });
    on!(on_tag_toggle_expand, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.toggle_expanded(id));
    });
    on!(on_filter_changed, |b, text| b.change(|s| s.filter = text.to_string()));
    on!(on_filter_regex_toggled, |b, on| b.change(|s| s.regex = on));
    on!(on_filter_done, |b| b.publish());
    on!(on_tree_height_changed, |b, height| b.tree_height_changed(height));
    on!(on_history_tag_clicked, |b, id| if let Some(id) = tag_id(id) {
        b.change(|s| s.click_tag(id));
    });
    on!(on_history_width_changed, |b, width| b.history_width_changed(width));
    on!(on_history_collapsed_changed, |b, collapsed| b
        .state
        .borrow_mut()
        .history_collapsed =
        collapsed);
    on!(on_history_resized, |b, height| {
        let range = TagPanelState::HISTORY_HEIGHT;
        let height = az::checked_cast::<f32, u32>(height.round()).unwrap_or(*range.start());
        b.state.borrow_mut().history_height = height.clamp(*range.start(), *range.end());
    });
    on!(on_apply_last_tags, |b| b.change(TagPanel::apply_last_tags));
    on!(on_apply_last_rating, |b| b.change(TagPanel::apply_last_rating));
    on!(on_clear, |b| b.change(TagPanel::clear));
    on!(on_toggle_show_hidden, |b| b.change(TagPanel::toggle_show_hidden));
    on!(on_toggle_active, |b| b.toggle_active());
    on!(on_open_filter, |b| b.open_filter());
    on!(on_toggle_warn, |b| b.toggle_warn());
}

/// Follows the sync actor's events; a subscriber that fell behind asks for the tree again.
async fn follow(sync: SyncService, ui: UiDispatcher) {
    let mut events = sync.events.subscribe();
    loop {
        match events.recv().await {
            Ok(SyncEvent::JobSynced { .. }) => {}
            Ok(event) => with_bridge(&ui, move |b| b.event(event)),
            Err(RecvError::Lagged(missed)) => {
                tracing::debug!(missed, "[Tags] Fell behind the RustView sync; loading the tree again");
                with_bridge(&ui, |b| b.load_tree());
            }
            Err(RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{INITIAL_TREE_HEIGHT, TagTreeBuilder};

    #[test]
    fn the_first_columns_hold_many_rows_like_rustview() {
        // Before the panel reports its height the tree is split for RustView's start height: one row per column
        // (every tag beside the other) was the bug of 0.0.21 (user report Q-N5).
        assert_eq!(TagTreeBuilder::rows_per_column(INITIAL_TREE_HEIGHT), 18);
    }
}
