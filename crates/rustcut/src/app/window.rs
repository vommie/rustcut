//! The main window's platform side: the Slint backend (winit + OpenGL), the window class, the geometry that is restored
//! at startup and captured at quit, and the window title.

use std::path::Path;

use az::CheckedAs as _;
use rustcut_core::app_identity::{APP_ID, APP_NAME, BINARY_NAME};
use rustcut_core::settings::WindowState;
use slint::ComponentHandle as _;
use slint::winit_030::winit::window::WindowAttributes;

use crate::MainWindow;

/// Selects winit with an OpenGL renderer: the player draws libmpv's frames into a GL texture on Slint's context (ADR-2).
/// Without OpenGL the default backend is used, so the window still opens (the player then reports that it cannot render).
pub fn select_backend() -> Result<(), slint::PlatformError> {
    match slint::BackendSelector::new()
        .backend_name("winit".into())
        .require_opengl()
        .with_winit_window_attributes_hook(with_window_class)
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

/// The window class: X11 `WM_CLASS` = ("rustcut", app id), Wayland app id = app id. The desktop file and the task manager
/// match on it. (Slint's `set_xdg_app_id` would set an empty X11 instance, so it is not used.)
fn with_window_class(attributes: WindowAttributes) -> WindowAttributes {
    use slint::winit_030::winit::platform::wayland::WindowAttributesExtWayland;
    use slint::winit_030::winit::platform::x11::WindowAttributesExtX11;
    let attributes = WindowAttributesExtX11::with_name(attributes, APP_ID, BINARY_NAME);
    WindowAttributesExtWayland::with_name(attributes, APP_ID, BINARY_NAME)
}

/// Size, position and maximized state before the first frame. The window manager may still place the window elsewhere.
pub fn restore(window: &MainWindow, state: &WindowState) {
    let w = window.window();
    tracing::debug!(
        width = state.width,
        height = state.height,
        x = ?state.x,
        y = ?state.y,
        maximized = state.maximized,
        "[App] Window restored"
    );
    w.set_size(slint::LogicalSize::new(state.width as f32, state.height as f32));
    if let (Some(x), Some(y)) = (state.x, state.y) {
        w.set_position(slint::PhysicalPosition::new(x, y));
    }
    if state.maximized {
        w.set_maximized(true);
    }
}

/// The window's geometry while it is still shown (at quit, before the event loop ends). A maximized or minimized window
/// keeps its last normal size and position.
pub fn capture(window: &MainWindow, state: &mut WindowState) {
    let w = window.window();
    let maximized = w.is_maximized();
    state.maximized = maximized;
    if maximized || w.is_minimized() || w.is_fullscreen() {
        return;
    }
    let size = w.size().to_logical(w.scale_factor());
    state.width = to_dimension(size.width, state.width);
    state.height = to_dimension(size.height, state.height);
    let position = w.position();
    state.x = Some(position.x);
    state.y = Some(position.y);
}

/// A logical size as whole pixels; `fallback` for values that are not a sensible size.
fn to_dimension(value: f32, fallback: u32) -> u32 {
    value
        .round()
        .checked_as::<u32>()
        .filter(|px| (1..=65_535).contains(px))
        .unwrap_or(fallback)
}

/// "<file> (<folder>) - RustCut", or the app name without a file (PyCutRv's title).
pub fn title(file: Option<&Path>) -> String {
    let Some(file) = file else {
        return APP_NAME.to_owned();
    };
    let name = file
        .file_name()
        .map_or_else(|| file.to_string_lossy(), |n| n.to_string_lossy());
    match file.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(folder) => format!("{name} ({}) - {APP_NAME}", folder.display()),
        None => format!("{name} - {APP_NAME}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_the_file_and_its_folder() {
        assert_eq!(title(None), "RustCut");
        assert_eq!(
            title(Some(Path::new("/media/Ärger über Größe [test] (1).mp4"))),
            "Ärger über Größe [test] (1).mp4 (/media) - RustCut"
        );
        assert_eq!(title(Some(Path::new("clip.mkv"))), "clip.mkv - RustCut");
        assert_eq!(title(Some(Path::new("/clip.mkv"))), "clip.mkv (/) - RustCut");
    }

    #[test]
    fn odd_sizes_fall_back() {
        assert_eq!(to_dimension(1279.6, 5), 1280);
        assert_eq!(to_dimension(-3.0, 5), 5);
        assert_eq!(to_dimension(f32::NAN, 5), 5);
        assert_eq!(to_dimension(1e9, 5), 5);
    }
}
