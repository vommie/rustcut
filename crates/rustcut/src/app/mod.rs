//! Startup and shutdown of the application: command line, profile, logging, single instance, runtime, Slint backend, main
//! window, event loop, and the one quit path (`shutdown`).

mod cli;
mod logging;
mod persistence;
mod shutdown;
pub mod window;

use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser as _;
use rustcut_core::app_identity::{APP_NAME, VERSION};
use rustcut_core::hashing::{HashCommand, HashService, spawn_hash_service};
use rustcut_core::queue::{QueueCommand, QueueOptions, QueueService, spawn_queue};
use rustcut_core::render::{HardwareContext, RenderEnv, probe_dir, spawn_capability_check};
use rustcut_core::settings::{AppPaths, Settings};
use rustcut_core::store::{Store, StoreCommand, StoreHandle, spawn_store};
use slint::ComponentHandle as _;

pub use cli::Cli;
pub use persistence::SettingsWriter;

use crate::MainWindow;
use crate::desktop::instance::{self, Request};
use crate::ui::dispatcher::{SlintSink, UiDispatcher};
use crate::ui::dock::{DockBridge, persist as dock_persist};
use crate::ui::editor::Editor;
use crate::ui::export::{ExportBridge, ExportParts};
use crate::ui::filters::FiltersBridge;
use crate::ui::keymap::Action;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::open::{self, FileOpener, OpenerParts};
use crate::ui::player::{PlayerBridge, PlayerBridgeError};
use crate::ui::probe::{self, ProbeBridge};
use crate::ui::queue::QueueBridge;
use crate::ui::render_settings::RenderSettingsBridge;
use crate::ui::status::{self, Level};

/// How long the runtime may take to finish its tasks at quit.
const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(2);

/// Errors that end the application before or while the event loop runs.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("the window system could not be initialized: {0}")]
    Platform(#[from] slint::PlatformError),
    #[error("no home folder is known; start with --home <dir>")]
    NoHome,
    #[error("the async runtime could not be started: {0}")]
    Runtime(std::io::Error),
    #[error("the player could not be started: {0}")]
    Player(#[from] PlayerBridgeError),
}

/// How a run ended.
enum Outcome {
    Quit,
    /// A running instance of the same profile took the files.
    Forwarded,
}

/// Runs the application and returns the process exit code.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let log = logging::init();
    tracing::info!(version = VERSION, "[App] {APP_NAME} starting");
    let result = run(&cli, &log);
    let code = match result {
        Ok(Outcome::Quit) => {
            tracing::info!("[App] Exited normally");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Forwarded) => {
            tracing::info!("[App] Handed over to the running instance");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "[App] Exited with an error");
            ExitCode::FAILURE
        }
    };
    // Flushes the log file.
    drop(log);
    code
}

fn run(cli: &Cli, log: &logging::LogControl) -> Result<Outcome, AppError> {
    if let Some(home) = &cli.home {
        tracing::info!(home = %home.display(), "[App] Isolated profile requested");
    }
    let paths = AppPaths::resolve(cli.home.as_deref()).ok_or(AppError::NoHome)?;
    let loaded = persistence::load_all(paths);
    log.apply_setting(&loaded.settings.advanced.log_level);
    log.open_file(&loaded.paths.log_dir(), loaded.settings.advanced.log_keep_days);

    let single_instance = loaded.settings.general.single_instance;
    let socket = instance::socket_path(&loaded.paths.config_dir);
    if single_instance && forward(&socket, cli) {
        return Ok(Outcome::Forwarded);
    }

    let runtime = start_runtime()?;
    let ui = UiDispatcher::new(SlintSink, runtime.handle().clone());
    window::select_backend()?;
    let main = MainWindow::new()?;
    main.set_app_name(APP_NAME.into());
    main.set_app_version(VERSION.into());
    window::restore(&main, &loaded.ui_state.window);
    let quit = shutdown::QuitPath::install(&main, &loaded.ui_state.window);
    let dock = DockBridge::install(&main, dock_persist::from_table(loaded.ui_state.dock.as_ref()));
    let player = PlayerBridge::start(&main, &ui, &loaded.settings.player, &loaded.ui_state.export)?;
    let (hashing, _hash_task) = spawn_hash_service(runtime.handle());
    // Which encoders work here (ADR-14); the Render Settings panel offers only those.
    let encoders = spawn_capability_check(runtime.handle(), loaded.settings.advanced.ffmpeg.clone());
    let writer = SettingsWriter::start(
        runtime.handle(),
        loaded.paths.config_dir.clone(),
        loaded.settings_read_only,
        &loaded.settings,
    );
    let (store, store_thread, store_problem) = open_store(&loaded.paths);
    let queue_service = start_queue(&runtime, &store, &loaded, &encoders);
    let queue = QueueBridge::install(&main, &ui, runtime.handle(), &queue_service);
    let render = RenderSettingsBridge::install(
        &main,
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
        writer.clone(),
        &loaded.paths.cache_dir,
        encoders,
    );
    let media = MediaInfoBridge::install(&main, &ui, runtime.handle().clone(), hashing.clone(), &loaded.settings);
    let filters = FiltersBridge::install(
        &main,
        &player,
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
    );
    let editor = Editor::install(
        &main,
        &player,
        &media,
        &filters,
        &loaded.settings,
        &loaded.ui_state.timeline,
    );
    let probe = ProbeBridge::install(
        &main,
        runtime.handle().clone(),
        &probe::Parts {
            player: &player,
            editor: &editor,
            filters: &filters,
            render: &render,
        },
        RenderEnv::new(&loaded.settings, &loaded.paths, HardwareContext::default()),
        probe_dir(&loaded.paths.cache_dir),
        loaded.ui_state.export.probe,
        &editor.key_label(Action::ProbeRender),
    );
    editor.set_probe(&probe);
    let export = ExportBridge::install(
        &main,
        &ExportParts {
            player: &player,
            editor: &editor,
            filters: &filters,
            media: &media,
            render: &render,
            queue: &queue,
        },
        runtime.handle().clone(),
        &loaded.settings,
        &loaded.ui_state.export,
        writer,
    );
    editor.set_export(&export);
    let opener = FileOpener::install(
        &main,
        &OpenerParts {
            player: &player,
            media: &media,
            editor: &editor,
            filters: &filters,
            probe: &probe,
            export: &export,
        },
        runtime.handle().clone(),
        loaded.ui_state.last_open_dir.clone(),
    );
    let server = single_instance.then(|| listen(&socket, &runtime, &ui, &main)).flatten();
    shutdown::watch_signals(runtime.handle(), ui, main.as_weak());

    if let Some(problem) = store_problem {
        status::show(&main, Level::Warning, &problem);
    }
    if let Some(first) = cli.files.first() {
        opener.open(first.clone());
    }
    if cli.files.len() > 1 {
        tracing::info!(
            count = cli.files.len(),
            "[App] Only the first file on the command line is opened"
        );
    }
    main.run()?;

    // The single quit path: save what the user arranged, then stop the services.
    let mut ui_state = loaded.ui_state.clone();
    ui_state.window = quit.geometry(&main);
    ui_state.dock = dock_persist::to_table(&dock.layout());
    (ui_state.export.volume, ui_state.export.muted) = player.volume_and_mute();
    ui_state.last_open_dir = opener.last_open_dir();
    ui_state.timeline = editor.timeline_state();
    filters.save_into(&mut ui_state.export);
    render.save_into(&mut ui_state.export);
    probe.save_into(&mut ui_state.export);
    export.save_into(&mut ui_state.export);
    ui_state.queue.paused = queue.paused();
    persistence::save_ui_state(&loaded, &ui_state);
    drop(server);
    drop(opener);
    drop(export);
    drop(queue);
    drop(probe);
    drop(render);
    drop(editor);
    drop(filters);
    drop(media);
    drop(main);
    stop_hashing(&hashing);
    stop_queue(&queue_service);
    stop_store(&store, store_thread);
    stop_player(player);
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
    Ok(Outcome::Quit)
}

/// The tokio runtime of the services (ADR-4). Built before the window: it takes about a millisecond (logged), and mpv's
/// first events need the dispatcher, whose coalescing runs on it.
fn start_runtime() -> Result<tokio::runtime::Runtime, AppError> {
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
fn forward(socket: &std::path::Path, cli: &Cli) -> bool {
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
fn listen(
    socket: &std::path::Path,
    runtime: &tokio::runtime::Runtime,
    ui: &UiDispatcher,
    main: &MainWindow,
) -> Option<instance::Server> {
    let ui = ui.clone();
    let main = main.as_weak();
    let served = instance::serve(socket, runtime.handle(), move |request| {
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
fn open_store(paths: &AppPaths) -> (StoreHandle, Option<std::thread::JoinHandle<()>>, Option<String>) {
    let (store, problem) = match Store::open(&paths.database(), &paths.data_backups()) {
        Ok(store) => (store, None),
        Err(e) => {
            tracing::error!(error = %e, "[Store] The queue database cannot be used; jobs are kept in memory for this run");
            let problem = format!("The queue database cannot be used ({e}); jobs are kept only until RustCut quits.");
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
                Some(format!("The queue database thread could not be started: {e}")),
            )
        }
    }
}

/// Starts the queue engine with the settings and the panel's last processing state; once the capability check knows
/// the hardware (a VAAPI device), the renders get it.
fn start_queue(
    runtime: &tokio::runtime::Runtime,
    store: &StoreHandle,
    loaded: &persistence::Loaded,
    encoders: &tokio::sync::watch::Receiver<Option<std::sync::Arc<rustcut_core::render::Capabilities>>>,
) -> QueueService {
    let settings: &Settings = &loaded.settings;
    let options = QueueOptions {
        workers: settings.queue.workers,
        paused: loaded.ui_state.queue.paused,
        pause_on_start: settings.queue.pause_on_start,
        when_done: loaded.ui_state.queue.when_done.unwrap_or(settings.queue.when_done),
    };
    let env = RenderEnv::new(settings, &loaded.paths, HardwareContext::default());
    let (service, _task) = spawn_queue(runtime.handle(), store.clone(), env.clone(), options);
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
fn stop_queue(queue: &QueueService) {
    if queue
        .handle
        .blocking_request(|reply| QueueCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The queue had already stopped");
    }
}

/// Closes the database after the commands before it and joins its thread.
fn stop_store(store: &StoreHandle, thread: Option<std::thread::JoinHandle<()>>) {
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

/// Stops the hash service: a running read ends at its next chunk (the service waits for it at most two seconds).
fn stop_hashing(hashing: &HashService) {
    if hashing
        .handle
        .blocking_request(|reply| HashCommand::Shutdown { reply })
        .is_err()
    {
        tracing::debug!("[App] The hash service had already stopped");
    }
}

/// Drops the last reference to the player: mpv quits and its threads are joined.
fn stop_player(player: Rc<PlayerBridge>) {
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
