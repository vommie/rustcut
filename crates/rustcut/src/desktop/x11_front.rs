//! Brings a window to the front on X11 although another program has the focus (RustSearch's question, 0.0.120).
//! winit's `focus_window` is an application's request (`_NET_ACTIVE_WINDOW` with source 1), which KWin's focus stealing
//! prevention turns into "demands attention" while another program is in front. RustCut comes to the front only to ask
//! about a file the user just clicked in RustSearch, so it asks the way a task bar does (source 2, EWMH). On Wayland
//! nothing is sent (`false`): the caller keeps winit's own request.

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask};

/// EWMH source indication of a pager or task bar.
const SOURCE_PAGER: u32 = 2;

/// Asks the window manager to activate `window`; `true` when the request was sent.
pub fn to_front(window: &slint::Window) -> bool {
    let Some(xid) = super::x11_dialog::xid(window) else {
        return false;
    };
    let Ok((conn, screen)) = x11rb::connect(None) else {
        return false;
    };
    let Some(root) = conn.setup().roots.get(screen).map(|s| s.root) else {
        return false;
    };
    let Some(active) = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.atom)
    else {
        return false;
    };
    // Data: source, timestamp (0 = now), the requester's active window (none).
    let event = ClientMessageEvent::new(32, xid, active, [SOURCE_PAGER, 0, 0, 0, 0]);
    let sent = conn
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .is_ok()
        && conn.flush().is_ok();
    tracing::debug!(
        xid,
        sent,
        "[Desktop] Asked the window manager to bring the window to the front"
    );
    sent
}
