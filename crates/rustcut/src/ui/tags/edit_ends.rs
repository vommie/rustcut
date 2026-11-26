//! The ends of an edit of queued jobs' tags and rating besides Apply, Cancel, Escape and a loaded job (0.0.122): another
//! file opens, or the edited jobs leave the queue. The save bubble shows only while an edit runs, so it never waits for
//! an edit that can no longer be applied.

use rustcut_core::job::JobId;

use super::TagsBridge;
use crate::ui::notices::{Kind, Notice};

impl TagsBridge {
    /// The user opened another file: an open edit ends like Cancel (its changes are dropped). An Apply that runs
    /// finishes first and ends the edit itself.
    pub fn file_opened(&self) {
        if self.editing() && !self.applying.get() {
            tracing::info!("[Tags] Another file was opened: the edit of the jobs' tags and rating ends");
            self.cancel_edit();
        }
    }

    /// After every queue refresh (every job's id): edited jobs that left the queue leave the edit; with none left it
    /// ends. Not while Apply runs (its answers say what happened to each job).
    pub fn queue_jobs(&self, queued: &[i64]) {
        if self.applying.get() {
            return;
        }
        let (left, ended) = {
            let mut state = self.state.borrow_mut();
            let Some(editing) = state.editing.as_mut() else {
                return;
            };
            let left = editing.keep_queued(|id: JobId| queued.contains(&id.0));
            if left.is_empty() {
                return;
            }
            let ended = editing.jobs.is_empty();
            if ended {
                state.end_edit();
            }
            (left, ended)
        };
        let ids = left.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        let text = if ended {
            "The edited job left the queue: its tags and rating were not changed".to_owned()
        } else {
            format!("Job {ids} left the queue and the edit")
        };
        Self::tell(Notice::new(Kind::Editing, text));
        self.publish();
    }
}
