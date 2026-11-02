//! The queue engine with the real ffmpeg, the real store and PyCutRv's test media (`~/claude/pycutrv-testenv/golden/
//! source/`, or `RUSTCUT_GOLDEN_MEDIA`): two jobs rendered end to end one after the other with their logs stored, two
//! jobs with the same target never at the same time even with free workers, and a render stopped by the shutdown
//! waiting again at the next start. ffmpeg (with libx264) and the media are required; the tests fail with what is
//! missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rustcut_core::edit::{FilterChain, Section, SectionList};
use rustcut_core::job::{JobId, JobMetadata, JobSpec, JobState, JobStateKind, TargetSpec};
use rustcut_core::media::ProbeOptions;
use rustcut_core::queue::{QueueCommand, QueueEvent, QueueOptions, QueueService, STOPPED_NOTE, spawn_queue};
use rustcut_core::render::{
    BitDepth, Container, HardwareContext, RateControl, RenderEnv, RenderPriority, RenderProfile, VideoEncoder,
    WorkDirs, builtin_profiles, temp_path,
};
use rustcut_core::settings::WhenDone;
use rustcut_core::store::{LogStream, Store, StoreHandle, spawn_store};
use rustcut_core::time::Timestamp;
use tokio::sync::broadcast;

fn golden(name: &str) -> PathBuf {
    let dir = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || Path::new(&std::env::var_os("HOME").expect("HOME is set")).join("claude/pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    let path = dir.join(name);
    assert!(
        path.is_file(),
        "the test medium is missing: {} (set RUSTCUT_GOLDEN_MEDIA to the folder with PyCutRv's golden sources)",
        path.display()
    );
    path
}

fn fast_profile(preset: &str) -> RenderProfile {
    let mut p = builtin_profiles()
        .into_iter()
        .find(|p| p.id == "x264-fast-share")
        .unwrap();
    p.video.encoder = VideoEncoder::X264;
    p.video.preset = Some(preset.into());
    p.video.rate_control = RateControl::Quality { value: 30 };
    p.video.bit_depth = BitDepth::Eight;
    p.video.tune = None;
    p.video.profile = None;
    p.container = Container::Mkv;
    p
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("out")).unwrap();
        Self { dir }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn env(&self) -> RenderEnv {
        RenderEnv {
            ffmpeg: PathBuf::from("ffmpeg"),
            probe: ProbeOptions::default(),
            dirs: WorkDirs {
                deshake: self.dir.path().join("data/deshake"),
                passlog: self.dir.path().join("data/passlog"),
            },
            hardware: HardwareContext::default(),
            priority: RenderPriority::default(),
            stall_timeout: Some(Duration::from_secs(120)),
        }
    }

    fn store(&self) -> StoreHandle {
        let store = Store::open(&self.dir.path().join("queue.sqlite"), &self.dir.path().join("backups")).unwrap();
        spawn_store(store).unwrap().0
    }

    fn start(&self, store: StoreHandle, workers: u8, paused: bool) -> QueueService {
        let options = QueueOptions {
            workers,
            paused,
            pause_on_start: false,
            when_done: WhenDone::Nothing,
        };
        spawn_queue(&tokio::runtime::Handle::current(), store, self.env(), options).0
    }

    fn spec(&self, source: &str, sections: &[(i64, i64)], stem: &str, preset: &str) -> Box<JobSpec> {
        let list: SectionList = sections
            .iter()
            .map(|&(a, b)| Section::new(Timestamp::from_millis(a), Timestamp::from_millis(b)).unwrap())
            .collect::<Vec<_>>()
            .into();
        Box::new(JobSpec {
            version: 1,
            source: golden(source),
            source_info: None,
            sections: list,
            filters: FilterChain::default(),
            final_multiple: 2,
            profile: fast_profile(preset),
            target: TargetSpec {
                folder: self.out(),
                stem: stem.into(),
                overwrite: true,
                naming: None,
            },
            metadata: JobMetadata::default(),
        })
    }
}

async fn enqueue(queue: &QueueService, spec: Box<JobSpec>) -> JobId {
    queue
        .handle
        .request(|reply| QueueCommand::Enqueue {
            spec,
            hold: false,
            reply,
        })
        .await
        .unwrap()
        .unwrap()
}

/// Collects events until the queue reports that it is done (or the time is up).
async fn until_done(events: &mut broadcast::Receiver<QueueEvent>, limit: Duration) -> (Vec<QueueEvent>, (u32, u32)) {
    let mut seen = Vec::new();
    let done = tokio::time::timeout(limit, async {
        loop {
            match events.recv().await {
                Ok(QueueEvent::Done { finished, failed }) => return (finished, failed),
                Ok(event) => seen.push(event),
                Err(broadcast::error::RecvError::Lagged(n)) => panic!("{n} events lost"),
                Err(broadcast::error::RecvError::Closed) => panic!("the queue stopped"),
            }
        }
    })
    .await
    .expect("the queue finished in time");
    (seen, done)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_queued_jobs_render_one_after_the_other() {
    let fx = Fixture::new();
    let store = fx.store();
    let queue = fx.start(store.clone(), 1, true);
    let mut events = queue.events.subscribe();
    let a = enqueue(
        &queue,
        fx.spec("basic_h264_720p.mp4", &[(1_000, 2_500)], "first", "ultrafast"),
    )
    .await;
    let b = enqueue(
        &queue,
        fx.spec(
            "multi_audio_2tracks.mkv",
            &[(0, 1_000), (2_000, 3_000)],
            "second",
            "ultrafast",
        ),
    )
    .await;
    queue.handle.send(QueueCommand::SetPaused(false)).unwrap();
    let (seen, done) = until_done(&mut events, Duration::from_secs(180)).await;
    assert_eq!(done, (2, 0));

    // One worker: the second job starts only after the first one ended.
    let rows: Vec<(JobId, JobStateKind)> = seen
        .iter()
        .filter_map(|e| match e {
            QueueEvent::Updated(row) => Some((row.id, row.state.kind())),
            _ => None,
        })
        .collect();
    let first_finished = rows.iter().position(|r| *r == (a, JobStateKind::Finished)).unwrap();
    let second_started = rows.iter().position(|r| *r == (b, JobStateKind::Rendering)).unwrap();
    assert!(first_finished < second_started, "{rows:?}");
    assert!(seen.iter().any(|e| matches!(e, QueueEvent::Progress(p) if p.id == a)));
    assert!(
        seen.iter().any(|e| matches!(e, QueueEvent::Updated(row) if row.id == b
            && matches!(row.state, JobState::Rendering { pass: 1, passes: 1 }))),
        "the pass reaches the row"
    );

    for (id, name) in [(a, "first.mkv"), (b, "second.mkv")] {
        assert!(fx.out().join(name).is_file(), "{name} was written");
        let log = queue
            .handle
            .request(|reply| QueueCommand::Log {
                id,
                limit: 5_000,
                reply,
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            log.iter().any(|l| l.stream == LogStream::Ffmpeg),
            "ffmpeg's lines are stored"
        );
        assert!(log.iter().any(|l| l.line.contains("[Render] Finished")), "{log:?}");
    }
    let snapshot = queue
        .handle
        .request(|reply| QueueCommand::Snapshot { reply })
        .await
        .unwrap();
    assert!(
        snapshot
            .jobs
            .iter()
            .all(|r| r.state == JobState::Finished && r.finished_at.is_some())
    );
    let left: Vec<_> = std::fs::read_dir(fx.out())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 2, "no temp file is left: {left:?}");
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn jobs_with_the_same_target_never_run_together() {
    let fx = Fixture::new();
    let queue = fx.start(fx.store(), 3, true);
    let mut events = queue.events.subscribe();
    let a = enqueue(
        &queue,
        fx.spec("basic_h264_720p.mp4", &[(0, 2_000)], "same", "ultrafast"),
    )
    .await;
    let b = enqueue(
        &queue,
        fx.spec("basic_h264_720p.mp4", &[(2_000, 4_000)], "same", "ultrafast"),
    )
    .await;
    let c = enqueue(
        &queue,
        fx.spec("basic_h264_720p.mp4", &[(0, 1_000)], "other", "ultrafast"),
    )
    .await;
    queue.handle.send(QueueCommand::SetPaused(false)).unwrap();
    let (seen, done) = until_done(&mut events, Duration::from_secs(180)).await;
    assert_eq!(done, (3, 0));
    let mut running: Vec<JobId> = Vec::new();
    let mut together = false;
    let mut c_beside_a = false;
    for event in &seen {
        if let QueueEvent::Processing(p) = event {
            running.clone_from(&p.running);
            together |= running.contains(&a) && running.contains(&b);
            c_beside_a |= running.contains(&a) && running.contains(&c);
        }
    }
    assert!(!together, "the two jobs writing same.mkv overlapped");
    assert!(c_beside_a, "a free worker took the job behind the waiting one");
    assert!(fx.out().join("same.mkv").is_file() && fx.out().join("other.mkv").is_file());
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_render_stopped_at_quit_waits_again() {
    let fx = Fixture::new();
    let store = fx.store();
    let queue = fx.start(store.clone(), 1, false);
    let mut events = queue.events.subscribe();
    let spec = fx.spec("hd_h264_1080p.mp4", &[(0, 60_000)], "long", "veryslow");
    let temp = temp_path(&fx.out(), "long", "mkv");
    let id = enqueue(&queue, spec).await;
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Ok(QueueEvent::Progress(p)) = events.recv().await
                && p.id == id
                && p.done > Timestamp::ZERO
            {
                return;
            }
        }
    })
    .await
    .expect("the render made progress");
    assert!(temp.exists(), "ffmpeg writes the temp file");
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
    assert!(!temp.exists(), "the stopped render removed its temp file");
    let restarted = fx.start(store, 1, true);
    let row = restarted
        .handle
        .request(|reply| QueueCommand::Snapshot { reply })
        .await
        .unwrap()
        .jobs
        .remove(0);
    assert_eq!(
        (row.state, row.note.as_deref(), row.attempt),
        (JobState::Waiting, Some(STOPPED_NOTE), 1)
    );
    assert!(!fx.out().join("long.mkv").exists());
    restarted
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
}

/// P12 block 10: a saved job gets its own picture (WebP), turned like the output and cropped by the job's filters, and
/// keeps it in the database.
#[tokio::test(flavor = "multi_thread")]
async fn a_saved_job_gets_its_own_picture() {
    use rustcut_core::edit::{Crop, Filter, FilterKind};
    let fixture = Fixture::new();
    let queue = fixture.start(fixture.store(), 1, true);
    let picture = |id| {
        let handle = queue.handle.clone();
        async move {
            tokio::time::timeout(
                Duration::from_secs(60),
                handle.request(|reply| QueueCommand::Picture { id, reply }),
            )
            .await
            .unwrap()
            .unwrap()
        }
    };
    let turned = enqueue(
        &queue,
        fixture.spec("rotated_90_metadata.mp4", &[(1_000, 2_000)], "turned", "ultrafast"),
    )
    .await;
    let turned = picture(turned).await.expect("a picture of the turned video");
    assert_eq!(turned.height, 90, "a portrait picture is as high as the room");
    assert!(turned.width < 60, "the source's rotation applies: {}", turned.width);
    assert!(turned.webp.starts_with(b"RIFF"), "stored as WebP");
    let mut square = fixture.spec("hd_h264_1080p.mp4", &[(500, 1_500)], "square", "ultrafast");
    square.filters.set(Filter::Crop(Crop {
        top: 0,
        right: 420,
        bottom: 0,
        left: 420,
    }));
    square.filters.set_enabled(FilterKind::Crop, true);
    let id = enqueue(&queue, square).await;
    let square = picture(id).await.expect("a picture of the cropped job");
    assert_eq!((square.width, square.height), (90, 90), "the job's crop applies");
    let mut missing = fixture.spec("hd_h264_1080p.mp4", &[(500, 1_500)], "missing", "ultrafast");
    missing.source = fixture.dir.path().join("gone.mp4");
    let gone = enqueue(&queue, missing).await;
    assert_eq!(picture(gone).await, None, "no source, no picture");
    queue
        .handle
        .request(|reply| QueueCommand::Shutdown { reply })
        .await
        .unwrap();
    let store = Store::open(
        &fixture.dir.path().join("queue.sqlite"),
        &fixture.dir.path().join("backups"),
    )
    .unwrap();
    assert_eq!(store.picture(id).unwrap(), Some(square), "kept with the job");
}
