//! A dialog window belongs to the window it was opened from (taken over from RustView's `viewer/x11/dialog_hints.rs`
//! with the color picker, P8): ICCCM `WM_TRANSIENT_FOR` names that window and EWMH `_NET_WM_WINDOW_TYPE_DIALOG` says
//! what it is. The window manager then keeps the dialog above its parent (and only above it, not above other programs)
//! and places it on the parent. winit 0.30 has no such hint, so the properties are written here (`x11rb`); on Wayland
//! nothing is written and the window is an ordinary one.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// The X11 window of a Slint window (`None` on Wayland or before the window exists).
pub fn xid(window: &slint::Window) -> Option<u32> {
    let handle = window.window_handle();
    let raw = handle.window_handle().ok()?.as_raw();
    let xid = match raw {
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok()?,
        RawWindowHandle::Xcb(h) => h.window.get(),
        _ => return None,
    };
    (xid != 0).then_some(xid)
}

/// Makes `dialog` a dialog of `parent`. The window manager reads these properties when the dialog is mapped, so they are
/// written before it is shown (a hidden dialog is managed anew when it is shown again). Waits until the X server has
/// them, so winit's map request on its own connection cannot overtake them. Returns whether they were written.
pub fn set_parent(dialog: u32, parent: u32) -> bool {
    let Ok((conn, _screen)) = x11rb::connect(None) else {
        tracing::debug!("[Desktop] No X11 connection; the dialog has no parent window");
        return false;
    };
    let atom = |name: &str| {
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    let (Some(window_type), Some(dialog_type)) = (atom("_NET_WM_WINDOW_TYPE"), atom("_NET_WM_WINDOW_TYPE_DIALOG"))
    else {
        return false;
    };
    let written = conn
        .change_property32(
            PropMode::REPLACE,
            dialog,
            AtomEnum::WM_TRANSIENT_FOR,
            AtomEnum::WINDOW,
            &[parent],
        )
        .is_ok()
        && conn
            .change_property32(PropMode::REPLACE, dialog, window_type, AtomEnum::ATOM, &[dialog_type])
            .is_ok();
    // A round trip: the properties are there before this returns.
    let synced = conn.get_input_focus().ok().and_then(|c| c.reply().ok()).is_some();
    let _ = conn.flush();
    tracing::debug!(
        dialog,
        parent,
        written,
        synced,
        "[Desktop] Dialog parent written before the dialog is mapped"
    );
    written && synced
}

/// Makes `dialog` a dialog of `parent` as soon as Slint has made its window, before it is mapped the first time (a
/// rendering notifier; later showings find the properties in place). For a dialog whose parent never changes (the tag
/// filter window); the color picker, opened from several windows, writes its parent per opening.
pub fn parent_when_set_up<D, P>(dialog: &D, parent: &P)
where
    D: slint::ComponentHandle + 'static,
    P: slint::ComponentHandle + 'static,
{
    let (dialog_weak, parent_weak) = (dialog.as_weak(), parent.as_weak());
    let installed = dialog.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::RenderingSetup)
            && let (Some(dialog), Some(parent)) = (dialog_weak.upgrade(), parent_weak.upgrade())
            && let (Some(dialog), Some(parent)) = (xid(dialog.window()), xid(parent.window()))
        {
            set_parent(dialog, parent);
        }
    });
    if let Err(e) = installed {
        tracing::warn!(error = ?e, "[Desktop] No rendering notifier: the dialog has no parent window");
    }
}

/// Puts `window` in the middle of `parent` (physical pixels, as both report them; Wayland compositors place windows
/// themselves).
pub fn center_over(window: &slint::Window, parent: &slint::Window) {
    use az::SaturatingAs as _;
    let (parent_pos, parent_size) = (parent.position(), parent.size());
    let size = window.size();
    let x = parent_pos.x + (parent_size.width.saturating_as::<i32>() - size.width.saturating_as::<i32>()) / 2;
    let y = parent_pos.y + (parent_size.height.saturating_as::<i32>() - size.height.saturating_as::<i32>()) / 2;
    window.set_position(slint::PhysicalPosition::new(x, y));
}
