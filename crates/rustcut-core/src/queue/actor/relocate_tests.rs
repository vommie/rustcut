//! Moving targets (P12 block 7) with the in-memory RustView and real files in a temp folder, no ffmpeg: a finished job's
//! file and record move (tags and rating kept, the record is never deleted and set), a waiting job's target and stub move,
//! RustView failing leaves the record alone and says so, and a move a crash cut short is settled at the next start.

use std::path::{Path, PathBuf};

use super::tests::{enqueue, env, options, spec_in, wait_for};
use super::*;
use crate::job::{JobMetadata, JobSpec};
use crate::queue::{QueueEvent, Relocated, Relocation, RelocationOutcome, TargetMove};
use crate::rustview::fake::FakeRustView;
use crate::rustview::{JobSyncRecord, RustViewError, SyncState, spawn_sync};
use crate::settings::RustViewSettings;
use crate::store::{Store, spawn_store};

fn tagged(dir: &Path, stem: &str) -> JobSpec {
    let mut spec = *spec_in(dir, stem);
    spec.metadata = JobMetadata {
        tags: vec![2],
        rating: Some(4),
        source_hash: Some("8f618d1ebd606025257057ee18536e64".into()),
        without_rustview: false,
    };
    spec
}

/// A store with one finished job whose file exists and whose record RustCut wrote (RustView holds it with tags 2 and 7:
/// the 7 was added in RustView after the render).
fn finished_job(dir: &Path, fake: &FakeRustView, relocation: Option<&Relocation>) -> (Store, JobId, PathBuf) {
    let mut store = Store::open_in_memory().unwrap();
    let spec = tagged(dir, "clip");
    let target = spec.target_path();
    std::fs::write(&target, b"rendered video").unwrap();
    let record = store.insert_job(&spec, &JobState::Finished).unwrap();
    let sync = JobSyncRecord {
        state: SyncState::Synced,
        synced_path: Some(target.clone()),
        synced: Some(spec.metadata.clone()),
        preexisting: Some(false),
        finished: true,
        ..JobSyncRecord::default()
    };
    store.set_sync(record.id, sync.to_column().as_deref()).unwrap();
    if let Some(journal) = relocation {
        store.set_relocation(record.id, journal.to_column().as_deref()).unwrap();
    }
    fake.put(&target.display().to_string(), &[2, 7], 5, Some("RustCut"));
    (store, record.id, target)
}

fn start(store: Store, dir: &Path, fake: &FakeRustView) -> (QueueService, StoreHandle) {
    let (store, _thread) = spawn_store(store).unwrap();
    let client = fake.clone();
    let runtime = tokio::runtime::Handle::current();
    let (sync, records, _) = spawn_sync(&runtime, RustViewSettings::default(), move |_| client.clone());
    let link = SyncLink {
        handle: sync.handle,
        records,
    };
    let (queue, _) = spawn_queue_linked(&runtime, store.clone(), env(dir), options(true), Some(link));
    (queue, store)
}

async fn outcome(events: &mut tokio::sync::broadcast::Receiver<QueueEvent>, id: JobId) -> RelocationOutcome {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(QueueEvent::Relocated(outcome)) = events.recv().await
                && outcome.id == id
            {
                return *outcome;
            }
        }
    })
    .await
    .unwrap()
}

async fn move_to(queue: &QueueService, id: JobId, folder: &Path, stem: &str) -> usize {
    queue
        .handle
        .request(|reply| QueueCommand::MoveTargets {
            moves: vec![TargetMove {
                id,
                folder: folder.to_path_buf(),
                stem: stem.into(),
                overwrite: false,
            }],
            reply,
        })
        .await
        .unwrap()
        .unwrap()
}

async fn row(store: &StoreHandle, id: JobId) -> crate::store::JobRecord {
    store
        .request(|reply| StoreCommand::Job { id, reply })
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_finished_file_and_its_record_move_with_tags_and_rating() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let (store, id, old) = finished_job(dir.path(), &fake, None);
    let (queue, store) = start(store, dir.path(), &fake);
    let mut events = queue.events.subscribe();
    let folder = dir.path().join("videos/2026");
    assert_eq!(move_to(&queue, id, &folder, "Beach").await, 1);
    let done = outcome(&mut events, id).await;
    assert_eq!((done.result, done.record_error), (Ok(Relocated::Renamed), None));
    let new = folder.join(old.file_name().unwrap().to_string_lossy().replace("clip", "Beach"));
    assert!(!old.exists());
    assert_eq!(std::fs::read(&new).unwrap(), b"rendered video");
    let stored = row(&store, id).await;
    assert_eq!(stored.target_path, new);
    assert_eq!(stored.relocation, None, "the journal is cleared");
    let record = fake.media(&new.display().to_string()).unwrap();
    assert_eq!(
        (record.tags, record.rating),
        (vec![2, 7], 5),
        "RustView's own values moved along"
    );
    assert!(fake.media(&old.display().to_string()).is_none());
    assert!(
        !fake
            .writes()
            .iter()
            .any(|w| w.starts_with("delete") || w.starts_with("set")),
        "moved, never deleted and set: {:?}",
        fake.writes()
    );
    assert_eq!(
        JobSyncRecord::from_column(stored.sync.as_deref()).synced_path,
        Some(new.clone())
    );
    let log = store
        .request(|reply| StoreCommand::Log { id, limit: 10, reply })
        .await
        .unwrap()
        .unwrap();
    assert!(log.iter().any(|l| l.line.starts_with("[Move] Moved from")), "{log:?}");
}

#[tokio::test]
async fn a_waiting_job_changes_its_target_and_its_stub_follows() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let (queue, store) = start(Store::open_in_memory().unwrap(), dir.path(), &fake);
    let mut events = queue.events.subscribe();
    let id = enqueue(&queue, Box::new(tagged(dir.path(), "a"))).await.unwrap();
    wait_for(&mut events, id, |r| r.sync == SyncState::Synced).await;
    let stub_id = fake.media(&dir.path().join("a.mkv").display().to_string()).unwrap().id;
    let folder = dir.path().join("elsewhere");
    move_to(&queue, id, &folder, "a").await;
    let done = outcome(&mut events, id).await;
    assert_eq!((done.result, done.record_error), (Ok(Relocated::TargetChanged), None));
    let moved = fake.media(&folder.join("a.mkv").display().to_string()).unwrap();
    assert_eq!(moved.id, stub_id, "the same record, not a new one");
    assert!(!folder.exists(), "nothing rendered yet: no folder, no file");
    assert_eq!(row(&store, id).await.target_path, folder.join("a.mkv"));
}

#[tokio::test]
async fn rustview_failing_is_reported_and_the_record_is_left_to_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let (store, id, old) = finished_job(dir.path(), &fake, None);
    let (queue, store) = start(store, dir.path(), &fake);
    let mut events = queue.events.subscribe();
    // Let the start's calls (the tag tree) through, then RustView cannot be reached.
    tokio::time::sleep(Duration::from_millis(300)).await;
    fake.fail_next(RustViewError::Api("the database is locked".into()));
    let folder = dir.path().join("moved");
    move_to(&queue, id, &folder, "clip").await;
    let done = outcome(&mut events, id).await;
    assert_eq!(done.result, Ok(Relocated::Renamed), "the file moves anyway");
    assert!(done.record_error.unwrap().contains("locked"));
    assert!(
        fake.media(&old.display().to_string()).is_some(),
        "the record stays for the user to move"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !fake
            .writes()
            .iter()
            .any(|w| w.starts_with("delete") || w.starts_with("set")),
        "never deleted and set instead: {:?}",
        fake.writes()
    );
    let stored = row(&store, id).await;
    assert_eq!(
        JobSyncRecord::from_column(stored.sync.as_deref()).synced_path,
        Some(folder.join(old.file_name().unwrap()))
    );
}

#[tokio::test]
async fn an_interrupted_move_is_finished_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let to = dir.path().join("new/clip.mkv");
    let journal = Relocation {
        from: dir.path().join("clip.mkv"),
        to: to.clone(),
        overwrite: false,
        after_render: false,
        restore_overwrite: false,
    };
    // The crash came after the rename: the file is at the new place, the job still points at the old one.
    let (store, id, old) = finished_job(dir.path(), &fake, Some(&journal));
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::rename(&old, &to).unwrap();
    let (queue, store) = start(store, dir.path(), &fake);
    let mut events = queue.events.subscribe();
    tokio::time::timeout(Duration::from_secs(20), async {
        while row(&store, id).await.relocation.is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(row(&store, id).await.target_path, to);
    let _ = &mut events;
    tokio::time::timeout(Duration::from_secs(20), async {
        while fake.media(&to.display().to_string()).is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fake.media(&to.display().to_string()).unwrap().tags, vec![2, 7]);
}

#[tokio::test]
async fn an_interrupted_copy_starts_again_and_its_partial_copy_goes() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let to = dir.path().join("new/clip.mkv");
    let journal = Relocation {
        from: dir.path().join("clip.mkv"),
        to: to.clone(),
        overwrite: false,
        after_render: false,
        restore_overwrite: false,
    };
    let (store, id, old) = finished_job(dir.path(), &fake, Some(&journal));
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(crate::files::temp_path(&to), b"rend").unwrap();
    let (queue, store) = start(store, dir.path(), &fake);
    let _keep = queue;
    tokio::time::timeout(Duration::from_secs(20), async {
        while row(&store, id).await.relocation.is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(!old.exists() && !crate::files::temp_path(&to).exists());
    assert_eq!(std::fs::read(&to).unwrap(), b"rendered video");
}

#[tokio::test]
async fn an_existing_file_at_the_new_place_stops_the_move() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeRustView::new();
    let (store, id, old) = finished_job(dir.path(), &fake, None);
    let folder = dir.path().join("taken");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(old.file_name().unwrap()), b"someone else's").unwrap();
    let (queue, store) = start(store, dir.path(), &fake);
    let mut events = queue.events.subscribe();
    move_to(&queue, id, &folder, "clip").await;
    let done = outcome(&mut events, id).await;
    assert!(done.result.unwrap_err().contains("exists already"));
    assert!(old.exists());
    assert_eq!(
        std::fs::read(folder.join(old.file_name().unwrap())).unwrap(),
        b"someone else's"
    );
    let stored = row(&store, id).await;
    assert_eq!((stored.target_path, stored.relocation), (old, None));
}
