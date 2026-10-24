//! Queue progress on RustCut's taskbar entry: the Unity `LauncherEntry` signal on the session bus, which KDE Plasma's task
//! manager (and others) show as a bar over the entry. The entry is found by the installed desktop file's id.

use std::collections::HashMap;

use zbus::Connection;
use zbus::zvariant::Value;

use rustcut_core::app_identity::APP_ID;

const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";
const PATH: &str = "/io/github/vommie/rustcut/launcher";

/// What the taskbar entry shows: a fraction of the current run (0 to 1) and the number of open jobs, or nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Badge {
    pub progress: Option<f64>,
    pub count: Option<u32>,
}

impl Badge {
    pub const NONE: Self = Self {
        progress: None,
        count: None,
    };
}

/// `application://io.github.vommie.rustcut.desktop`
fn app_uri() -> String {
    format!("application://{APP_ID}.desktop")
}

/// The signal's properties for `badge`.
fn properties(badge: Badge) -> HashMap<&'static str, Value<'static>> {
    HashMap::from([
        ("progress", Value::from(badge.progress.unwrap_or(0.0).clamp(0.0, 1.0))),
        ("progress-visible", Value::from(badge.progress.is_some())),
        ("count", Value::from(i64::from(badge.count.unwrap_or(0)))),
        ("count-visible", Value::from(badge.count.is_some_and(|c| c > 0))),
    ])
}

/// Sends the entry's new state.
pub async fn show(connection: &Connection, badge: Badge) -> zbus::Result<()> {
    connection
        .emit_signal(None::<()>, PATH, INTERFACE, "Update", &(app_uri(), properties(badge)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signal_names_the_desktop_file_and_hides_what_is_unknown() {
        assert_eq!(app_uri(), "application://io.github.vommie.rustcut.desktop");
        let shown = properties(Badge {
            progress: Some(1.4),
            count: Some(3),
        });
        assert_eq!(shown.get("progress"), Some(&Value::from(1.0)));
        assert_eq!(shown.get("count-visible"), Some(&Value::from(true)));
        let hidden = properties(Badge::NONE);
        assert_eq!(hidden.get("progress-visible"), Some(&Value::from(false)));
        assert_eq!(hidden.get("count-visible"), Some(&Value::from(false)));
    }
}
