//! The services behind the window: the runtime, the single-instance socket, the queue database, the queue engine, and
//! stopping them at quit in order.

use std::rc::Rc;
use std::time::Instant;

use rustcut_core::hashing::{HashCommand, HashService};
use rustcut_core::queue::{QueueCommand, QueueOptions, QueueService, SyncLink, spawn_queue_linked};
use rustcut_core::render::{HardwareContext, RenderEnv};
use rustcut_core::rustview::{SyncCommand, SyncService};
use rustcut_core::settings::{AppPaths, Settings};
use rustcut_core::store::{Problem, Store, StoreCommand, StoreHandle, spawn_store};

use super::{AppError, Cli, persistence};
use crate::MainWindow;
use crate::desktop::instance::{self, Request};
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::open;
use crate::ui::player::PlayerBridge;
use crate::ui::problems::StoreProblem;

/// The tokio runtime of the services (ADR-4). Built before the window: it takes about a millisecond (logged), and mpv's
/// first events need the dispatcher, whose coalescing runs on it.
pub(super) fn start_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("rustcut-rt")
        .build()
        .map_err(AppError::Runtime)?;
    tracing::debug!(micros = started.elapsed().as_micros(), "[App] Runtime started");
    Ok(runtime)
}

/// Hands the command line's files to a running instance of this profile. `true` = it took them, this start ends.
pub(super) fn forward(socket: &std::path::Path, cli: &Cli) -> bool {
    let request = Request::Open {
        files: instance::absolute_files(&cli.files),
    };
    match instance::send(socket, &request) {
        Ok(Some(reply)) if reply.ok => {
            tracing::info!(
                files = cli.files.len(),
                "[Instance] RustCut is already running; files forwarded"
            );
            true
        }
        Ok(Some(reply)) => {
            tracing::warn!(message = %reply.message, "[Instance] The running instance refused the files; starting anyway");
            false
        }
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(error = %e, "[Instance] The running instance did not answer; starting anyway");
            false
        }
    }
}

/// Listens for a second start; its files open in this window. `None` (logged) when the socket cannot be used.
pub(super) fn listen(
    socket: &std::path::Path,
    runtime: &tokio::runtime::Handle,
    ui: &UiDispatcher,
    main: &slint::Weak<MainWindow>,
) -> Option<instance::Server> {
    let ui = ui.clone();
    let main = main.clone();
    let served = instance::serve(socket, runtime, move |request| {
        if let Request::Open { files } = request {
            let main = main.clone();
            ui.post(move || {
                if let Some(main) = main.upgrade() {
                    open::open_forwarded(&main, &files);
                }
            });
        }
    });
    match served {
        Ok(server) => Some(server),
        Err(e) => {
            tracing::warn!(error = %e, "[Instance] Not listening for a second start; it opens its own window");
            None
        }
    }
}

/// Opens the queue database and starts its thread. A database that cannot be used (newer, damaged, no access) is left
/// alone: the queue runs on a database in memory for this run, and the returned text says so in the status line.
pub(super) fn open_store(paths: &AppPaths) -> (StoreHandle, Option<std::thread::JoinHandle<()>>, Option<StoreProblem>) {
    let (store, problem) = match Store::open(&paths.database(), &paths.data_backups()) {
        Ok(store) => (store, None),
        Err(e) => {
            tracing::error!(error = %e, "[Store] The queue database cannot be used; jobs are kept in memory until a way out is chosen");
            let problem = StoreProblem {
                problem: Problem::of(&e),
                error: e.to_string(),
            };
            match Store::open_in_memory() {
                Ok(store) => (store, Some(problem)),
                Err(e) => {
                    tracing::error!(error = %e, "[Store] Not even a database in memory could be opened");
                    return (StoreHandle::closed(), None, Some(problem));
                }
            }
        }
    };
    match spawn_store(store) {
        Ok((handle, thread)) => (handle, Some(thread), problem),
        Err(e) => {
            tracing::error!(error = %e, "[Store] The database thread could not be started");
            (
                StoreHandle::closed(),
                None,
                Some(StoreProblem {
                    problem: Problem::NoAccess,
                    error: format!("the database thread could not be started: {e}"),
                }),
            )
        }
    }
}

/// Starts the queue engine with the settings and the panel's last processing state; once the capability check knows
/// the hardware (a VAAPI device), the renders get it.
pub(super) fn start_queue(
    runtime: &tokio::runtime::Runtime,
    store: &StoreHandle,
    loaded: &persistence::Loaded,
    encoders: &tokio::sync::watch::Receiver<Option<std::sync::Arc<rustcut_core::render::Capabilities>>>,
    sync: SyncLink,
) -> QueueService {
    let settings: &Settings = &loaded.settings;
    let options = QueueOptions {
        workers: settings.queue.workers,
        paused: loaded.ui_state.queue.paused,
        pause_on_start: settings.queue.pause_on_start,
        when_done: settings.queue.when_done,
    };
    let env = RenderEnv::new(settings, &loaded.paths, HardwareContext::default());
    let (service, _task) = spawn_queue_linked(runtime.handle(), store.clone(), env.clone(), options, Some(sync));
    let mut capabilities = encoders.clone();
    let handle = service.handle.clone();
    runtime.spawn(async move {
        while capabilities.changed().await.is_ok() {
            let hardware = capabilities.borrow_and_update().as_ref().map(|c| c.hardware.clone());
            if let Some(hardware) = hardware {
                if hardware != HardwareContext::default() {
                    let _ = handle.send(QueueCommand::SetRenderEnv(Box::new(RenderEnv { hardware, ..env })));
                }
                return;
            }
        }
    });
    service
}

/// Stops the queue: running renders end (they wait again at the next start); takes at most a few seconds.
pub(super) fn stop_queue(queue: &QueueService) {
    if queue
        .handle
        .blocking_request(|reply| QueueCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The queue had already stopped");
    }
}

/// Closes the database after the commands before it and joins its thread.
pub(super) fn stop_store(store: &StoreHandle, thread: Option<std::thread::JoinHandle<()>>) {
    if store
        .blocking_request(|reply| StoreCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The database had already stopped");
    }
    if let Some(thread) = thread
        && thread.join().is_err()
    {
        tracing::warn!("[App] The database thread ended with a panic");
    }
}

/// Stops the RustView sync: a running `rustview` call is dropped (its child killed); records it reported before are
/// stored by the queue, which stops after it. Unfinished work is picked up at the next start from the stored records.
pub(super) fn stop_sync(sync: &SyncService) {
    if sync
        .handle
        .blocking_request(|reply| SyncCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The RustView sync had already stopped");
    }
}

/// Stops the hash service: a running read ends at its next chunk (the service waits for it at most two seconds).
pub(super) fn stop_hashing(hashing: &HashService) {
    if hashing
        .handle
        .blocking_request(|reply| HashCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The hash service had already stopped");
    }
}

/// Drops the last reference to the player: mpv quits and its threads are joined.
pub(super) fn stop_player(player: Rc<PlayerBridge>) {
    match Rc::try_unwrap(player) {
        Ok(player) => drop(player),
        Err(player) => {
            tracing::warn!(
                references = Rc::strong_count(&player),
                "[App] The player is still referenced; it stops when the last reference goes"
            );
        }
    }
}
