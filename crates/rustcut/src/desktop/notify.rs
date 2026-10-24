//! Desktop notifications (the freedesktop.org Notifications service, KDE's notification daemon), as in RustStart: named
//! icons from the icon theme, the desktop entry (Plasma groups them under RustCut), an optional action button. A
//! notification is sent from a blocking thread of the runtime; the UI never waits for the daemon.

use notify_rust::{Hint, Notification, Urgency};
use rustcut_core::app_identity::{APP_ID, APP_NAME};

/// What a notification is about; decides its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Done,
    Failed,
    Power,
}

impl Style {
    const fn icon(self) -> &'static str {
        match self {
            Self::Done => "dialog-ok-apply",
            Self::Failed => "dialog-error",
            Self::Power => "system-shutdown",
        }
    }
}

/// One notification; `timeout_secs` 0 = until dismissed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub style: Style,
    pub title: String,
    pub body: String,
    pub timeout_secs: u32,
    /// The text of the action button (`None` = no button).
    pub action: Option<String>,
}

/// The notification as it goes over D-Bus.
pub fn build(note: &Note) -> Notification {
    let mut n = Notification::new();
    n.appname(APP_NAME)
        .summary(&note.title)
        .body(&note.body)
        .icon(note.style.icon())
        .urgency(if note.style == Style::Failed && note.timeout_secs == 0 {
            Urgency::Critical
        } else {
            Urgency::Normal
        })
        .timeout(i32::try_from(note.timeout_secs.saturating_mul(1000)).unwrap_or(i32::MAX))
        .hint(Hint::DesktopEntry(APP_ID.to_owned()));
    if let Some(label) = &note.action {
        n.action("default", label);
    }
    n
}

/// Shows `note`; when its button is clicked, `on_action` runs (on the runtime's blocking thread).
pub fn send(runtime: &tokio::runtime::Handle, note: Note, on_action: Option<Box<dyn FnOnce() + Send>>) {
    runtime.spawn_blocking(move || match build(&note).show() {
        Ok(handle) => {
            if let Some(action) = on_action {
                let mut action = Some(action);
                handle.wait_for_action(|clicked| {
                    if clicked == "default"
                        && let Some(action) = action.take()
                    {
                        action();
                    }
                });
            }
        }
        Err(e) => tracing::warn!(error = %e, "[Desktop] The notification could not be shown"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_carry_the_entry_icon_and_button() {
        let note = Note {
            style: Style::Power,
            title: "Suspending in 5 s".into(),
            body: "The queue is done.".into(),
            timeout_secs: 5,
            action: Some("Abort".into()),
        };
        let n = build(&note);
        assert_eq!(n.summary, "Suspending in 5 s");
        assert_eq!(n.icon, "system-shutdown");
        assert_eq!(n.actions, vec!["default".to_owned(), "Abort".to_owned()]);
        assert_eq!(n.appname, "RustCut");
    }
}
