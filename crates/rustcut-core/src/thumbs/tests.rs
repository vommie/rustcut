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
    let nothing = fixture
        .store
        .request(|reply| ThumbStoreCommand::Stats { reply })
        .await
        .unwrap();
    assert_eq!(nothing.thumbs, 0, "without a job nothing goes to the cache");

    let sources = HashSet::from([fixture.video.path.clone()]);
    handle
        .send(ThumbCommand::Jobs {
            sources,
            removed: false,
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stored = fixture
        .store
        .request(|reply| ThumbStoreCommand::Stats { reply })
        .await
        .unwrap();
    assert_eq!(stored.thumbs, u64::try_from(count).unwrap());

    handle
        .send(ThumbCommand::Jobs {
            sources: HashSet::new(),
            removed: true,
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let wiped = fixture
        .store
        .request(|reply| ThumbStoreCommand::Stats { reply })
        .await
        .unwrap();
    assert_eq!((wiped.sources, wiped.thumbs), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_duration_of_zero_generates_nothing() {
    let mut fixture = fixture(None);
    fixture.video.duration_ms = 0;
    fixture
        .service
        .handle
        .send(ThumbCommand::Open(fixture.video.clone()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let state = fixture.service.shared.lock();
    assert_eq!((state.total, state.set.len()), (0, 0));
    assert!(state.path.is_none());
}

fn thumb(time_ms: u64) -> Thumb {
    Thumb {
        time_ms,
        exact: false,
        width: 320,
        height: 180,
        webp: Arc::from(&b"x"[..]),
    }
}

async fn stats(store: &ThumbStoreHandle) -> ThumbStats {
    tokio::time::sleep(Duration::from_millis(120)).await;
    store.request(|reply| ThumbStoreCommand::Stats { reply }).await.unwrap()
}

/// Two jobs on one source, one on another: the cache follows only the source no job needs any more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_stays_until_its_last_job_goes_and_the_last_job_empties_everything() {
    let fixture = fixture(None);
    let (a, b) = (PathBuf::from("/videos/a.mkv"), PathBuf::from("/videos/b.mkv"));
    for source in [&a, &b] {
        let put = ThumbStoreCommand::Put {
            source: source.clone(),
            thumbs: vec![thumb(1), thumb(2)],
        };
        fixture.store.send(put).unwrap();
    }
    let orphan = ThumbStoreCommand::Put {
        source: PathBuf::from("/videos/orphan.mkv"),
        thumbs: vec![thumb(1)],
    };
    fixture.store.send(orphan).unwrap();
    let handle = &fixture.service.handle;
    let both = HashSet::from([a.clone(), b.clone()]);
    handle
        .send(ThumbCommand::Jobs {
            sources: both.clone(),
            removed: false,
        })
        .unwrap();
    assert_eq!(stats(&fixture.store).await.sources, 3, "a new job removes nothing");

    // One of a's two jobs goes: the sources did not change, only the orphan falls.
    handle
        .send(ThumbCommand::Jobs {
            sources: both,
            removed: true,
        })
        .unwrap();
    assert_eq!(stats(&fixture.store).await.sources, 2);

    handle
        .send(ThumbCommand::Jobs {
            sources: HashSet::from([b]),
            removed: true,
        })
        .unwrap();
    assert_eq!(stats(&fixture.store).await.sources, 1);

    handle
        .send(ThumbCommand::Jobs {
            sources: HashSet::new(),
            removed: true,
        })
        .unwrap();
    let empty = stats(&fixture.store).await;
    assert_eq!((empty.sources, empty.thumbs), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn emptying_the_cache_keeps_the_open_pictures_and_the_file_stays_empty() {
    let fixture = fixture(Some(PathBuf::from("/nonexistent/rustcut-thumbs")));
    let handle = &fixture.service.handle;
    let sources = HashSet::from([fixture.video.path.clone()]);
    handle
        .send(ThumbCommand::Jobs {
            sources,
            removed: false,
        })
        .unwrap();
    handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    settle(&fixture.service.shared).await;
    assert!(stats(&fixture.store).await.thumbs > 0);
    let shown = fixture.service.shared.lock().set.len();
    handle.send(ThumbCommand::ClearCache).unwrap();
    assert_eq!(stats(&fixture.store).await.thumbs, 0);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(stats(&fixture.store).await.thumbs, 0, "nothing comes back by itself");
    assert_eq!(fixture.service.shared.lock().set.len(), shown);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switching_off_stops_and_switching_on_starts_again() {
    let fixture = fixture(Some(PathBuf::from("/nonexistent/rustcut-thumbs")));
    let handle = &fixture.service.handle;
    handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    settle(&fixture.service.shared).await;
    let settings = |enabled| ThumbCommand::Settings {
        settings: ThumbnailSettings {
            enabled,
            ..ThumbnailSettings::default()
        },
        render_nice: 10,
    };
    handle.send(settings(false)).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(fixture.service.shared.lock().path.is_none());
    handle.send(settings(true)).unwrap();
    settle(&fixture.service.shared).await;
    assert!(fixture.service.shared.lock().set.len() >= 30);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_the_same_video_twice_does_not_start_over() {
    let fixture = fixture(Some(PathBuf::from("/nonexistent/rustcut-thumbs")));
    let mut events = fixture.service.events.subscribe();
    let handle = &fixture.service.handle;
    handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    settle(&fixture.service.shared).await;
    let total = fixture.service.shared.lock().total;
    handle.send(ThumbCommand::Open(fixture.video.clone())).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fixture.service.shared.lock().total, total);
    let mut fallbacks = 0;
    while let Ok(event) = events.try_recv() {
        fallbacks += u32::from(event == ThumbEvent::FallbackUsed);
    }
    assert_eq!(fallbacks, 1);
}
