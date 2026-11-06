//! The desktop's color scheme through the XDG desktop portal (`org.freedesktop.appearance` `color-scheme`, via `ashpd`):
//! read once at the start and followed while RustCut runs (RustView does the same). KDE Plasma answers it on X11 and on
//! Wayland. Without a portal the scheme stays unknown (dark then). It runs on the tokio runtime; the UI hears of it
//! through the event loop.

use futures_util::StreamExt as _;
use rustcut_core::settings::SystemScheme;

fn scheme(value: ashpd::desktop::settings::ColorScheme) -> SystemScheme {
    match value {
        ashpd::desktop::settings::ColorScheme::PreferDark => SystemScheme::Dark,
        ashpd::desktop::settings::ColorScheme::PreferLight => SystemScheme::Light,
        ashpd::desktop::settings::ColorScheme::NoPreference => SystemScheme::Unknown,
    }
}

async fn follow(send: impl Fn(SystemScheme)) -> Result<(), ashpd::Error> {
    let settings = ashpd::desktop::settings::Settings::new().await?;
    let first = settings.color_scheme().await?;
    tracing::info!(scheme = ?first, "[Theme] The desktop portal answered");
    send(scheme(first));
    let mut changes = settings.receive_color_scheme_changed().await?;
    while let Some(value) = changes.next().await {
        send(scheme(value));
    }
    Ok(())
}

/// Starts watching; `changed` runs on the UI thread with the first value and with every change.
pub fn watch(runtime: &tokio::runtime::Handle, changed: fn(SystemScheme)) {
    runtime.spawn(async move {
        let send = move |value: SystemScheme| {
            if let Err(e) = slint::invoke_from_event_loop(move || changed(value)) {
                tracing::debug!(error = %e, "[Theme] The event loop is gone");
            }
        };
        if let Err(e) = follow(send).await {
            tracing::info!(error = %e, "[Theme] No color scheme from the desktop portal; following the system stays dark");
        }
    });
}
