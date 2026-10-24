//! Desktop notifications about the queue (P5 M5.2, ADR-15): a finished job (with "Open folder"), a failed job (its
//! reason, with "Show log"), the queue being done. Which ones appear is `desktop.notifications.*`; the "when done"
//! choice "notify" always shows the last one.

use std::path::Path;

use rustcut_core::job::{JobId, JobState};
use rustcut_core::queue::JobSummary;

use super::PowerBridge;
use crate::desktop::notify::{self, Note, Style};
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
        let settings = self.notices.borrow().clone();
        if !settings.enabled {
            return;
        }
        let timeout_secs = u32::from(settings.timeout_secs);
        match &row.state {
            JobState::Finished if settings.job_finished => {
                let target = row.target.clone();
                let open: Box<dyn FnOnce() + Send> = Box::new(move || {
                    if let Err(e) = opener::reveal(&target) {
                        tracing::warn!(error = %e, "[Power] The folder could not be shown");
                    }
                });
                notify::send(
                    &self.runtime,
                    Note {
                        style: Style::Done,
                        title: "Job finished".into(),
                        body: finished_text(row),
                        timeout_secs,
                        action: Some("Open folder".into()),
                    },
                    Some(open),
                );
            }
            JobState::Failed { reason } if settings.job_failed => {
                let (ui, id) = (self.ui.clone(), row.id);
                let show: Box<dyn FnOnce() + Send> = Box::new(move || ui.post(move || show_log(id)));
                notify::send(
                    &self.runtime,
                    Note {
                        style: Style::Failed,
                        title: "Job failed".into(),
                        body: failed_text(row, reason),
                        timeout_secs,
                        action: Some("Show log".into()),
                    },
                    Some(show),
                );
            }
            _ => {}
        }
    }

    /// Whether the queue-done notification appears for `explicit` ("when done: notify") or by the settings.
    pub(super) fn queue_done_notice(&self, explicit: bool) -> Option<u32> {
        let settings = self.notices.borrow();
        (explicit || (settings.enabled && settings.queue_done)).then_some(u32::from(settings.timeout_secs))
    }
}

/// A notification's "Show log" (UI thread).
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
        };
        assert_eq!(finished_text(&row), "clip - 01.mkv in /videos/out");
        assert_eq!(failed_text(&row, "exit code 1"), "clip - 01.mkv: exit code 1");
    }
}
