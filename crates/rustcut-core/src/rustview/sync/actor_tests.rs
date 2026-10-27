//! The sync actor against the in-memory RustView ([`super::fake`]): the job's record through enqueue, finish, failure,
//! cancel, removal and a renamed target; preexisting records never deleted; the back-off while RustView is unreachable.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::fake::FakeRustView;
use super::{ConnectionState, JobSyncRecord, SyncCommand, SyncEvent, SyncPhase, SyncRecords, SyncService, SyncState};
use crate::job::{JobId, JobMetadata};
use crate::rustview::{RustViewError, SOURCE_APP, spawn_sync};
use crate::settings::{RustViewSettings, WriteTiming};

const HASH: &str = "8f618d1ebd606025257057ee18536e64";
const WAIT: Duration = Duration::from_secs(20);

struct Harness {
    sync: SyncService,
    records: SyncRecords,
    fake: FakeRustView,
    dir: tempfile::TempDir,
}

fn start(settings: RustViewSettings) -> Harness {
    let fake = FakeRustView::new();
    let client = fake.clone();
    let (sync, records, _task) = spawn_sync(&tokio::runtime::Handle::current(), settings, move |_| client.clone());
    Harness {
        sync,
        records,
        fake,
        dir: tempfile::tempdir().unwrap(),
    }
}

fn metadata(tags: &[u32], rating: Option<u8>) -> JobMetadata {
    JobMetadata {
        tags: tags.to_vec(),
        rating,
        source_hash: Some(HASH.into()),
        without_rustview: false,
    }
}

impl Harness {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn text(&self, name: &str) -> String {
        self.path(name).display().to_string()
    }

    fn job(&self, id: i64, name: &str, metadata: JobMetadata, phase: SyncPhase) {
        self.sync
            .handle
            .send(SyncCommand::Job {
                id: JobId(id),
                target: self.path(name),
                metadata,
                phase,
                record: JobSyncRecord::default(),
            })
            .unwrap();
    }

    fn cleanup(&self, id: i64) {
        self.sync
            .handle
            .send(SyncCommand::Cleanup {
                id: JobId(id),
                record: JobSyncRecord::default(),
            })
            .unwrap();
    }

    /// The first record of job `id` that satisfies `done`.
    async fn until(&mut self, id: i64, done: impl Fn(&JobSyncRecord) -> bool) -> JobSyncRecord {
        tokio::time::timeout(WAIT, async {
            loop {
                let (job, record) = self.records.recv().await.unwrap();
                if job == JobId(id) && done(&record) {
                    return record;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn synced(&mut self, id: i64) -> JobSyncRecord {
        self.until(id, |r| r.state == SyncState::Synced).await
    }

    /// Waits until everything sent before is done: jobs are worked through in id order, so a later job with a higher id
    /// settles last (a finished one writes with either timing).
    async fn barrier(&mut self) {
        self.touch("barrier.mkv");
        self.job(999, "barrier.mkv", JobMetadata::default(), SyncPhase::Finished);
        self.until(999, |r| r.finished).await;
    }

    fn touch(&self, name: &str) {
        std::fs::write(self.path(name), b"video").unwrap();
    }
}

#[tokio::test]
async fn at_enqueue_a_stub_is_written_and_asserted_after_the_render() {
    let mut h = start(RustViewSettings::default());
    h.job(1, "a.mkv", metadata(&[5, 4], Some(3)), SyncPhase::Enqueued);
    assert_eq!(
        h.until(1, |_| true).await.state,
        SyncState::Pending,
        "marked before the call"
    );
    let record = h.synced(1).await;
    assert_eq!(record.preexisting, Some(false));
    assert_eq!(record.synced_path, Some(h.path("a.mkv")));
    assert!(record.hash_registered && !record.finished);
    let stub = h.fake.media(&h.text("a.mkv")).unwrap();
    assert_eq!(
        (stub.source.as_deref(), stub.file_hash.as_str(), stub.tags, stub.rating),
        (Some(SOURCE_APP), HASH, vec![4, 5], 3)
    );

    h.touch("a.mkv");
    h.job(1, "a.mkv", metadata(&[5, 4], Some(3)), SyncPhase::Finished);
    let finished = h.until(1, |r| r.finished).await;
    assert_eq!(finished.state, SyncState::Synced);
    let writes = h.fake.writes();
    assert_eq!(writes.len(), 3, "{writes:?}");
    assert!(writes[0].starts_with("insert-hash"));
    assert!(
        writes[2].starts_with("set-own-media") && writes[2].ends_with("hash=None"),
        "{writes:?}"
    );
}

#[tokio::test]
async fn a_failed_canceled_or_removed_job_loses_its_stub() {
    let mut h = start(RustViewSettings::default());
    h.job(1, "a.mkv", metadata(&[1], Some(2)), SyncPhase::Enqueued);
    h.synced(1).await;
    h.cleanup(1);
    let record = h.until(1, |r| r.state == SyncState::CleanedUp).await;
    assert!(record.synced_path.is_none() && record.preexisting.is_none());
    assert!(record.hash_registered, "the hash list has no removal");
    assert!(h.fake.media(&h.text("a.mkv")).is_none());
    assert!(h.fake.calls().iter().any(|c| c.starts_with("delete-media")));
}

#[tokio::test]
async fn a_canceled_job_that_was_never_written_calls_nothing() {
    let mut h = start(RustViewSettings {
        enabled: false,
        ..RustViewSettings::default()
    });
    // Queued and canceled while nothing could be written.
    h.job(1, "a.mkv", metadata(&[1], None), SyncPhase::Enqueued);
    h.cleanup(1);
    h.sync
        .handle
        .send(SyncCommand::Reconfigure {
            settings: RustViewSettings::default(),
        })
        .unwrap();
    h.barrier().await;
    assert!(h.fake.media(&h.text("a.mkv")).is_none());
    assert!(
        !h.fake
            .calls()
            .iter()
            .any(|c| c.contains("a.mkv") || c.starts_with("insert-hash")),
        "{:?}",
        h.fake.calls()
    );
}

#[tokio::test]
async fn a_renamed_target_moves_the_record() {
    let mut h = start(RustViewSettings::default());
    h.job(1, "a.mkv", metadata(&[1], Some(2)), SyncPhase::Enqueued);
    h.synced(1).await;
    h.job(1, "b.mkv", metadata(&[1], Some(2)), SyncPhase::Edited);
    let b = h.path("b.mkv");
    let record = h.until(1, |r| r.synced_path.as_ref() == Some(&b)).await;
    assert_eq!(record.state, SyncState::Synced);
    assert!(h.fake.media(&h.text("a.mkv")).is_none());
    assert_eq!(h.fake.media(&h.text("b.mkv")).unwrap().tags, vec![1]);
    assert_eq!(
        h.fake.writes().iter().filter(|w| w.starts_with("insert-hash")).count(),
        1,
        "the hash is registered once per job"
    );
}

#[tokio::test]
async fn a_preexisting_record_is_never_deleted_and_gets_its_values_back() {
    let mut h = start(RustViewSettings::default());
    h.fake.put(&h.text("a.mkv"), &[9], 5, None);
    h.job(1, "a.mkv", metadata(&[1, 2], Some(2)), SyncPhase::Enqueued);
    let record = h.synced(1).await;
    assert_eq!(
        (record.preexisting, record.previous_tags.clone(), record.previous_rating),
        (Some(true), Some(vec![9]), Some(5))
    );
    let media = h.fake.media(&h.text("a.mkv")).unwrap();
    assert_eq!(
        (media.tags, media.rating, media.source),
        (vec![1, 2], 2, None),
        "the source stays"
    );
    assert_ne!(media.file_hash, HASH, "a preexisting record keeps its hash");
    h.cleanup(1);
    h.until(1, |r| r.state == SyncState::CleanedUp).await;
    let media = h.fake.media(&h.text("a.mkv")).unwrap();
    assert_eq!((media.tags, media.rating), (vec![9], 5));
    assert!(!h.fake.calls().iter().any(|c| c.starts_with("delete-media")));
}

#[tokio::test]
async fn a_preexisting_record_is_not_touched_by_a_job_without_metadata() {
    let mut h = start(RustViewSettings::default());
    h.fake.put(&h.text("a.mkv"), &[9], 5, Some("RustView"));
    h.job(1, "a.mkv", JobMetadata::default(), SyncPhase::Enqueued);
    h.synced(1).await;
    h.cleanup(1);
    h.until(1, |r| r.state == SyncState::CleanedUp).await;
    assert!(
        !h.fake
            .calls()
            .iter()
            .any(|c| c.starts_with("set") || c.starts_with("delete")),
        "{:?}",
        h.fake.calls()
    );
}

#[tokio::test]
async fn a_stub_taken_over_by_another_app_or_with_a_file_stays() {
    let mut h = start(RustViewSettings::default());
    h.job(1, "a.mkv", metadata(&[], None), SyncPhase::Enqueued);
    h.job(2, "b.mkv", metadata(&[], None), SyncPhase::Enqueued);
    h.synced(1).await;
    h.synced(2).await;
    h.fake.0.lock().media.get_mut(&h.path("a.mkv")).unwrap().source = Some("RustView".into());
    h.touch("b.mkv");
    h.cleanup(1);
    h.cleanup(2);
    h.until(2, |r| r.state == SyncState::CleanedUp).await;
    assert!(h.fake.media(&h.text("a.mkv")).is_some());
    assert!(h.fake.media(&h.text("b.mkv")).is_some());
    assert!(!h.fake.calls().iter().any(|c| c.starts_with("delete-media")));
}

#[tokio::test]
async fn after_render_registers_the_hash_at_once_and_writes_after_the_render() {
    let mut h = start(RustViewSettings {
        write_timing: WriteTiming::AfterRender,
        ..RustViewSettings::default()
    });
    h.job(1, "a.mkv", metadata(&[3], Some(4)), SyncPhase::Enqueued);
    let record = h.until(1, |r| r.hash_registered).await;
    assert_eq!(record.state, SyncState::NotSynced);
    h.barrier().await;
    assert!(h.fake.media(&h.text("a.mkv")).is_none());
    h.touch("a.mkv");
    h.job(1, "a.mkv", metadata(&[3], Some(4)), SyncPhase::Finished);
    let record = h.until(1, |r| r.finished).await;
    assert_eq!(record.preexisting, Some(false));
    assert_eq!(h.fake.media(&h.text("a.mkv")).unwrap().tags, vec![3]);
}

#[tokio::test]
async fn a_finished_job_whose_file_is_gone_writes_nothing() {
    let mut h = start(RustViewSettings {
        write_timing: WriteTiming::AfterRender,
        ..RustViewSettings::default()
    });
    h.job(1, "gone.mkv", JobMetadata::default(), SyncPhase::Finished);
    h.barrier().await;
    assert!(h.fake.media(&h.text("gone.mkv")).is_none());
}

#[tokio::test]
async fn two_jobs_with_one_target_hand_the_record_over() {
    let mut h = start(RustViewSettings::default());
    h.job(1, "a.mkv", metadata(&[1], None), SyncPhase::Enqueued);
    h.synced(1).await;
    h.job(2, "a.mkv", metadata(&[2], None), SyncPhase::Enqueued);
    assert_eq!(h.synced(2).await.preexisting, Some(true), "job 1's stub was there");
    h.cleanup(1);
    let heir = h.until(2, |r| r.preexisting == Some(false)).await;
    assert_eq!(heir.state, SyncState::Synced);
    h.until(1, |r| r.state == SyncState::CleanedUp).await;
    assert!(h.fake.media(&h.text("a.mkv")).is_some(), "job 2 still wants it");
    h.cleanup(2);
    h.until(2, |r| r.state == SyncState::CleanedUp).await;
    assert!(h.fake.media(&h.text("a.mkv")).is_none());
}

#[tokio::test]
async fn a_refused_write_fails_without_retrying() {
    let mut h = start(RustViewSettings::default());
    let (tx, rx) = tokio::sync::oneshot::channel();
    h.sync.handle.send(SyncCommand::TagTree { reply: tx }).unwrap();
    assert_eq!(rx.await.unwrap().unwrap().tags.len(), 10);
    h.fake.fail_next(RustViewError::Api("Invalid argument".into()));
    h.job(1, "a.mkv", JobMetadata::default(), SyncPhase::Enqueued);
    let record = h.until(1, |r| matches!(r.state, SyncState::Failed { .. })).await;
    assert_eq!(record.attempts, 1);
    h.barrier().await;
    assert!(h.fake.media(&h.text("a.mkv")).is_none(), "not tried again");
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_rustview_is_retried_with_a_growing_pause() {
    let mut h = start(RustViewSettings::default());
    let mut events = h.sync.events.subscribe();
    h.fake.set_unreachable(true);
    h.job(1, "a.mkv", metadata(&[1], None), SyncPhase::Enqueued);
    let mut pauses = Vec::new();
    while pauses.len() < 3 {
        if let SyncEvent::Connection(ConnectionState::Unavailable { retry_in_secs, reason }) =
            tokio::time::timeout(WAIT, events.recv()).await.unwrap().unwrap()
        {
            assert!(reason.contains("not installed"), "{reason}");
            pauses.push(retry_in_secs);
        }
    }
    assert_eq!(pauses, [2, 4, 8]);
    let (tx, rx) = tokio::sync::oneshot::channel();
    h.sync
        .handle
        .send(SyncCommand::GetMedia {
            path: h.path("a.mkv"),
            reply: tx,
        })
        .unwrap();
    assert!(
        rx.await.unwrap().unwrap_err().is_transient(),
        "a lookup during the pause fails at once"
    );
    let calls_before = h.fake.calls().len();
    let started = tokio::time::Instant::now();
    h.fake.set_unreachable(false);
    h.synced(1).await;
    assert!(started.elapsed() >= Duration::from_secs(7), "waited for the pause");
    assert!(h.fake.calls().len() > calls_before);
    let mut connected = false;
    while let Ok(event) = events.try_recv() {
        connected |= event == SyncEvent::Connection(ConnectionState::Connected);
    }
    assert!(connected);
}

#[tokio::test]
async fn switched_off_nothing_is_called_until_switched_on() {
    let mut h = start(RustViewSettings {
        enabled: false,
        ..RustViewSettings::default()
    });
    h.job(1, "a.mkv", metadata(&[1], None), SyncPhase::Enqueued);
    let (tx, rx) = tokio::sync::oneshot::channel();
    h.sync
        .handle
        .send(SyncCommand::CheckHash {
            md5: HASH.into(),
            reply: tx,
        })
        .unwrap();
    assert!(rx.await.unwrap().is_err());
    assert!(h.fake.calls().is_empty());
    h.sync
        .handle
        .send(SyncCommand::Reconfigure {
            settings: RustViewSettings::default(),
        })
        .unwrap();
    h.synced(1).await;
    assert!(h.fake.media(&h.text("a.mkv")).is_some());
}

#[tokio::test]
async fn lookups_are_answered_and_the_tag_tree_is_kept() {
    let mut h = start(RustViewSettings::default());
    let mut events = h.sync.events.subscribe();
    for _ in 0..2 {
        let (tx, rx) = tokio::sync::oneshot::channel();
        h.sync.handle.send(SyncCommand::TagTree { reply: tx }).unwrap();
        assert_eq!(rx.await.unwrap().unwrap().total_tags, 10);
    }
    assert_eq!(h.fake.calls().iter().filter(|c| *c == "tag-tree").count(), 1);
    assert!(matches!(
        events.recv().await.unwrap(),
        SyncEvent::TagTreeChanged(_) | SyncEvent::Connection(_)
    ));
    h.sync
        .handle
        .send(SyncCommand::InsertHash {
            md5: HASH.to_uppercase(),
        })
        .unwrap();
    h.sync
        .handle
        .send(SyncCommand::InsertHash { md5: "no".into() })
        .unwrap();
    h.barrier().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    h.sync
        .handle
        .send(SyncCommand::CheckHash {
            md5: HASH.into(),
            reply: tx,
        })
        .unwrap();
    assert!(rx.await.unwrap().unwrap().known);
    drop(h.records.try_recv());
}

#[tokio::test]
async fn shutdown_does_not_wait_for_a_hanging_call() {
    let h = start(RustViewSettings::default());
    h.fake.0.lock().hang = true;
    let (tx, _rx) = tokio::sync::oneshot::channel();
    h.sync
        .handle
        .send(SyncCommand::GetMedia {
            path: Path::new("/x.mkv").into(),
            reply: tx,
        })
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        h.sync.handle.request(|reply| SyncCommand::Shutdown { reply }),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn the_test_asks_rustview_again_also_when_connected() {
    let h = start(RustViewSettings::default());
    let test = || async {
        let (tx, rx) = tokio::sync::oneshot::channel();
        h.sync.handle.send(SyncCommand::Test { reply: tx }).unwrap();
        tokio::time::timeout(WAIT, rx).await.unwrap().unwrap()
    };
    let trees = || h.fake.calls().iter().filter(|c| *c == "tag-tree").count();
    assert_eq!(test().await.unwrap().total_tags, 10);
    let before = trees();
    assert_eq!(test().await.unwrap().total_tags, 10);
    assert_eq!(
        trees(),
        before + 1,
        "every test calls RustView, the cache does not answer it"
    );
    h.fake.set_unreachable(true);
    assert!(test().await.unwrap_err().is_transient());
    // During the pause after a failure the test does not wait for it.
    h.fake.set_unreachable(false);
    assert_eq!(test().await.unwrap().total_tags, 10);

    let off = start(RustViewSettings {
        enabled: false,
        ..RustViewSettings::default()
    });
    let (tx, rx) = tokio::sync::oneshot::channel();
    off.sync.handle.send(SyncCommand::Test { reply: tx }).unwrap();
    assert!(rx.await.unwrap().is_err());
    assert!(off.fake.calls().is_empty());
}
