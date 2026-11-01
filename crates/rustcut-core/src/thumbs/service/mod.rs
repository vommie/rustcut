//! The thumbnail service (P12 block 6): one actor that follows the open video, feeds the generator a little at a time, so
//! the pointer and a zoom can always go first, and keeps the job sources in the cache. Keyframes come first; then the
//! refinement decodes where a keyframe is farther away than the spacing.

mod actor;

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actor::{Actor, Phase};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::generator::Arrived;
use super::set::ThumbSet;
use super::store::{ThumbStats, ThumbStoreHandle};
use crate::actor::{EventBus, Handle, Mailbox, Reply as Answer, mailbox};
use crate::settings::ThumbnailSettings;

/// The longer side of a stored picture, whatever size it shows at.
pub const LONG_SIDE: u32 = 320;
const TICK: Duration = Duration::from_millis(100);

/// The video to cover (from the probe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbVideo {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub size: (u32, u32),
    pub aspect: (i32, i32),
}

#[derive(Debug)]
pub enum ThumbCommand {
    Open(ThumbVideo),
    Close,
    /// The pointer is at this place: a keyframe near it goes first.
    Hover {
        ms: u64,
    },
    /// The pointer rests: the exact picture goes first.
    Rest {
        ms: u64,
    },
    /// The timeline shows this part (zoomed).
    View {
        from_ms: u64,
        to_ms: u64,
    },
    Settings {
        settings: ThumbnailSettings,
        render_nice: u8,
    },
    /// A render runs or stopped running.
    Rendering(bool),
    /// The sources of all jobs. `removed` = jobs went away, so the cache follows.
    Jobs {
        sources: HashSet<PathBuf>,
        removed: bool,
    },
    /// The user stops the generation of this video.
    Cancel,
    /// The cache's hold button.
    ClearCache,
    Stats {
        reply: Answer<ThumbStats>,
    },
    Shutdown {
        reply: Answer<()>,
    },
}

pub type ThumbHandle = Handle<ThumbCommand>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbEvent {
    /// The shared state has news (at most every 100 ms).
    Updated,
    /// The helper started fine.
    HelperWorks,
    /// The helper could not start; ffmpeg took over.
    FallbackUsed,
}

/// What the UI reads, with a short lock.
#[derive(Debug, Default)]
pub struct ThumbShared {
    pub path: Option<PathBuf>,
    pub set: ThumbSet,
    pub spacing_ms: u64,
    pub duration_ms: u64,
    /// Places answered and places planned; `running` while the plan is not through.
    pub done: u64,
    pub total: u64,
    pub running: bool,
}

pub struct ThumbConfig {
    pub ffmpeg: PathBuf,
    pub helper: Option<PathBuf>,
    pub settings: ThumbnailSettings,
    pub render_nice: u8,
    pub store: ThumbStoreHandle,
}

pub struct ThumbService {
    pub handle: ThumbHandle,
    pub events: Arc<EventBus<ThumbEvent>>,
    pub shared: Arc<Mutex<ThumbShared>>,
}

async fn run(
    mut actor: Actor,
    mut mailbox: Mailbox<ThumbCommand>,
    mut arrivals: mpsc::UnboundedReceiver<(u64, Arrived)>,
) {
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            command = mailbox.recv() => {
                let Some(command) = command else { return };
                if actor.command(command).await {
                    return;
                }
            }
            Some((run, arrived)) = arrivals.recv() => {
                if run == actor.run {
                    actor.arrived(arrived);
                }
            }
            _ = tick.tick() => actor.tick(),
        }
    }
}

/// Starts the service on the runtime.
pub fn spawn_thumbs(runtime: &tokio::runtime::Handle, config: ThumbConfig) -> ThumbService {
    let (handle, mail) = mailbox();
    let events = Arc::new(EventBus::with_capacity(64));
    let shared = Arc::new(Mutex::new(ThumbShared::default()));
    let (arrivals, incoming) = mpsc::unbounded_channel();
    let now = Instant::now();
    let actor = Actor {
        config,
        shared: Arc::clone(&shared),
        events: Arc::clone(&events),
        arrivals,
        run: 0,
        generator: None,
        video: None,
        wanted: None,
        grid: None,
        phase: Phase::Done,
        pending: VecDeque::new(),
        in_flight: 0,
        asked: HashSet::new(),
        sources: HashSet::new(),
        unsaved: Vec::new(),
        ready: false,
        rendering: false,
        helper_broken: false,
        crashes: 0,
        braked: false,
        slow: 0,
        last_answer: now,
        last_flush: now,
        opened: now,
        dirty: false,
    };
    runtime.spawn(run(actor, mail, incoming));
    ThumbService { handle, events, shared }
}
