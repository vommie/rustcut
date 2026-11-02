//! Rows of the queue panel from the stored jobs, and the files a job leaves behind when its render was cut short.

use std::path::{Path, PathBuf};

use crate::job::JobSpec;
use crate::render::{TEMP_MARKER, WorkDirs, temp_path};
use crate::rustview::JobSyncRecord;
use crate::store::{JobRecord, SpecSlot};
use crate::time::Timestamp;

use super::JobSummary;

/// The panel's row of a job. A job whose spec this version cannot read shows its stored paths and why.
pub fn summarize(record: &JobRecord) -> JobSummary {
    let sync = JobSyncRecord::from_column(record.sync.as_deref()).state;
    let (name, profile, sections, duration, note) = match &record.spec {
        SpecSlot::Decoded(spec) => (
            spec.target.stem.clone(),
            spec.profile.name.clone(),
            spec.sections.len(),
            spec.output_duration(),
            record.note.clone(),
        ),
        SpecSlot::Unreadable { error, .. } => (
            record
                .target_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            String::new(),
            0,
            Timestamp::ZERO,
            Some(
                record
                    .note
                    .clone()
                    .unwrap_or_else(|| format!("This job cannot be read: {error}")),
            ),
        ),
    };
    JobSummary {
        id: record.id,
        state: record.state.clone(),
        attempt: record.attempt,
        name,
        target: record.target_path.clone(),
        source: record.source_path.clone(),
        profile,
        sections,
        duration,
        note,
        created_at: record.created_at,
        started_at: record.started_at,
        finished_at: record.finished_at,
        sync,
    }
}

/// What a render of `record` may have left when it was cut short (a crash, a kill, a removed job): the hidden temp
/// file next to the target, the vid.stab transform file and the two-pass statistics folder. Never the target itself
/// and never a file of the source (sidecars stay, PyCutRv bug 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleFiles {
    pub temp: Option<PathBuf>,
    pub transform: PathBuf,
    pub passlog: PathBuf,
}

impl StaleFiles {
    pub fn of(record: &JobRecord, dirs: &WorkDirs) -> Self {
        let temp = match &record.spec {
            SpecSlot::Decoded(spec) => Some(spec_temp(spec)),
            SpecSlot::Unreadable { .. } => stored_temp(&record.target_path),
        };
        Self {
            temp,
            transform: dirs.deshake.join(format!("job-{}.trf", record.id.0)),
            passlog: dirs.passlog.join(format!("job-{}", record.id.0)),
        }
    }

    /// Deletes what exists; a missing file is fine, anything else is logged.
    pub async fn remove(&self) {
        for file in self.temp.iter().chain(std::iter::once(&self.transform)) {
            remove_file(file).await;
        }
        match tokio::fs::remove_dir_all(&self.passlog).await {
            Ok(()) => tracing::info!(folder = %self.passlog.display(), "[Queue] Stale two-pass statistics removed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(folder = %self.passlog.display(), error = %e, "[Queue] Stale statistics not removed");
            }
        }
    }
}

fn spec_temp(spec: &JobSpec) -> PathBuf {
    temp_path(
        &spec.target.folder,
        &spec.target.stem,
        spec.profile.container.extension(),
    )
}

/// The temp file of a stored target path (a row whose spec is unreadable still knows its target).
fn stored_temp(target: &Path) -> Option<PathBuf> {
    let folder = target.parent()?;
    let stem = target.file_stem()?.to_str()?;
    let extension = target.extension()?.to_str()?;
    Some(temp_path(folder, stem, extension))
}

async fn remove_file(path: &Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let transform = path.extension().is_some_and(|e| e == "trf");
    // Only files RustCut names itself are deleted here.
    if !(name.contains(TEMP_MARKER) || transform) {
        tracing::warn!(file = %path.display(), "[Queue] Not a RustCut work file; left alone");
        return;
    }
    match tokio::fs::remove_file(path).await {
        Ok(()) => tracing::info!(file = %path.display(), "[Queue] Stale work file removed"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(file = %path.display(), error = %e, "[Queue] Stale work file not removed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobId, JobState, sample_spec};

    fn record(spec: SpecSlot) -> JobRecord {
        let sample = sample_spec();
        JobRecord {
            id: JobId(7),
            position: 1,
            state: JobState::Waiting,
            attempt: 1,
            created_at: 0,
            started_at: None,
            finished_at: None,
            source_path: sample.source.clone(),
            target_path: sample.target_path(),
            note: None,
            spec,
            sync: Some(r#"{"state":{"state":"pending"}}"#.into()),
            relocation: None,
        }
    }

    #[test]
    fn rows_of_readable_and_unreadable_jobs() {
        let row = summarize(&record(SpecSlot::Decoded(Box::new(sample_spec()))));
        assert_eq!((row.name.as_str(), row.sections), ("Clip - 01", 1));
        assert_eq!(row.duration, Timestamp::from_secs(3));
        assert_eq!(row.sync, crate::rustview::SyncState::Pending);
        let row = summarize(&record(SpecSlot::Unreadable {
            version: 9,
            error: "newer".into(),
        }));
        assert_eq!(row.name, "Clip - 01");
        assert!(row.note.unwrap().contains("newer"));
    }

    #[tokio::test]
    async fn stale_files_go_and_others_stay() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = sample_spec();
        spec.target.folder = dir.path().to_path_buf();
        let dirs = WorkDirs {
            deshake: dir.path().join("deshake"),
            passlog: dir.path().join("passlog"),
        };
        let mut rec = record(SpecSlot::Decoded(Box::new(spec.clone())));
        rec.target_path = spec.target_path();
        let stale = StaleFiles::of(&rec, &dirs);
        let temp = stale.temp.clone().unwrap();
        assert_eq!(temp, dir.path().join(".Clip - 01.rustcut-part.mkv"));
        std::fs::write(&temp, b"x").unwrap();
        std::fs::write(spec.target_path(), b"target").unwrap();
        std::fs::create_dir_all(&dirs.deshake).unwrap();
        std::fs::write(&stale.transform, b"t").unwrap();
        std::fs::create_dir_all(stale.passlog.join("x")).unwrap();
        stale.remove().await;
        assert!(!temp.exists() && !stale.transform.exists() && !stale.passlog.exists());
        assert!(spec.target_path().exists(), "the target is never touched");
        stale.remove().await;
        // A row without a readable spec still finds its temp file from the stored target.
        assert_eq!(
            StaleFiles::of(
                &record(SpecSlot::Unreadable {
                    version: 9,
                    error: String::new()
                }),
                &dirs
            )
            .temp,
            Some(PathBuf::from("/videos/out/.Clip - 01.rustcut-part.mkv"))
        );
    }
}
