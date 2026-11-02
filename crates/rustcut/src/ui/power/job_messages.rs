//! The queue's messages (P5 M5.2, M6.5): a finished job (with "Open folder"), a failed job (its reason, with "Show
//! log"). Where they appear (toast, the desktop, nothing) is the notice system's routing (`Kind::JobFinished`,
//! `Kind::JobFailed`); the queue being done is posted by the bridge itself.

use std::path::Path;

use rustcut_core::job::{JobId, JobState};
use rustcut_core::queue::JobSummary;

use super::PowerBridge;
use crate::ui::jump::JumpTarget;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::queue::file_label;

/// "clip - 01.mkv in /videos/out".
pub fn finished_text(row: &JobSummary) -> String {
    let folder = row
        .target
        .parent()
        .map(Path::display)
        .map(|d| d.to_string())
        .unwrap_or_default();
    format!("{} in {folder}", file_label(row))
}

/// "clip - 01.mkv: ffmpeg ended with exit code 1".
pub fn failed_text(row: &JobSummary, reason: &str) -> String {
    format!("{}: {reason}", file_label(row))
}

impl PowerBridge {
    /// A job's render ended.
    pub(super) fn job_done(&self, row: &JobSummary) {
        match &row.state {
            JobState::Finished => {
                let (target, runtime) = (row.target.clone(), self.runtime.clone());
                notices::post(
                    Notice::new(Kind::JobFinished, "Job finished")
                        .detail(finished_text(row))
                        .action("Open folder", move || {
                            let target = target.clone();
                            // The file manager is started off the UI thread.
                            runtime.spawn_blocking(move || {
                                if let Err(e) = opener::reveal(&target) {
                                    tracing::warn!(error = %e, "[Power] The folder could not be shown");
                                }
                            });
                        }),
                );
            }
            JobState::Failed { reason } => {
                let id = row.id;
                // A click on it leads to the job in the queue (M6.5e, Q-N8 #29).
                notices::post(
                    Notice::new(Kind::JobFailed, "Job failed")
                        .detail(failed_text(row, reason))
                        .jump(JumpTarget::QueueJob(id))
                        .leading_action("Show log", move || show_log(id)),
                );
            }
            _ => {}
        }
    }
}

/// "Show log" of a failed job's message (UI thread).
fn show_log(id: JobId) {
    super::POWER.with(|p| {
        if let Some(b) = p.borrow().upgrade()
            && let Some(queue) = b.queue.upgrade()
        {
            queue.show_log(id);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::job::JobId;
    use rustcut_core::rustview::SyncState;
    use rustcut_core::time::Timestamp;

    use super::*;

    #[test]
    fn notification_texts_name_the_file_and_where_or_why() {
        let row = JobSummary {
            id: JobId(4),
            state: JobState::Finished,
            attempt: 1,
            name: "clip - 01".into(),
            target: PathBuf::from("/videos/out/clip - 01.mkv"),
            source: PathBuf::from("/videos/clip.mp4"),
            profile: "AV1 balanced".into(),
            sections: 1,
            duration: Timestamp::from_secs(20),
            note: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            sync: SyncState::default(),
            relocation: None,
        };
        assert_eq!(finished_text(&row), "clip - 01.mkv in /videos/out");
        assert_eq!(failed_text(&row, "exit code 1"), "clip - 01.mkv: exit code 1");
    }
}
