//! Startup and shutdown of the application: command line, logging, Slint backend, main window, event loop.

mod cli;
mod logging;
mod persistence;

use std::process::ExitCode;

use az::CheckedAs as _;
use clap::Parser as _;
use rustcut_core::app_identity::{APP_ID, APP_NAME, VERSION};
use rustcut_core::settings::{AppPaths, UiState, WindowState};
use slint::ComponentHandle as _;

pub use cli::Cli;

use crate::MainWindow;
use crate::ui::player::{PlayerBridge, PlayerBridgeError};

/// Errors that end the application before or while the event loop runs.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("the window system could not be initialized: {0}")]
    Platform(#[from] slint::PlatformError),
    #[error("no home folder is known; start with --home <dir>")]
    NoHome,
    #[error("the player could not be started: {0}")]
    Player(#[from] PlayerBridgeError),
}

/// Runs the application and returns the process exit code.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let log = logging::init();
    tracing::info!(version = VERSION, "[App] {APP_NAME} starting");
    match run(&cli, &log) {
        Ok(()) => {
            tracing::info!("[App] Exited normally");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "[App] Exited with an error");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, log: &logging::LogControl) -> Result<(), AppError> {
    if let Some(home) = &cli.home {
        tracing::info!(home = %home.display(), "[App] Isolated profile requested");
    }
    let paths = AppPaths::resolve(cli.home.as_deref()).ok_or(AppError::NoHome)?;
    let loaded = persistence::load_all(paths);
    log.apply_setting(&loaded.settings.advanced.log_level);
    select_backend()?;
    let window = MainWindow::new()?;
    // The XDG app id must be set after the backend exists and before the window is mapped (taskbar grouping, desktop file).
    if let Err(e) = slint::set_xdg_app_id(APP_ID) {
        tracing::warn!(error = %e, "[App] Failed to register the XDG app id");
    }
    window.set_app_name(APP_NAME.into());
    window.set_app_version(VERSION.into());
    restore_window(&window, &loaded.ui_state.window);
    let player = PlayerBridge::start(&window)?;
    if let Some(first) = cli.files.first() {
        player.open(first);
    }
    if cli.files.len() > 1 {
        tracing::info!(
            count = cli.files.len(),
            "[App] Only the first file on the command line is opened"
        );
    }
    window.run()?;

    let mut ui_state = loaded.ui_state.clone();
    capture_window(&window, &mut ui_state);
    persistence::save_ui_state(&loaded, &ui_state);
    drop(player);
    Ok(())
}

/// Size and maximized state before the first frame (the position is left to the window manager until M1.3).
fn restore_window(window: &MainWindow, state: &WindowState) {
    let w = window.window();
    tracing::debug!(
        width = state.width,
        height = state.height,
        maximized = state.maximized,
        "[App] Window restored"
    );
    w.set_size(slint::LogicalSize::new(state.width as f32, state.height as f32));
    if state.maximized {
        w.set_maximized(true);
    }
}

/// The window's size in logical pixels; a maximized window keeps its last normal size.
fn capture_window(window: &MainWindow, state: &mut UiState) {
    let w = window.window();
    let maximized = w.is_maximized();
    state.window.maximized = maximized;
    if !maximized {
        let size = w.size().to_logical(w.scale_factor());
        state.window.width = to_dimension(size.width, state.window.width);
        state.window.height = to_dimension(size.height, state.window.height);
    }
}

/// A logical size as whole pixels; `fallback` for values that are not a sensible size.
fn to_dimension(value: f32, fallback: u32) -> u32 {
    value
        .round()
        .checked_as::<u32>()
        .filter(|px| (1..=65_535).contains(px))
        .unwrap_or(fallback)
}

/// Selects winit with an OpenGL renderer: the player draws libmpv's frames into a GL texture on Slint's context (ADR-2).
/// Without OpenGL the default backend is used, so the window still opens (the player then reports that it cannot render).
fn select_backend() -> Result<(), slint::PlatformError> {
    match slint::BackendSelector::new()
        .backend_name("winit".into())
        .require_opengl()
        .select()
    {
        Ok(()) => {
            tracing::info!("[App] Backend winit with OpenGL selected");
            Ok(())
        }
        Err(e) => {
            tracing::warn!(error = %e, "[App] OpenGL is not available; falling back to the default backend");
            slint::BackendSelector::new().select()
        }
    }
}
