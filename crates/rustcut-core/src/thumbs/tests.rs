//! The service with the real ffmpeg as its fallback, over a test video: no helper is built for the core's tests.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::settings::ThumbnailSettings;

fn media(name: &str) -> PathBuf {
    let root = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap()).join("claude/pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    root.join(name)
}

struct Fixture {
    service: ThumbService,
    store: ThumbStoreHandle,
    video: ThumbVideo,
}

fn fixture(helper: Option<PathBuf>) -> Fixture {
    let (store, _thread) = spawn_thumb_store(ThumbStore::open_in_memory().unwrap()).unwrap();
    let service = spawn_thumbs(
        &tokio::runtime::Handle::current(),
        ThumbConfig {
            ffmpeg: PathBuf::from("ffmpeg"),
            helper,
            settings: ThumbnailSettings::default(),
            render_nice: 10,
            store: store.clone(),
        },
    );
    let video = ThumbVideo {
        path: media("no_audio_h264.mp4"),
        duration_ms: 8000,
        size: (640, 360),
        aspect: (1, 1),
    };
    Fixture { service, store, video }
}

async fn settle(shared: &Arc<parking_lot::Mutex<ThumbShared>>) {
    for _ in 0..600 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = shared.lock();
        if state.total > 0 && !state.running {
            return;
        }
    }
    panic!("the plan did not finish");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ffmpeg_takes_over_when_the_helper_is_missing_and_the_source_of_a_job_is_kept() {
    let fixture = fixture(Some(PathBuf::from("/nonexistent/rustcut-thumbs")));
    let mut events = fixture.service.events.subscribe();
    let handle = &fixture.service.handle;
    handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    settle(&fixture.service.shared).await;
    assert_eq!(events.recv().await.unwrap(), ThumbEvent::FallbackUsed);
    let count = fixture.service.shared.lock().set.len();
    assert!(count >= 30, "{count} pictures");
    let nothing = fixture.store.request(|reply| ThumbStoreCommand::Stats { reply }).await.unwrap();
    assert_eq!(nothing.thumbs, 0, "without a job nothing goes to the cache");

    let sources = HashSet::from([fixture.video.path.clone()]);
    handle.send(ThumbCommand::Jobs { sources, removed: false }).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stored = fixture.store.request(|reply| ThumbStoreCommand::Stats { reply }).await.unwrap();
    assert_eq!(stored.thumbs, u64::try_from(count).unwrap());

    handle.send(ThumbCommand::Jobs { sources: HashSet::new(), removed: true }).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let wiped = fixture.store.request(|reply| ThumbStoreCommand::Stats { reply }).await.unwrap();
    assert_eq!((wiped.sources, wiped.thumbs), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_duration_of_zero_generates_nothing() {
    let mut fixture = fixture(None);
    fixture.video.duration_ms = 0;
    fixture.service.handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let state = fixture.service.shared.lock();
    assert_eq!((state.total, state.set.len()), (0, 0));
    assert!(state.path.is_none());
}
