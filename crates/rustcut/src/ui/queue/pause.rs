//! The queue's pause outlives a crash (user decision Q-P8b): every change of it is handed to the app right away
//! (`QueueHooks::paused_changed`, which writes `ui-state.toml`), and a queue that starts paused while jobs wait says so
//! once with a button that resumes it (kind `queue_paused`: a banner by default, or a toast, the desktop or nothing).

use rustcut_core::job::JobStateKind;
use rustcut_core::queue::QueueCommand;

use super::QueueBridge;
use crate::ui::notices::{self, Kind, Notice};

impl QueueBridge {
    /// After a refresh: what the pause is now (`None` before the queue's first snapshot) and how many jobs wait.
    pub(super) fn pause_seen(&self, paused: Option<bool>, waiting: usize) {
        let Some(paused) = paused else { return };
        match self.last_paused.replace(Some(paused)) {
            None => {
                if paused && waiting > 0 {
                    self.tell_paused_at_start(waiting);
                }
            }
            Some(before) if before != paused => {
                tracing::info!(paused, "[Queue] Pause changed; saved at once");
                if let Some(hook) = self.hooks.borrow().paused_changed.as_ref() {
                    hook(paused);
                }
                if !paused {
                    notices::clear_kind(Kind::QueuePaused);
                }
            }
            Some(_) => {}
        }
    }

    fn tell_paused_at_start(&self, waiting: usize) {
        let jobs = if waiting == 1 {
            "1 job waits".to_owned()
        } else {
            format!("{waiting} jobs wait")
        };
        let why = if self.paused_before {
            "It was paused when RustCut ended"
        } else {
            "Jobs were waiting at the start (\"Start paused\" in the settings)"
        };
        tracing::info!(waiting, remembered = self.paused_before, "[Queue] Paused at the start");
        let handle = self.handle.clone();
        notices::post(
            Notice::new(Kind::QueuePaused, "The queue is paused")
                .detail(format!("{why}. {jobs}."))
                .action("Resume", move || {
                    tracing::info!("[Queue] Resumed from the start message");
                    if handle.send(QueueCommand::SetPaused(false)).is_err() {
                        tracing::warn!("[Queue] The queue engine has stopped");
                    }
                }),
        );
    }
}

/// How many jobs of `rows` wait.
pub(super) fn waiting(rows: &[rustcut_core::queue::JobSummary]) -> usize {
    rows.iter().filter(|r| r.state.kind() == JobStateKind::Waiting).count()
}
