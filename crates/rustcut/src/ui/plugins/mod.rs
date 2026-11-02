//! The plugins in the app (P12 block 8, ADR-17, Q-P12g): what happens in the editor becomes plugin events, what the
//! plugins answer is applied on the UI thread, failures become messages. The service itself runs on the runtime
//! (`rustcut_core::plugins`); the UI thread only sends and applies.
//!
//! Applied in order: a later plugin's change wins. A target the user chose for this video stays (the export keeps that
//! rule), and an answer about an earlier video is dropped.

mod apply;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use rustcut_core::plugins::{Context, Event, Failure, OpenedBy, PluginCommand, PluginEvent, PluginInfo, PluginService};
use tokio::sync::broadcast::error::RecvError;

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::export::ExportBridge;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::tags::TagsBridge;

thread_local! {
    static BRIDGE: RefCell<Weak<PluginsBridge>> = const { RefCell::new(Weak::new()) };
}

/// Opens the Settings page of plugin `id` (the marks at the folder field and the rating, messages).
pub fn show_settings(id: &str) {
    if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade())
        && let Some(open) = bridge.open_page.borrow().as_ref()
    {
        open(id);
    }
}

fn with_bridge(ui: &UiDispatcher, job: impl FnOnce(&Rc<PluginsBridge>) + Send + 'static) {
    ui.post(move || {
        if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) {
            job(&bridge);
        }
    });
}

/// Opens a plugin's Settings page.
type OpenPage = Box<dyn Fn(&str)>;

pub struct PluginsBridge {
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    service: PluginService,
    export: Weak<ExportBridge>,
    tags: Weak<TagsBridge>,
    /// Counts the opened videos: an answer about an earlier one is dropped.
    generation: Cell<u64>,
    /// The plugins as the service last reported them, and whether it reported at all.
    list: RefCell<Arc<Vec<PluginInfo>>>,
    listed: Cell<bool>,
    /// Told when the list changed (the Settings pages).
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
    /// Opens a plugin's Settings page (set by the Settings window).
    open_page: RefCell<Option<OpenPage>>,
}

impl PluginsBridge {
    pub fn install(
        ui: &UiDispatcher,
        runtime: tokio::runtime::Handle,
        service: PluginService,
        export: &Rc<ExportBridge>,
        tags: &Rc<TagsBridge>,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            ui: ui.clone(),
            runtime,
            service,
            export: Rc::downgrade(export),
            tags: Rc::downgrade(tags),
            generation: Cell::new(0),
            list: RefCell::new(Arc::new(Vec::new())),
            listed: Cell::new(false),
            listeners: RefCell::new(Vec::new()),
            open_page: RefCell::new(None),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        bridge.follow();
        let weak = Rc::downgrade(&bridge);
        tags.set_on_user_tag(move |id, assigned| {
            if let Some(bridge) = weak.upgrade() {
                bridge.user_tag(id, assigned);
            }
        });
        bridge
    }

    /// The service (the Settings pages send it their commands).
    pub fn service(&self) -> &PluginService {
        &self.service
    }

    pub fn list(&self) -> Arc<Vec<PluginInfo>> {
        Arc::clone(&self.list.borrow())
    }

    /// `hook` hears every change of the list.
    pub fn listen(&self, hook: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(hook));
    }

    pub fn set_open_page(&self, open: impl Fn(&str) + 'static) {
        *self.open_page.borrow_mut() = Some(Box::new(open));
    }

    /// A file opened; `by_rustcut` = a queued job was loaded into the editor.
    pub fn video_opened(&self, path: &Path, by_rustcut: bool) {
        self.generation.set(self.generation.get() + 1);
        if let Some(tags) = self.tags.upgrade() {
            tags.forget_rating_mark();
        }
        let by = if by_rustcut { OpenedBy::Job } else { OpenedBy::User };
        let mut context = self.context();
        context.video = Some(path.to_path_buf());
        self.dispatch(Event::VideoOpened(by), context);
    }

    fn user_tag(&self, id: u32, assigned: bool) {
        let Some(tag) = self.tags.upgrade().and_then(|t| t.plugin_tag(id)) else {
            return;
        };
        let event = if assigned {
            Event::TagAssigned(tag)
        } else {
            Event::TagRemoved(tag)
        };
        self.dispatch(event, self.context());
    }

    /// What the editor holds now.
    fn context(&self) -> Context {
        let export = self.export.upgrade();
        let (rating, tags) = self.tags.upgrade().map(|t| t.plugin_choice()).unwrap_or_default();
        Context {
            video: export.as_ref().and_then(|e| e.source()),
            rating,
            tags,
            target: export.as_ref().and_then(|e| e.target_folder()),
        }
    }

    fn dispatch(&self, event: Event, context: Context) {
        // Before the first list arrives the service is still loading; it takes the event after that, so it is sent.
        if self.listed.get()
            && self
                .list
                .borrow()
                .iter()
                .all(|p| p.state != rustcut_core::plugins::PluginState::Ready)
        {
            return;
        }
        let generation = self.generation.get();
        let folders: Arc<std::collections::BTreeMap<u32, PathBuf>> =
            Arc::new(self.export.upgrade().map(|e| e.folder_paths()).unwrap_or_default());
        let (outcomes, mut answers) = tokio::sync::mpsc::unbounded_channel();
        if self
            .service
            .handle
            .send(PluginCommand::Dispatch {
                event,
                context,
                folders,
                outcomes,
            })
            .is_err()
        {
            return;
        }
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            while let Some(outcome) = answers.recv().await {
                with_bridge(&ui, move |bridge| {
                    if bridge.generation.get() == generation {
                        bridge.apply(std::slice::from_ref(&outcome));
                    } else {
                        tracing::debug!("[Plugins] {}: an answer about an earlier video; dropped", outcome.id);
                    }
                });
            }
        });
    }

    /// Follows the service's events on the runtime and hands them to the UI thread.
    fn follow(&self) {
        let mut events = self.service.events.subscribe();
        let handle = self.service.handle.clone();
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(PluginEvent::Changed(list)) => with_bridge(&ui, move |b| b.changed(list)),
                    Ok(PluginEvent::Failed(failure)) => with_bridge(&ui, move |_| failed(&failure)),
                    Err(RecvError::Lagged(_)) => {
                        if let Ok(list) = handle.request(PluginCommand::Snapshot).await {
                            with_bridge(&ui, move |b| b.changed(Arc::new(list)));
                        }
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    fn changed(&self, list: Arc<Vec<PluginInfo>>) {
        self.list.replace(list);
        self.listed.set(true);
        for listener in self.listeners.borrow().iter() {
            listener();
        }
    }
}

/// A plugin failed: a banner with the way to its page; a pause says so instead.
fn failed(failure: &Failure) {
    let (kind, title) = if failure.paused {
        (
            Kind::PluginPaused,
            format!(
                "{} paused after {} failures",
                failure.name,
                rustcut_core::plugins::PAUSE_AFTER
            ),
        )
    } else {
        (Kind::PluginFailed, format!("{} failed", failure.name))
    };
    let id = failure.id.clone();
    notices::post(
        Notice::new(kind, title)
            .detail(failure.error.clone())
            .key(format!("plugin-failed:{}", failure.id))
            .action("Open plugin", move || show_settings(&id)),
    );
}
