//! The queue linked with the sync actor (in-memory RustView, no ffmpeg): a job's stub through enqueue, edit, cancel,
//! failure and removal, the record stored in the job's `sync` column, and a restart that picks the records up again.

use std::path::Path;

use super::tests::{enqueue, env, options, spec_in, wait_for};
use super::*;
use crate::job::JobMetadata;
use crate::queue::QueueEvent;
use crate::rustview::fake::FakeRustView;
use crate::rustview::{JobSyncRecord, SyncCommand, SyncState, spawn_sync};
use crate::settings::RustViewSettings;
use crate::store::{Store, spawn_store};

fn start(
    store: &StoreHandle,
    dir: &Path,
    fake: &FakeRustView,
    paused: bool,
) -> (QueueService, crate::rustview::SyncService) {
    let client = fake.clone();
    let runtime = tokio::runtime::Handle::current();
    let (sync, records, _) = spawn_sync(&runtime, RustViewSettings::default(), move |_| client.clone());
    let link = SyncLink {
        handle: sync.handle.clone(),
        records,
    };
    let (queue, _) = spawn_queue_linked(&runtime, store.clone(), env(dir), options(paused), Some(link));
    (queue, sync)
}

fn tagged(dir: &Path, stem: &str) -> Box<crate::job::JobSpec> {
    let mut spec = spec_in(dir, stem);
    spec.metadata = JobMetadata {
        tags: vec![2],
        rating: Some(4),
        source_hash: Some("8f618d1ebd606025257057ee18536e64".into()),
        without_rustview: false,
    };
    spec
}

async fn stored_record(store: &StoreHandle, id: JobId) -> JobSyncRecord {
    let row = store
        .request(|reply| StoreCommand::Job { id, reply })
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    JobSyncRecord::from_column(row.sync.as_deref())
}

#[tokio::test]
async fn a_queued_job_gets_a_stub_that_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let fake = FakeRustView::new();
    let (queue, _sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    let id = enqueue(&queue, tagged(dir.path(), "a")).await.unwrap();
    wait_for(&mut events, id, |row| row.sync == SyncState::Synced).await;
    let a = dir.path().join("a.mkv").display().to_string();
    let stub = fake.media(&a).unwrap();
    assert_eq!((stub.tags, stub.rating), (vec![2], 4));
    let stored = stored_record(&store, id).await;
    assert_eq!(stored.preexisting, Some(false), "the record is stored with the job");

    // The target is renamed while the job waits: the stub moves along.
    queue
        .handle
        .request(|reply| QueueCommand::UpdateSpec {
            id,
            spec: tagged(dir.path(), "b"),
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    let b = dir.path().join("b.mkv");
    tokio::time::timeout(Duration::from_secs(10), async {
        while stored_record(&store, id).await.synced_path.as_deref() != Some(b.as_path()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(fake.media(&a).is_none());
    assert!(fake.media(&b.display().to_string()).is_some());

    // Canceled while waiting: the stub goes.
    queue.handle.send(QueueCommand::Cancel { ids: vec![id] }).unwrap();
    wait_for(&mut events, id, |row| {
        row.state == JobState::Canceled && row.sync == SyncState::CleanedUp
    })
    .await;
    assert!(fake.media(&b.display().to_string()).is_none());
}

#[tokio::test]
async fn a_job_saved_without_rustview_is_never_sent() {
    // The Tag & Rate panel's "Active" switch was off (user decision Q-P8b): no record, no tags, no source hash, also
    // when the job is edited, canceled or removed.
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let fake = FakeRustView::new();
    let (queue, _sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    let mut private = spec_in(dir.path(), "private");
    private.metadata.without_rustview = true;
    private.metadata.source_hash = Some("0123456789abcdef0123456789abcdef".into());
    let private = enqueue(&queue, private).await.unwrap();
    // A job behind it is synced: the sync actor works in id order, so the first one was looked at before.
    let linked = enqueue(&queue, tagged(dir.path(), "linked")).await.unwrap();
    wait_for(&mut events, linked, |row| row.sync == SyncState::Synced).await;
    let metadata = JobMetadata {
        tags: vec![3],
        rating: Some(2),
        source_hash: None,
        without_rustview: true,
    };
    queue
        .handle
        .request(|reply| QueueCommand::UpdateMetadata {
            id: private,
            metadata,
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    queue.handle.send(QueueCommand::Cancel { ids: vec![private] }).unwrap();
    wait_for(&mut events, private, |row| row.state == JobState::Canceled).await;
    queue
        .handle
        .request(|reply| QueueCommand::Remove {
            ids: vec![private],
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    // The linked job is removed after it: once its stub is gone, everything before it was handled.
    queue
        .handle
        .request(|reply| QueueCommand::Remove {
            ids: vec![linked],
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fake.0.lock().media.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let calls = fake.calls();
    assert!(
        calls
            .iter()
            .all(|c| !c.contains("private") && !c.contains("0123456789abcdef")),
        "{calls:?}"
    );
    assert!(calls.iter().any(|c| c.contains("linked")), "{calls:?}");
}

#[tokio::test]
async fn removed_and_failed_jobs_lose_their_stubs() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let fake = FakeRustView::new();
    let (queue, _sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    let removed = enqueue(&queue, tagged(dir.path(), "removed")).await.unwrap();
    let failing = enqueue(&queue, tagged(dir.path(), "failing")).await.unwrap();
    wait_for(&mut events, failing, |row| row.sync == SyncState::Synced).await;
    assert_eq!(fake.0.lock().media.len(), 2);

    queue
        .handle
        .request(|reply| QueueCommand::Remove {
            ids: vec![removed],
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    // The source is missing: the render fails at once.
    queue.handle.send(QueueCommand::SetPaused(false)).unwrap();
    wait_for(&mut events, failing, |row| row.sync == SyncState::CleanedUp).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fake.0.lock().media.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        store
            .request(|reply| StoreCommand::Job { id: failing, reply })
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .state,
        JobState::Failed { .. }
    ));
    assert!(
        events.try_recv().map_or(true, |e| !matches!(e, QueueEvent::Added(_))),
        "nothing else happened"
    );
}

#[tokio::test]
async fn a_restart_continues_from_the_stored_records() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let fake = FakeRustView::new();
    let (queue, sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    let id = enqueue(&queue, tagged(dir.path(), "a")).await.unwrap();
    wait_for(&mut events, id, |row| row.sync == SyncState::Synced).await;
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
    sync.handle
        .request(|reply| SyncCommand::Shutdown { reply })
        .await
        .unwrap();
    let calls = fake.calls().len();

    // The next start: the waiting job is up to date (nothing called for it); canceling it removes the stub, because the
    // stored record says RustCut created it.
    let (queue, _sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    queue.handle.send(QueueCommand::Cancel { ids: vec![id] }).unwrap();
    wait_for(&mut events, id, |row| row.sync == SyncState::CleanedUp).await;
    let after: Vec<String> = fake.calls().split_off(calls);
    assert!(
        !after.iter().any(|c| c.starts_with("set")),
        "an up-to-date record is not written again: {after:?}"
    );
    assert!(after.iter().any(|c| c.starts_with("delete-media")), "{after:?}");
}

#[tokio::test]
async fn the_metadata_of_waiting_finished_and_failed_jobs_can_be_edited() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let insert = |stem: &'static str, state: JobState| {
        let store = store.clone();
        let spec = tagged(dir.path(), stem);
        async move {
            store
                .request(|reply| StoreCommand::InsertJob { spec, state, reply })
                .await
                .unwrap()
                .unwrap()
                .id
        }
    };
    let finished = insert("done", JobState::Finished).await;
    let failed = insert("failed", JobState::Failed { reason: "test".into() }).await;
    let done_file = dir.path().join("done.mkv");
    std::fs::write(&done_file, b"rendered").unwrap();
    let fake = FakeRustView::new();
    let (queue, _sync) = start(&store, dir.path(), &fake, true);
    let mut events = queue.events.subscribe();
    let waiting = enqueue(&queue, tagged(dir.path(), "waiting")).await.unwrap();
    wait_for(&mut events, waiting, |row| row.sync == SyncState::Synced).await;
    assert!(
        fake.media(&done_file.display().to_string()).is_none(),
        "a finished job that was never synced stays alone (D-V2)"
    );

    let edit = |id: JobId, tags: Vec<u32>, rating: u8| {
        let handle = queue.handle.clone();
        async move {
            let mut metadata = handle
                .request(|reply| QueueCommand::Metadata { id, reply })
                .await
                .unwrap()
                .unwrap();
            metadata.tags = tags;
            metadata.rating = Some(rating);
            handle
                .request(|reply| QueueCommand::UpdateMetadata { id, metadata, reply })
                .await
                .unwrap()
        }
    };
    edit(waiting, vec![7, 8], 2).await.unwrap();
    edit(finished, vec![5], 5).await.unwrap();
    edit(failed, vec![6], 1).await.unwrap();
    let waiting_file = dir.path().join("waiting.mkv").display().to_string();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let w = fake.media(&waiting_file).map(|m| (m.tags, m.rating));
            let f = fake.media(&done_file.display().to_string()).map(|m| (m.tags, m.rating));
            if w == Some((vec![7, 8], 2)) && f == Some((vec![5], 5)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        stored_record(&store, finished).await.finished,
        "the finished file was asserted"
    );
    let failed_file = dir.path().join("failed.mkv").display().to_string();
    assert!(fake.media(&failed_file).is_none(), "a failed job only keeps the values");
    let kept = queue
        .handle
        .request(|reply| QueueCommand::Metadata { id: failed, reply })
        .await
        .unwrap()
        .unwrap();
    assert_eq!((kept.tags, kept.rating), (vec![6], Some(1)));
    let unknown = JobId(999);
    let metadata = JobMetadata::default();
    assert_eq!(
        queue
            .handle
            .request(|reply| QueueCommand::UpdateMetadata {
                id: unknown,
                metadata,
                reply
            })
            .await
            .unwrap(),
        Err(QueueError::NoSuchJob(unknown))
    );
}
