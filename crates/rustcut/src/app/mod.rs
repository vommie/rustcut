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
use rustcut_core::settings::AppPaths;
use slint::ComponentHandle as _;

pub use cli::Cli;

use crate::MainWindow;
use crate::desktop::instance::{self, Request};
use crate::ui::dispatcher::{SlintSink, UiDispatcher};
use crate::ui::dock::{DockBridge, persist as dock_persist};
use crate::ui::editor::Editor;
use crate::ui::filters::FiltersBridge;
use crate::ui::media_info::MediaInfoBridge;
use crate::ui::open::{self, FileOpener};
use crate::ui::player::{PlayerBridge, PlayerBridgeError};

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
    let opener = FileOpener::install(
        &main,
        &player,
        &media,
        &editor,
        &filters,
        runtime.handle().clone(),
        loaded.ui_state.last_open_dir.clone(),
    );
    let server = single_instance.then(|| listen(&socket, &runtime, &ui, &main)).flatten();
    shutdown::watch_signals(runtime.handle(), ui, main.as_weak());

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
    persistence::save_ui_state(&loaded, &ui_state);
    drop(server);
    drop(opener);
    drop(editor);
    drop(filters);
    drop(media);
    drop(main);
    stop_hashing(&hashing);
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
