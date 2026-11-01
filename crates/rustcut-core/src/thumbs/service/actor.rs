//! The service's state and what it does with commands and replies.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustcut_thumbs_protocol::{Job, Reply, Start};
use tokio::sync::mpsc;

use super::{LONG_SIDE, ThumbCommand, ThumbConfig, ThumbEvent, ThumbShared, ThumbVideo};
use crate::actor::EventBus;
use crate::settings::ThumbnailSettings;
use crate::thumbs::generator::{Arrived, Fallback, FallbackVideo, Generator, Helper};
use crate::thumbs::plan::{Grid, automatic_workers, niceness};
use crate::thumbs::set::{Thumb, ThumbSet};
use crate::thumbs::store::ThumbStoreCommand;

/// Near enough to count as the exact picture of a place.
const EXACT_MS: u64 = 40;
/// A thumbnail that takes longer than this, a few times in a row, means a slow drive.
const SLOW: Duration = Duration::from_millis(500);
const SLOW_IN_A_ROW: u32 = 3;
/// With nothing to do, the helper ends after this (its decoders hold memory) and comes back at the next hover.
const IDLE_END: Duration = Duration::from_secs(30);
const FLUSH_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Keyframes,
    Refinement,
    Done,
}

pub(super) struct Actor {
    pub(super) config: ThumbConfig,
    pub(super) shared: Arc<Mutex<ThumbShared>>,
    pub(super) events: Arc<EventBus<ThumbEvent>>,
    pub(super) arrivals: mpsc::UnboundedSender<(u64, Arrived)>,
    pub(super) run: u64,
    pub(super) generator: Option<Generator>,
    pub(super) video: Option<ThumbVideo>,
    /// The open video, also while thumbnails are off: switching them on starts with it.
    pub(super) wanted: Option<ThumbVideo>,
    pub(super) grid: Option<Grid>,
    pub(super) phase: Phase,
    pub(super) pending: VecDeque<Job>,
    pub(super) in_flight: usize,
    /// Places asked for out of line; their answers do not count for the plan.
    pub(super) asked: HashSet<u64>,
    pub(super) sources: HashSet<PathBuf>,
    /// New pictures of a source with a job, on their way to the cache.
    pub(super) unsaved: Vec<Thumb>,
    pub(super) ready: bool,
    pub(super) rendering: bool,
    pub(super) helper_broken: bool,
    pub(super) crashes: u32,
    pub(super) braked: bool,
    pub(super) slow: u32,
    pub(super) last_answer: Instant,
    pub(super) last_flush: Instant,
    /// When the open video's first plan started, for the log.
    pub(super) opened: Instant,
    pub(super) dirty: bool,
}

fn places(job: &Job) -> usize {
    match job {
        Job::Key { .. } | Job::Exact { .. } => 1,
        Job::Span { targets, .. } => targets.len(),
    }
}

impl Actor {
    fn workers(&self) -> u32 {
        if self.braked {
            return 1;
        }
        match self.config.settings.workers {
            0 => automatic_workers(std::thread::available_parallelism().map_or(2, usize::from)),
            count => u32::from(count),
        }
    }

    fn nice(&self) -> i32 {
        niceness(self.config.settings.nice, self.config.render_nice, self.rendering)
    }

    /// Starts the generator for the open video, unless it runs.
    fn start(&mut self) {
        if self.generator.is_some() {
            return;
        }
        let Some(video) = self.video.clone() else { return };
        self.run += 1;
        self.ready = false;
        self.in_flight = 0;
        let start = video.path.to_str().map(|path| Start {
            path: path.to_owned(),
            long_side: LONG_SIDE,
            quality: self.config.settings.quality,
            workers: self.workers(),
            nice: self.nice(),
        });
        // libav's Rust side takes only UTF-8 paths, so such a file goes the long way without bothering anyone.
        let program = self.config.helper.clone().filter(|_| !self.helper_broken);
        if let (Some(start), Some(program)) = (&start, &program) {
            match Helper::start(program, start, self.run, self.arrivals.clone()) {
                Ok(helper) => {
                    self.generator = Some(Generator::Helper(helper));
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, helper = %program.display(), "[Thumbs] The helper did not start");
                    self.helper_broken = true;
                }
            }
        }
        if start.is_some() {
            self.events.publish(ThumbEvent::FallbackUsed);
        }
        self.start_fallback(video);
    }

    fn start_fallback(&mut self, video: ThumbVideo) {
        let fallback = FallbackVideo {
            ffmpeg: self.config.ffmpeg.clone(),
            path: video.path,
            size: video.size,
            aspect: video.aspect,
            long_side: LONG_SIDE,
            quality: self.config.settings.quality,
        };
        self.generator = Some(Generator::Fallback(Fallback::start(fallback, self.run, &self.arrivals)));
    }

    fn stop(&mut self) {
        self.generator = None;
        self.run += 1;
        self.in_flight = 0;
        self.asked.clear();
        self.ready = false;
    }

    /// Fills the generator's line up to a small number of places, so anything new can still go first.
    fn refill(&mut self) {
        if !self.ready {
            return;
        }
        let limit = if self.braked {
            1
        } else {
            usize::try_from(self.workers()).unwrap_or(1).saturating_mul(4)
        };
        let mut jobs = Vec::new();
        while self.in_flight < limit {
            let Some(job) = self.pending.pop_front() else { break };
            self.in_flight += places(&job);
            jobs.push(job);
        }
        if !jobs.is_empty()
            && let Some(generator) = &mut self.generator
        {
            generator.add(jobs);
        }
    }

    fn set_running(&mut self, running: bool) {
        self.shared.lock().running = running;
        self.dirty = true;
    }

    /// The plan of the current phase from what exists.
    fn plan(&mut self) {
        let Some(grid) = self.grid else { return };
        let jobs = {
            let shared = self.shared.lock();
            match self.phase {
                Phase::Keyframes => grid.keyframes(&shared.set),
                Phase::Refinement => grid.refinement(&shared.set),
                Phase::Done => Vec::new(),
            }
        };
        let count = jobs.iter().map(places).sum::<usize>();
        self.shared.lock().total += u64::try_from(count).unwrap_or(0);
        self.pending = jobs.into();
    }

    /// The line ran dry: on to the next phase.
    fn advance(&mut self) {
        if !self.pending.is_empty() || self.in_flight > 0 {
            return;
        }
        match self.phase {
            Phase::Keyframes if !self.braked => {
                let pictures = self.shared.lock().set.len();
                tracing::info!(
                    pictures,
                    ms = self.opened.elapsed().as_millis(),
                    "[Thumbs] Keyframes done"
                );
                self.phase = Phase::Refinement;
                self.plan();
                if self.pending.is_empty() {
                    self.finish();
                } else {
                    self.refill();
                }
            }
            Phase::Keyframes | Phase::Refinement => self.finish(),
            Phase::Done => {}
        }
    }

    fn finish(&mut self) {
        let pictures = self.shared.lock().set.len();
        tracing::info!(pictures, ms = self.opened.elapsed().as_millis(), "[Thumbs] Finished");
        self.phase = Phase::Done;
        self.set_running(false);
        self.flush();
    }

    async fn open(&mut self, video: ThumbVideo) {
        if self
            .video
            .as_ref()
            .is_some_and(|known| known.path == video.path && known.duration_ms == video.duration_ms)
        {
            return;
        }
        self.close();
        self.wanted = Some(video.clone());
        if !self.config.settings.enabled || video.duration_ms == 0 {
            return;
        }
        let grid = Grid::new(video.duration_ms, self.config.settings.density.factor());
        let mut set = ThumbSet::default();
        if self.sources.contains(&video.path) {
            let path = video.path.clone();
            let cached = self
                .config
                .store
                .request(|reply| ThumbStoreCommand::Load { source: path, reply })
                .await;
            for thumb in cached.unwrap_or_default() {
                set.insert(thumb);
            }
        }
        {
            let mut shared = self.shared.lock();
            *shared = ThumbShared {
                path: Some(video.path.clone()),
                set,
                spacing_ms: grid.spacing_ms,
                duration_ms: video.duration_ms,
                done: 0,
                total: 0,
                running: true,
            };
        }
        tracing::debug!(
            spacing_ms = grid.spacing_ms,
            places = grid.count(),
            "[Thumbs] Video opened"
        );
        self.video = Some(video);
        self.grid = Some(grid);
        self.phase = Phase::Keyframes;
        self.opened = Instant::now();
        self.crashes = 0;
        self.braked = false;
        self.slow = 0;
        self.dirty = true;
        self.plan();
        self.start();
    }

    fn close(&mut self) {
        self.flush();
        self.stop();
        self.video = None;
        self.wanted = None;
        self.grid = None;
        self.pending.clear();
        self.phase = Phase::Done;
        *self.shared.lock() = ThumbShared::default();
        self.dirty = true;
    }

    /// Starts over with the same pictures (a change of priority, workers or density).
    fn restart(&mut self) {
        if self.video.is_none() {
            return;
        }
        let was_done = self.phase == Phase::Done;
        self.stop();
        self.pending.clear();
        if !was_done {
            let done = self.shared.lock().done;
            self.shared.lock().total = done;
            self.plan();
            self.start();
        }
    }

    fn flush(&mut self) {
        self.last_flush = Instant::now();
        if self.unsaved.is_empty() {
            return;
        }
        let thumbs = std::mem::take(&mut self.unsaved);
        if let Some(video) = &self.video {
            let source = video.path.clone();
            let _ = self.config.store.send(ThumbStoreCommand::Put { source, thumbs });
        }
    }

    fn keeps(&self, path: &Path) -> bool {
        self.sources.contains(path)
    }

    fn answered(&mut self, target_ms: u64) {
        if !self.asked.remove(&target_ms) {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.shared.lock().done += 1;
        }
        self.dirty = true;
    }

    fn thumb(&mut self, thumb: Thumb) {
        let now = Instant::now();
        if self.phase == Phase::Keyframes && !self.braked {
            self.slow = if now.duration_since(self.last_answer) > SLOW {
                self.slow + 1
            } else {
                0
            };
            if self.slow >= SLOW_IN_A_ROW {
                tracing::info!("[Thumbs] A slow drive: one worker and no refinement for this video");
                self.braked = true;
                self.last_answer = now;
                self.shared.lock().set.insert(thumb);
                self.restart();
                return;
            }
        }
        self.last_answer = now;
        let count = self.shared.lock().set.len() + 1;
        if count == 1 || count == 5 {
            tracing::info!(count, ms = self.opened.elapsed().as_millis(), "[Thumbs] First pictures");
        }
        if self.video.as_ref().is_some_and(|video| self.keeps(&video.path)) {
            self.unsaved.push(thumb.clone());
        }
        self.shared.lock().set.insert(thumb);
    }

    pub(super) fn arrived(&mut self, arrived: Arrived) {
        match arrived {
            Arrived::Reply(reply, picture) => self.reply(reply, picture),
            Arrived::Closed => self.helper_gone(),
        }
    }

    fn reply(&mut self, reply: Reply, picture: Vec<u8>) {
        match reply {
            Reply::Ready { .. } => {
                self.ready = true;
                self.last_answer = Instant::now();
                if matches!(self.generator, Some(Generator::Helper(_))) {
                    self.events.publish(ThumbEvent::HelperWorks);
                }
                self.refill();
                // A plan the cache covered sends nothing, so no `Idle` would come.
                self.advance();
            }
            Reply::Thumb {
                target_ms,
                time_ms,
                exact,
                width,
                height,
            } => {
                self.thumb(Thumb {
                    time_ms,
                    exact,
                    width,
                    height,
                    webp: Arc::from(picture),
                });
                self.answered(target_ms);
                self.refill();
            }
            Reply::Known { target_ms, .. } | Reply::Missing { target_ms } => {
                self.answered(target_ms);
                self.refill();
            }
            Reply::Idle => self.advance(),
            Reply::Failed { message } => {
                // The helper runs, but this file is nothing it can open: no video to show thumbnails of.
                tracing::info!(%message, "[Thumbs] No thumbnails for this file");
                self.stop();
                self.pending.clear();
                self.finish();
            }
        }
    }

    /// The helper's output ended without anyone asking.
    fn helper_gone(&mut self) {
        let started = self.ready;
        self.generator = None;
        self.ready = false;
        self.in_flight = 0;
        self.asked.clear();
        if started {
            self.crashes += 1;
            tracing::warn!(crashes = self.crashes, "[Thumbs] The helper ended (a damaged file?)");
            if self.crashes > 1 {
                self.pending.clear();
                self.finish();
                return;
            }
        } else {
            // It never answered: a library it needs is gone (an updated FFmpeg). Thumbnails go on through ffmpeg.
            tracing::warn!("[Thumbs] The helper ended before it answered");
            self.helper_broken = true;
            self.events.publish(ThumbEvent::FallbackUsed);
        }
        self.pending.clear();
        if self.phase != Phase::Done {
            self.plan();
            self.start();
        }
    }

    fn ask(&mut self, jobs: Vec<Job>, targets: impl IntoIterator<Item = u64>) {
        // A file that crashed the helper twice stays without it.
        if jobs.is_empty() || self.crashes > 1 {
            return;
        }
        self.start();
        self.asked.extend(targets);
        if let Some(generator) = &mut self.generator {
            generator.front(jobs);
        }
    }

    fn hover(&mut self, ms: u64) {
        let Some(grid) = self.grid else { return };
        let far = self.shared.lock().set.gap(ms).is_none_or(|gap| gap > grid.spacing_ms);
        if far && !self.asked.contains(&ms) {
            self.ask(vec![Job::Key { ms }], [ms]);
        }
    }

    fn rest(&mut self, ms: u64) {
        if self.grid.is_none() {
            return;
        }
        let near = self
            .shared
            .lock()
            .set
            .nearest(ms)
            .is_some_and(|thumb| thumb.exact && thumb.time_ms.abs_diff(ms) <= EXACT_MS);
        if !near {
            self.ask(vec![Job::Exact { ms }], [ms]);
        }
    }

    fn view(&mut self, from_ms: u64, to_ms: u64) {
        let Some(grid) = self.grid else { return };
        let jobs = grid.view(from_ms, to_ms, &self.shared.lock().set);
        if jobs.is_empty() || self.crashes > 1 {
            return;
        }
        self.start();
        let count = jobs.iter().map(places).sum::<usize>();
        self.shared.lock().total += u64::try_from(count).unwrap_or(0);
        for job in jobs.into_iter().rev() {
            self.pending.push_front(job);
        }
        if self.phase == Phase::Done {
            self.phase = Phase::Refinement;
            self.set_running(true);
        }
        self.refill();
    }

    async fn settings(&mut self, settings: ThumbnailSettings, render_nice: u8) {
        let before = std::mem::replace(&mut self.config.settings, settings);
        self.config.render_nice = render_nice;
        let now = self.config.settings.clone();
        if before.enabled && !now.enabled {
            let wanted = self.wanted.clone();
            self.close();
            self.wanted = wanted;
            return;
        }
        if !before.enabled && now.enabled {
            if let Some(video) = self.wanted.clone() {
                self.open(video).await;
            }
            return;
        }
        if before.density != now.density && self.video.is_some() {
            let video = self.video.clone();
            self.close();
            if let Some(video) = video {
                self.open(video).await;
            }
            return;
        }
        if (before.workers, before.nice, before.quality) != (now.workers, now.nice, now.quality) {
            self.restart();
        }
    }

    fn rendering(&mut self, rendering: bool) {
        let before = self.nice();
        self.rendering = rendering;
        if before != self.nice() {
            self.restart();
        }
    }

    fn jobs(&mut self, sources: HashSet<PathBuf>, removed: bool) {
        let open = self.video.as_ref().map(|video| video.path.clone());
        if let Some(path) = open
            && sources.contains(&path)
            && !self.sources.contains(&path)
        {
            let thumbs: Vec<Thumb> = self.shared.lock().set.thumbs().cloned().collect();
            let _ = self.config.store.send(ThumbStoreCommand::Put { source: path, thumbs });
        }
        if removed {
            self.flush();
            if sources.is_empty() {
                let _ = self.config.store.send(ThumbStoreCommand::Wipe);
            } else {
                let _ = self
                    .config
                    .store
                    .send(ThumbStoreCommand::Retain { keep: sources.clone() });
            }
        }
        self.sources = sources;
    }

    fn cancel(&mut self) {
        self.pending.clear();
        if let Some(generator) = &mut self.generator {
            generator.clear();
        }
        self.finish();
    }

    /// Empties the file. The open video keeps its pictures in memory, so the hover goes on and the file stays empty.
    fn clear_cache(&mut self) {
        self.unsaved.clear();
        let _ = self.config.store.send(ThumbStoreCommand::Wipe);
    }

    pub(super) fn tick(&mut self) {
        if self.dirty {
            self.dirty = false;
            self.events.publish(ThumbEvent::Updated);
        }
        let now = Instant::now();
        if now.duration_since(self.last_flush) >= FLUSH_EVERY {
            self.flush();
        }
        if self.phase == Phase::Done
            && self.generator.is_some()
            && self.asked.is_empty()
            && now.duration_since(self.last_answer) >= IDLE_END
        {
            self.stop();
        }
    }

    /// True when the actor should end.
    pub(super) async fn command(&mut self, command: ThumbCommand) -> bool {
        match command {
            ThumbCommand::Open(video) => self.open(video).await,
            ThumbCommand::Close => self.close(),
            ThumbCommand::Hover { ms } => self.hover(ms),
            ThumbCommand::Rest { ms } => self.rest(ms),
            ThumbCommand::View { from_ms, to_ms } => self.view(from_ms, to_ms),
            ThumbCommand::Settings { settings, render_nice } => self.settings(settings, render_nice).await,
            ThumbCommand::Rendering(rendering) => self.rendering(rendering),
            ThumbCommand::Jobs { sources, removed } => self.jobs(sources, removed),
            ThumbCommand::Cancel => self.cancel(),
            ThumbCommand::ClearCache => self.clear_cache(),
            ThumbCommand::Stats { reply } => {
                let stats = self
                    .config
                    .store
                    .request(|reply| ThumbStoreCommand::Stats { reply })
                    .await;
                let _ = reply.send(stats.unwrap_or_default());
            }
            ThumbCommand::Shutdown { reply } => {
                self.flush();
                self.stop();
                let _ = reply.send(());
                return true;
            }
        }
        false
    }
}
