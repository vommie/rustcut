//! Tests of the actor without ffmpeg: processing stays paused (or the source is missing), so no render runs. Renders
//! with the real ffmpeg are in `tests/queue_golden.rs`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::broadcast;

use super::*;
use crate::job::{JobSpec, sample_spec};
use crate::queue::{JobSummary, MoveTarget, QueueSnapshot};
use crate::render::{HardwareContext, RenderPriority, WorkDirs};
use crate::store::{Store, spawn_store};

pub(super) fn env(dir: &Path) -> RenderEnv {
    RenderEnv {
        ffmpeg: PathBuf::from("ffmpeg"),
        probe: crate::media::ProbeOptions::default(),
        dirs: WorkDirs {
            deshake: dir.join("deshake"),
            passlog: dir.join("passlog"),
        },
        hardware: HardwareContext::default(),
        priority: RenderPriority {
            nice: 0,
            idle_io: false,
        },
        stall_timeout: None,
    }
}

pub(super) fn options(paused: bool) -> QueueOptions {
    QueueOptions {
        workers: 1,
        paused,
        pause_on_start: false,
        when_done: WhenDone::Nothing,
    }
}

pub(super) fn spec_in(dir: &Path, stem: &str) -> Box<JobSpec> {
    let mut spec = sample_spec();
    spec.source = dir.join("missing-source.mp4");
    spec.target.folder = dir.to_path_buf();
    spec.target.stem = stem.into();
    Box::new(spec)
}

pub(super) async fn snapshot(queue: &QueueService) -> QueueSnapshot {
    queue
        .handle
        .request(|reply| QueueCommand::Snapshot { reply })
        .await
        .unwrap()
}

pub(super) async fn enqueue(queue: &QueueService, spec: Box<JobSpec>) -> Result<JobId, QueueError> {
    queue
        .handle
        .request(|reply| QueueCommand::Enqueue {
            spec,
            hold: false,
            reply,
        })
        .await
        .unwrap()
}

/// The next row update of `id` that satisfies `done`.
pub(super) async fn wait_for(
    events: &mut broadcast::Receiver<QueueEvent>,
    id: JobId,
    done: impl Fn(&JobSummary) -> bool,
) -> JobSummary {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(QueueEvent::Updated(row) | QueueEvent::Added(row)) = events.recv().await
                && row.id == id
                && done(&row)
            {
                return *row;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn refused_jobs_and_editing_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let (queue, _task) = spawn_queue(
        &tokio::runtime::Handle::current(),
        store,
        env(dir.path()),
        options(true),
    );

    let mut empty = spec_in(dir.path(), "a");
    empty.sections = crate::edit::SectionList::default();
    assert_eq!(enqueue(&queue, empty).await, Err(QueueError::NoSections));
    let mut same = spec_in(dir.path(), "a");
    same.source = same.target_path();
    assert_eq!(enqueue(&queue, same).await, Err(QueueError::SameSourceAndTarget));
    let mut broken = spec_in(dir.path(), "a");
    broken.profile.container = crate::render::Container::Webm;
    broken.profile.video.encoder = crate::render::VideoEncoder::X264;
    assert!(matches!(
        enqueue(&queue, broken).await,
        Err(QueueError::InvalidProfile(_))
    ));

    let a = enqueue(&queue, spec_in(dir.path(), "a")).await.unwrap();
    let b = enqueue(&queue, spec_in(dir.path(), "b")).await.unwrap();
    let c = enqueue(&queue, spec_in(dir.path(), "c")).await.unwrap();
    queue
        .handle
        .send(QueueCommand::Move {
            ids: vec![c],
            to: MoveTarget::Top,
        })
        .unwrap();
    queue.handle.send(QueueCommand::Hold { ids: vec![b] }).unwrap();
    let mut renamed = spec_in(dir.path(), "a2");
    renamed.target.overwrite = true;
    queue
        .handle
        .request(|reply| QueueCommand::UpdateSpec {
            id: a,
            spec: renamed,
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    queue.handle.send(QueueCommand::Cancel { ids: vec![c] }).unwrap();
    let spec = queue
        .handle
        .request(|reply| QueueCommand::Spec { id: a, reply })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (spec.target.stem.as_str(), spec.target.overwrite),
        ("a2", true),
        "the whole updated spec comes back"
    );
    let missing = queue
        .handle
        .request(|reply| QueueCommand::Spec { id: JobId(999), reply })
        .await
        .unwrap();
    assert_eq!(missing, Err(QueueError::NoSuchJob(JobId(999))));
    let snap = snapshot(&queue).await;
    let rows: Vec<(JobId, JobState, &str)> = snap
        .jobs
        .iter()
        .map(|r| (r.id, r.state.clone(), r.name.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (c, JobState::Canceled, "c"),
            (a, JobState::Waiting, "a2"),
            (b, JobState::Held, "b")
        ]
    );
    assert!(snap.processing.paused && snap.processing.running.is_empty());
    let copies = queue
        .handle
        .request(|reply| QueueCommand::Duplicate { ids: vec![a], reply })
        .await
        .unwrap()
        .unwrap();
    let order: Vec<JobId> = snapshot(&queue).await.jobs.iter().map(|r| r.id).collect();
    assert_eq!(order, vec![c, a, copies[0], b], "a copy follows its original");
    let removed = queue
        .handle
        .request(|reply| QueueCommand::Remove {
            ids: vec![c, copies[0]],
            reply,
        })
        .await
        .unwrap();
    assert_eq!(removed, Ok(2));
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

#[tokio::test]
async fn crashed_jobs_wait_again_and_their_leftovers_go() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queue.sqlite");
    let spec = spec_in(dir.path(), "clip");
    let temp = crate::render::temp_path(dir.path(), "clip", spec.profile.container.extension());
    let id = {
        let mut store = Store::open(&path, &dir.path().join("backups")).unwrap();
        let id = store.insert_job(&spec, &JobState::Waiting).unwrap().id;
        store
            .set_state(id, &JobState::Rendering { pass: 1, passes: 2 }, None)
            .unwrap();
        id
    };
    // What a killed render left: the temp file, the transform and the two-pass statistics.
    std::fs::write(&temp, b"half").unwrap();
    std::fs::create_dir_all(dir.path().join("deshake")).unwrap();
    std::fs::write(dir.path().join(format!("deshake/job-{}.trf", id.0)), b"t").unwrap();
    std::fs::create_dir_all(dir.path().join(format!("passlog/job-{}", id.0))).unwrap();
    std::fs::write(spec.target_path(), b"an older output").unwrap();

    let (store, _thread) = spawn_store(Store::open(&path, &dir.path().join("backups")).unwrap()).unwrap();
    let (queue, _task) = spawn_queue(
        &tokio::runtime::Handle::current(),
        store,
        env(dir.path()),
        options(true),
    );
    let row = snapshot(&queue).await.jobs.remove(0);
    assert_eq!(
        (row.id, row.state, row.note.as_deref()),
        (id, JobState::Waiting, Some(RECOVERED_NOTE))
    );
    assert!(!temp.exists(), "the stale temp file is deleted");
    assert!(!dir.path().join(format!("deshake/job-{}.trf", id.0)).exists());
    assert!(!dir.path().join(format!("passlog/job-{}", id.0)).exists());
    assert!(spec.target_path().exists(), "the target is never touched");
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_failing_job_never_stalls_the_queue() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let (queue, _task) = spawn_queue(
        &tokio::runtime::Handle::current(),
        store,
        env(dir.path()),
        options(true),
    );
    let mut events = queue.events.subscribe();
    // Both sources are missing: each render fails at its probe, and the second still runs (PyCutRv bug 9).
    let a = enqueue(&queue, spec_in(dir.path(), "a")).await.unwrap();
    let b = enqueue(&queue, spec_in(dir.path(), "b")).await.unwrap();
    queue.handle.send(QueueCommand::SetPaused(false)).unwrap();
    let failed = |r: &JobSummary| r.state.kind() == crate::job::JobStateKind::Failed;
    let row_a = wait_for(&mut events, a, failed).await;
    let row_b = wait_for(&mut events, b, failed).await;
    assert!(
        matches!(&row_a.state, JobState::Failed { reason } if reason.contains("source")),
        "{:?}",
        row_a.state
    );
    assert!(row_b.started_at.is_some() && row_b.finished_at.is_some());
    let done = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(QueueEvent::Done { finished, failed }) = events.recv().await {
                return (finished, failed);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(done, (0, 2));
    let log = queue
        .handle
        .request(|reply| QueueCommand::Log {
            id: a,
            limit: 50,
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(log.iter().any(|l| l.line.contains("Attempt 1")), "{log:?}");
    // A retry is a new attempt.
    queue.handle.send(QueueCommand::SetPaused(true)).unwrap();
    queue
        .handle
        .send(QueueCommand::Restart {
            ids: vec![a],
            profile: None,
        })
        .unwrap();
    let row = snapshot(&queue).await.jobs.remove(0);
    assert_eq!((row.state, row.attempt), (JobState::Waiting, 2));
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_replaced_store_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _thread) = spawn_store(Store::open_in_memory().unwrap()).unwrap();
    let (queue, _task) = spawn_queue(
        &tokio::runtime::Handle::current(),
        store.clone(),
        env(dir.path()),
        options(true),
    );
    assert!(snapshot(&queue).await.jobs.is_empty());
    // The database dialog rescued two jobs into another database.
    let mut rescued = Store::open_in_memory().unwrap();
    for stem in ["a", "b"] {
        rescued
            .insert_job(&spec_in(dir.path(), stem), &JobState::Waiting)
            .unwrap();
    }
    store
        .request(|reply| StoreCommand::Replace {
            store: Box::new(rescued),
            reply,
        })
        .await
        .unwrap();
    let mut events = queue.events.subscribe();
    let loaded = queue
        .handle
        .request(|reply| QueueCommand::Reload { reply })
        .await
        .unwrap();
    assert_eq!(loaded, Ok(2));
    assert_eq!(snapshot(&queue).await.jobs.len(), 2);
    let mut added = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, QueueEvent::Added(_)) {
            added += 1;
        }
    }
    assert_eq!(added, 2, "the panel learns about the jobs");
}
