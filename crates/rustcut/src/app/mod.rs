//! Startup and shutdown of the application: command line, logging, Slint backend, main window, event loop.

mod cli;
mod logging;

use std::process::ExitCode;

use clap::Parser as _;
use rustcut_core::app_identity::{APP_ID, APP_NAME, VERSION};
use slint::ComponentHandle as _;

pub use cli::Cli;

use crate::MainWindow;

/// Errors that end the application before or while the event loop runs.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("the window system could not be initialized: {0}")]
    Platform(#[from] slint::PlatformError),
}

/// Runs the application and returns the process exit code.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    logging::init();
    tracing::info!(version = VERSION, "[App] {APP_NAME} starting");
    match run(&cli) {
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

fn run(cli: &Cli) -> Result<(), AppError> {
    if let Some(home) = &cli.home {
        tracing::info!(home = %home.display(), "[App] Isolated profile requested");
    }
    if !cli.files.is_empty() {
        tracing::info!(
            count = cli.files.len(),
            "[App] Files on the command line (opening arrives with the player)"
        );
    }

    select_backend()?;
    let window = MainWindow::new()?;
    // The XDG app id must be set after the backend exists and before the window is mapped (taskbar grouping, desktop file).
    if let Err(e) = slint::set_xdg_app_id(APP_ID) {
        tracing::warn!(error = %e, "[App] Failed to register the XDG app id");
    }
    window.set_app_name(APP_NAME.into());
    window.set_app_version(VERSION.into());
    window.run()?;
    Ok(())
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
