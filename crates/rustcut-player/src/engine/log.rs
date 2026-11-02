//! Forwards mpv's own log messages into RustCut's log (libmpv2 does not wrap `mpv_request_log_messages`).
#![allow(unsafe_code, reason = "one libmpv call that libmpv2 does not wrap")]

use std::ffi::CString;

use libmpv2::Mpv;

/// Asks mpv to send its log messages of `min_level` and above to `client`'s event queue.
pub(super) fn request_log_messages(client: &Mpv, min_level: &str) {
    let Ok(level) = CString::new(min_level) else {
        tracing::warn!(level = min_level, "[Player] Invalid mpv log level");
        return;
    };
    // SAFETY: `client.ctx` is a live mpv handle owned by `client` for the duration of this call; `level` is a NUL-terminated
    // string that outlives the call (mpv copies it). The function is documented as callable from any thread.
    let code = unsafe { libmpv2_sys::mpv_request_log_messages(client.ctx.as_ptr(), level.as_ptr()) };
    if code < 0 {
        tracing::warn!(code, level = min_level, "[Player] mpv refused the log level");
    }
}

/// Writes one mpv log message with the matching tracing level.
pub(super) fn forward(prefix: &str, level: libmpv2::LogLevel, text: &str) {
    use libmpv2::mpv_log_level as l;
    let text = text.trim_end();
    match level {
        l::Fatal | l::Error => tracing::error!(module = prefix, "[Player] mpv: {text}"),
        l::Warn => tracing::warn!(module = prefix, "[Player] mpv: {text}"),
        l::Info => tracing::info!(module = prefix, "[Player] mpv: {text}"),
        l::V | l::Debug => tracing::debug!(module = prefix, "[Player] mpv: {text}"),
        _ => tracing::trace!(module = prefix, "[Player] mpv: {text}"),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    #[test]
    fn a_client_keeps_its_name() {
        // libmpv2 6.0.0 freed the name before mpv read it, so mpv took a freed block for the name: mostly noise, but about
        // one start in 200 an empty string, for which `mpv_create_client` returns null and the app died (fixed in 6.0.1).
        let core = libmpv2::Mpv::with_initializer(|_| Ok(())).unwrap();
        let client = core.create_client(Some("rustcut_events")).unwrap();
        // SAFETY: `client.ctx` is a live handle.
        let name = unsafe { libmpv2_sys::mpv_client_name(client.ctx.as_ptr()) };
        // SAFETY: the name is NUL-terminated and stays valid while `client` lives (`client.h`).
        let name = unsafe { CStr::from_ptr(name) };
        assert_eq!(name.to_str(), Ok("rustcut_events"));
    }
}
