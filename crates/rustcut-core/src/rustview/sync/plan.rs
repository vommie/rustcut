//! The pure decisions of the sync (ADR-6): which values a job's record should hold, whether it already does, what a
//! write passes to RustView, and what a cleanup does. Tested without a client or a runtime.

use std::path::Path;

use super::{JobSyncRecord, SyncPhase, SyncState};
use crate::job::JobMetadata;
use crate::naming::same_path;
use crate::rustview::is_md5;
use crate::settings::WriteTiming;

/// The values RustCut wants in the record (tags sorted, no duplicates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Wanted {
    pub tags: Vec<u32>,
    pub rating: u8,
}

impl Wanted {
    /// What is stored as [`JobSyncRecord::synced`] after a write.
    pub fn as_metadata(&self, source_hash: Option<&str>) -> JobMetadata {
        JobMetadata {
            tags: self.tags.clone(),
            rating: Some(self.rating),
            source_hash: source_hash.map(str::to_owned),
            without_rustview: false,
        }
    }
}

fn normalized(tags: &[u32]) -> Vec<u32> {
    let mut tags = tags.to_vec();
    tags.sort_unstable();
    tags.dedup();
    tags
}

/// The job's metadata applied to what the record held before: on a record that existed before RustCut wrote to it, a job
/// without tags (or without a rating) leaves the record's own tags (or rating); a record RustCut creates gets exactly the
/// job's values.
pub(super) fn wanted(record: &JobSyncRecord, metadata: &JobMetadata) -> Wanted {
    if record.preexisting == Some(true) {
        let tags = if metadata.tags.is_empty() {
            record.previous_tags.clone().unwrap_or_default()
        } else {
            metadata.tags.clone()
        };
        Wanted {
            tags: normalized(&tags),
            rating: metadata.rating.or(record.previous_rating).unwrap_or(0),
        }
    } else {
        Wanted {
            tags: normalized(&metadata.tags),
            rating: metadata.rating.unwrap_or(0),
        }
    }
}

/// Whether the record was written at `target`.
pub(super) fn synced_at(record: &JobSyncRecord, target: &Path) -> bool {
    record.synced_path.as_deref().is_some_and(|p| same_path(p, target))
}

/// The source hash worth passing to RustView.
pub(super) fn valid_hash(metadata: &JobMetadata) -> Option<&str> {
    metadata.source_hash.as_deref().filter(|h| is_md5(h))
}

/// Whether RustView already holds what the job wants for `phase` (no call needed).
pub(super) fn in_sync(record: &JobSyncRecord, target: &Path, metadata: &JobMetadata, phase: SyncPhase) -> bool {
    record.state == SyncState::Synced
        && synced_at(record, target)
        && record.synced.as_ref().is_some_and(|synced| {
            let wanted = wanted(record, metadata);
            synced.tags == wanted.tags && synced.rating == Some(wanted.rating)
        })
        && (phase != SyncPhase::Finished || record.finished)
}

/// Whether a job in `phase` writes its record at all with this write timing. "After render" writes nothing before the
/// render was verified, unless a record was already written for the target (it is kept up to date).
pub(super) fn writes(timing: WriteTiming, phase: SyncPhase, record: &JobSyncRecord, target: &Path) -> bool {
    match timing {
        WriteTiming::AtEnqueue => true,
        WriteTiming::AfterRender => phase == SyncPhase::Finished || synced_at(record, target),
    }
}

/// What one write passes to `set-media`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WriteArgs {
    /// `set_own_media` (a record RustCut creates or created) instead of `set_media`.
    pub own: bool,
    pub rating: Option<u8>,
    pub tags: Option<Vec<u32>>,
    pub hash: Option<String>,
}

/// The `set-media` arguments for a record whose `preexisting` is known; `None` = nothing to write. A record RustCut
/// creates gets every value, and the source hash when it is created at this path (later writes never replace the hash
/// RustView computes from the finished file). A preexisting record gets only the values that differ from what it holds,
/// never a hash; after the render it is touched once more so RustView reads the new file's facts.
pub(super) fn write_args(
    record: &JobSyncRecord,
    target: &Path,
    metadata: &JobMetadata,
    phase: SyncPhase,
) -> Option<WriteArgs> {
    let wanted = wanted(record, metadata);
    let here = synced_at(record, target);
    if record.preexisting == Some(true) {
        let (tags_now, rating_now) = match record.synced.as_ref().filter(|_| here) {
            Some(synced) => (Some(normalized(&synced.tags)), synced.rating),
            None => (record.previous_tags.as_deref().map(normalized), record.previous_rating),
        };
        let tags = (tags_now.as_ref() != Some(&wanted.tags)).then(|| wanted.tags.clone());
        let rating = (rating_now != Some(wanted.rating)).then_some(wanted.rating);
        let touch = phase == SyncPhase::Finished && !record.finished;
        (tags.is_some() || rating.is_some() || touch).then_some(WriteArgs {
            own: false,
            rating,
            tags,
            hash: None,
        })
    } else {
        Some(WriteArgs {
            own: true,
            rating: Some(wanted.rating),
            tags: Some(wanted.tags),
            hash: if here {
                None
            } else {
                valid_hash(metadata).map(str::to_owned)
            },
        })
    }
}

/// What a cleanup does with the record of a job that failed, was canceled, removed or moved to another target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Cleanup {
    /// Nothing was written (or it is not known whose record it is): nothing to undo.
    Nothing,
    /// The record RustCut created: delete it (after checking that the file does not exist and that the record is still
    /// RustCut's).
    Delete,
    /// A record that existed before: give it back the tags and the rating it had (only what RustCut changed).
    Restore { tags: Option<Vec<u32>>, rating: Option<u8> },
}

pub(super) fn cleanup(record: &JobSyncRecord) -> Cleanup {
    if record.synced_path.is_none() {
        return Cleanup::Nothing;
    }
    match record.preexisting {
        Some(false) => Cleanup::Delete,
        Some(true) => {
            let synced = record.synced.clone().unwrap_or_default();
            let previous_tags = record.previous_tags.as_deref().map(normalized);
            let tags = previous_tags.filter(|previous| *previous != normalized(&synced.tags));
            let rating = record
                .previous_rating
                .filter(|previous| Some(*previous) != synced.rating);
            if tags.is_none() && rating.is_none() {
                Cleanup::Nothing
            } else {
                Cleanup::Restore { tags, rating }
            }
        }
        None => Cleanup::Nothing,
    }
}

/// The record after a cleanup (or after its record went to another job): nothing of it is in RustView any more. The
/// hash registration stays (the hash list has no removal).
pub(super) fn cleaned(record: &JobSyncRecord) -> JobSyncRecord {
    JobSyncRecord {
        state: SyncState::CleanedUp,
        hash_registered: record.hash_registered,
        ..JobSyncRecord::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "/out/clip - 01.mkv";

    fn meta(tags: &[u32], rating: Option<u8>) -> JobMetadata {
        JobMetadata {
            tags: tags.to_vec(),
            rating,
            source_hash: Some("8f618d1ebd606025257057ee18536e64".into()),
            without_rustview: false,
        }
    }

    fn preexisting(tags: &[u32], rating: u8) -> JobSyncRecord {
        JobSyncRecord {
            preexisting: Some(true),
            previous_tags: Some(tags.to_vec()),
            previous_rating: Some(rating),
            ..JobSyncRecord::default()
        }
    }

    fn synced(mut record: JobSyncRecord, metadata: &JobMetadata) -> JobSyncRecord {
        record.synced = Some(wanted(&record, metadata).as_metadata(None));
        record.synced_path = Some(TARGET.into());
        record.state = SyncState::Synced;
        record
    }

    #[test]
    fn a_created_record_gets_the_jobs_values_and_the_hash_once() {
        let created = JobSyncRecord {
            preexisting: Some(false),
            ..JobSyncRecord::default()
        };
        let metadata = meta(&[5, 4, 5], Some(3));
        let first = write_args(&created, Path::new(TARGET), &metadata, SyncPhase::Enqueued).unwrap();
        assert_eq!(
            first,
            WriteArgs {
                own: true,
                rating: Some(3),
                tags: Some(vec![4, 5]),
                hash: metadata.source_hash.clone(),
            }
        );
        let done = synced(created, &metadata);
        assert!(in_sync(&done, Path::new(TARGET), &metadata, SyncPhase::Enqueued));
        assert!(
            !in_sync(&done, Path::new(TARGET), &metadata, SyncPhase::Finished),
            "asserted once more after the render"
        );
        let again = write_args(&done, Path::new(TARGET), &metadata, SyncPhase::Finished).unwrap();
        assert_eq!(
            again.hash, None,
            "the hash RustView computes from the file is never replaced"
        );
        assert!(!in_sync(
            &done,
            Path::new("/out/clip - 02.mkv"),
            &metadata,
            SyncPhase::Enqueued
        ));
        assert!(!in_sync(
            &done,
            Path::new(TARGET),
            &meta(&[4], Some(3)),
            SyncPhase::Enqueued
        ));
    }

    #[test]
    fn a_preexisting_record_keeps_what_the_job_does_not_set() {
        let before = preexisting(&[9, 2], 5);
        assert_eq!(
            wanted(&before, &meta(&[], None)),
            Wanted {
                tags: vec![2, 9],
                rating: 5
            }
        );
        assert_eq!(
            write_args(&before, Path::new(TARGET), &meta(&[], None), SyncPhase::Enqueued),
            None,
            "nothing to change"
        );
        let touch = write_args(&before, Path::new(TARGET), &meta(&[], None), SyncPhase::Finished).unwrap();
        assert_eq!(
            (touch.own, touch.tags, touch.rating, touch.hash),
            (false, None, None, None),
            "the new file's facts are read once"
        );
        let rated = write_args(&before, Path::new(TARGET), &meta(&[], Some(2)), SyncPhase::Enqueued).unwrap();
        assert_eq!((rated.tags, rated.rating), (None, Some(2)));
        let tagged = write_args(&before, Path::new(TARGET), &meta(&[1], None), SyncPhase::Enqueued).unwrap();
        assert_eq!((tagged.tags, tagged.rating), (Some(vec![1]), None));
    }

    #[test]
    fn cleanup_deletes_only_created_records_and_restores_only_changes() {
        assert_eq!(cleanup(&JobSyncRecord::default()), Cleanup::Nothing);
        let created = synced(
            JobSyncRecord {
                preexisting: Some(false),
                ..JobSyncRecord::default()
            },
            &meta(&[1], Some(3)),
        );
        assert_eq!(cleanup(&created), Cleanup::Delete);
        let unknown = JobSyncRecord {
            preexisting: None,
            ..created
        };
        assert_eq!(
            cleanup(&unknown),
            Cleanup::Nothing,
            "never delete what is not known to be RustCut's"
        );
        let untouched = synced(preexisting(&[9], 5), &meta(&[], None));
        assert_eq!(cleanup(&untouched), Cleanup::Nothing);
        let retagged = synced(preexisting(&[9], 5), &meta(&[1, 2], None));
        assert_eq!(
            cleanup(&retagged),
            Cleanup::Restore {
                tags: Some(vec![9]),
                rating: None
            }
        );
        let after = cleaned(&JobSyncRecord {
            hash_registered: true,
            ..retagged
        });
        assert_eq!(after.state, SyncState::CleanedUp);
        assert!(after.hash_registered && after.synced_path.is_none() && after.preexisting.is_none());
    }

    #[test]
    fn after_render_timing_waits_for_the_render() {
        let fresh = JobSyncRecord::default();
        let target = Path::new(TARGET);
        assert!(writes(WriteTiming::AtEnqueue, SyncPhase::Enqueued, &fresh, target));
        assert!(!writes(WriteTiming::AfterRender, SyncPhase::Enqueued, &fresh, target));
        assert!(!writes(WriteTiming::AfterRender, SyncPhase::Edited, &fresh, target));
        assert!(writes(WriteTiming::AfterRender, SyncPhase::Finished, &fresh, target));
        let written = synced(fresh, &meta(&[], None));
        assert!(
            writes(WriteTiming::AfterRender, SyncPhase::Edited, &written, target),
            "a record written before the timing changed is kept up to date"
        );
    }
}
