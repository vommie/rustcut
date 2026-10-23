//! The one way service updates reach the UI thread (ADR-4). [`UiDispatcher::post`] runs a closure on the Slint event loop
//! in order; [`UiDispatcher::post_latest`] coalesces: of all closures posted under one key within its rate, only the latest
//! runs (playhead ~30 Hz, queue progress ~10 Hz). Coalesced updates must be idempotent states (a position, a progress
//! value), because they may overtake ordered posts.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::runtime::Handle;
use tokio::time::Instant;

/// A closure for the UI thread.
pub type UiJob = Box<dyn FnOnce() + Send + 'static>;

/// Runs closures on the UI thread. The app uses [`SlintSink`]; tests use a recording sink.
pub trait UiSink: Send + Sync + 'static {
    /// `false` when the event loop is gone (the app is quitting).
    fn run_on_ui(&self, job: UiJob) -> bool;
}

/// [`UiSink`] of the running app: `slint::invoke_from_event_loop`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlintSink;

impl UiSink for SlintSink {
    fn run_on_ui(&self, job: UiJob) -> bool {
        slint::invoke_from_event_loop(job).is_ok()
    }
}

/// Identity of a coalesced update: a topic and an id inside it (job id, 0 for singletons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoalesceKey {
    pub topic: &'static str,
    pub id: i64,
}

impl CoalesceKey {
    pub const fn new(topic: &'static str, id: i64) -> Self {
        Self { topic, id }
    }
}

/// Minimum intervals of the coalesced topics.
pub mod rate {
    use std::time::Duration;

    /// Playhead and time labels: about 30 updates per second.
    pub const PLAYHEAD: Duration = Duration::from_millis(33);
    /// Queue progress, hash progress: about 10 updates per second.
    pub const PROGRESS: Duration = Duration::from_millis(100);
}

#[derive(Default)]
struct Slot {
    pending: Option<UiJob>,
    last_flush: Option<Instant>,
    scheduled: bool,
}

struct Inner {
    sink: Box<dyn UiSink>,
    runtime: Handle,
    slots: Mutex<HashMap<CoalesceKey, Slot>>,
}

/// Cheap to clone; usable from any thread (tokio tasks, the mpv event thread, the store thread).
#[derive(Clone)]
pub struct UiDispatcher {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for UiDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiDispatcher").finish_non_exhaustive()
    }
}

impl UiDispatcher {
    /// `runtime` runs the delayed flushes of [`UiDispatcher::post_latest`].
    pub fn new(sink: impl UiSink, runtime: Handle) -> Self {
        Self {
            inner: Arc::new(Inner {
                sink: Box::new(sink),
                runtime,
                slots: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Runs `job` on the UI thread after everything posted before it.
    pub fn post(&self, job: impl FnOnce() + Send + 'static) {
        if !self.inner.sink.run_on_ui(Box::new(job)) {
            tracing::debug!("[UiDispatcher] Event loop is gone; update dropped");
        }
    }

    /// Runs `job` on the UI thread at most once per `min_interval` for `key`; a newer job replaces a pending one.
    pub fn post_latest(&self, key: CoalesceKey, min_interval: Duration, job: impl FnOnce() + Send + 'static) {
        let wait = {
            let mut slots = self.inner.slots.lock();
            let slot = slots.entry(key).or_default();
            slot.pending = Some(Box::new(job));
            if slot.scheduled {
                return;
            }
            slot.scheduled = true;
            slot.last_flush
                .map_or(Duration::ZERO, |t| min_interval.saturating_sub(t.elapsed()))
        };
        if wait.is_zero() {
            self.inner.flush(key);
        } else {
            let inner = Arc::clone(&self.inner);
            self.inner.runtime.spawn(async move {
                tokio::time::sleep(wait).await;
                inner.flush(key);
            });
        }
    }

    /// Drops pending coalesced updates of `key` (e.g. a job row that was removed).
    pub fn forget(&self, key: CoalesceKey) {
        self.inner.slots.lock().remove(&key);
    }
}

impl Inner {
    fn flush(&self, key: CoalesceKey) {
        let job = {
            let mut slots = self.slots.lock();
            let Some(slot) = slots.get_mut(&key) else { return };
            slot.scheduled = false;
            slot.last_flush = Some(Instant::now());
            slot.pending.take()
        };
        if let Some(job) = job
            && !self.sink.run_on_ui(job)
        {
            tracing::debug!(topic = key.topic, "[UiDispatcher] Event loop is gone; update dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Runs every job at once and counts them, like an idle UI thread would.
    #[derive(Clone, Default)]
    struct Inline(Arc<AtomicUsize>);

    impl UiSink for Inline {
        fn run_on_ui(&self, job: UiJob) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst);
            job();
            true
        }
    }

    #[tokio::test(start_paused = true)]
    async fn bursts_are_coalesced_and_the_latest_value_wins() {
        let sink = Inline::default();
        let ui = UiDispatcher::new(sink.clone(), Handle::current());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let key = CoalesceKey::new("progress", 1);
        for value in 0..1000 {
            let seen = Arc::clone(&seen);
            ui.post_latest(key, rate::PROGRESS, move || seen.lock().push(value));
        }
        // The first one runs at once, the rest collapse into one delayed flush.
        assert_eq!(*seen.lock(), vec![0]);
        tokio::time::sleep(rate::PROGRESS * 2).await;
        assert_eq!(*seen.lock(), vec![0, 999]);
        assert_eq!(sink.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn the_rate_is_kept_over_time() {
        let sink = Inline::default();
        let ui = UiDispatcher::new(sink.clone(), Handle::current());
        let key = CoalesceKey::new("playhead", 0);
        // 300 updates over one second at 3 ms spacing -> about 30 runs at 33 ms.
        for _ in 0..300 {
            ui.post_latest(key, rate::PLAYHEAD, || {});
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
        tokio::time::sleep(rate::PLAYHEAD).await;
        let runs = sink.0.load(Ordering::SeqCst);
        assert!((25..=35).contains(&runs), "{runs} runs");
    }

    #[tokio::test(start_paused = true)]
    async fn keys_are_independent_and_ordered_posts_are_never_merged() {
        let sink = Inline::default();
        let ui = UiDispatcher::new(sink.clone(), Handle::current());
        let order = Arc::new(Mutex::new(Vec::new()));
        for i in 0..5 {
            let order = Arc::clone(&order);
            ui.post(move || order.lock().push(i));
        }
        ui.post_latest(CoalesceKey::new("progress", 1), rate::PROGRESS, || {});
        ui.post_latest(CoalesceKey::new("progress", 2), rate::PROGRESS, || {});
        assert_eq!(*order.lock(), vec![0, 1, 2, 3, 4]);
        assert_eq!(sink.0.load(Ordering::SeqCst), 7);
    }

    #[tokio::test(start_paused = true)]
    async fn forgotten_keys_do_not_flush() {
        let sink = Inline::default();
        let ui = UiDispatcher::new(sink.clone(), Handle::current());
        let key = CoalesceKey::new("progress", 9);
        ui.post_latest(key, rate::PROGRESS, || {});
        ui.post_latest(key, rate::PROGRESS, || panic!("a removed row must not be updated"));
        ui.forget(key);
        tokio::time::sleep(rate::PROGRESS * 2).await;
        assert_eq!(sink.0.load(Ordering::SeqCst), 1);
    }
}
