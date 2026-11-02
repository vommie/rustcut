//! The plugin service's actor: the list of plugins, their workers, states and statistics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustcut_plugin_host::manifest::Manifest;
use rustcut_plugin_host::package::{self, Package};
use rustcut_plugin_host::runtime::{CallError, Limits, Loaded, Runtime, Worker};
use rustcut_plugin_host::types::Level;
use rustcut_plugin_host::values;
use serde_json::{Map, Value};
use tokio::sync::mpsc;

use super::dispatch::{self, CallKind, Report, Runner, Runners};
use super::scan::{self, Found};
use super::watch::{self, DevWatch, Watched};
use super::{
    Available, Failure, InstallError, Installed, PAUSE_AFTER, PluginCommand, PluginEvent, PluginInfo, PluginPaths,
    PluginState, Source, Stats,
};
use crate::actor::{EventBus, Handle, Mailbox};
use crate::settings::{PluginEntry, PluginSettings};

/// How long a development build must be quiet before it is reloaded.
const SETTLE: Duration = Duration::from_millis(500);

struct Slot {
    info: PluginInfo,
    loaded: Option<Loaded>,
    worker: Option<Worker>,
    /// Failures in a row.
    streak: u32,
    /// The values last handed to `configure`.
    configured: Option<Map<String, Value>>,
    /// The last `configure` was refused: the plugin does not get events until its settings change.
    refused: bool,
    limits: Limits,
}

pub(super) struct Actor {
    paths: PluginPaths,
    settings: PluginSettings,
    safe_mode: bool,
    me: Handle<PluginCommand>,
    events: EventBus<PluginEvent>,
    rt: tokio::runtime::Handle,
    runtime: Option<Arc<Runtime>>,
    slots: Vec<Slot>,
    runners: Runners,
    jobs: Option<mpsc::UnboundedSender<dispatch::Job>>,
    watch: Option<DevWatch>,
    touched: HashMap<String, u64>,
}

impl Actor {
    pub(super) fn new(
        paths: PluginPaths,
        settings: PluginSettings,
        safe_mode: bool,
        me: Handle<PluginCommand>,
        events: EventBus<PluginEvent>,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self {
            paths,
            settings,
            safe_mode,
            me,
            events,
            rt,
            runtime: None,
            slots: Vec::new(),
            runners: Runners::default(),
            jobs: None,
            watch: None,
            touched: HashMap::new(),
        }
    }

    pub(super) async fn run(mut self, mut rx: Mailbox<PluginCommand>) {
        let cache = self.paths.cache.clone();
        self.runtime = match tokio::task::spawn_blocking(move || Runtime::new(Some(&cache))).await {
            Ok(Ok(runtime)) => Some(Arc::new(runtime)),
            Ok(Err(err)) => {
                tracing::error!("[Plugins] no plugin can run: {err}");
                None
            }
            Err(err) => {
                tracing::error!("[Plugins] the plugin engine did not start: {err}");
                None
            }
        };
        let (jobs, inbox) = mpsc::unbounded_channel();
        self.jobs = Some(jobs);
        self.rt
            .spawn(dispatch::run(inbox, Arc::clone(&self.runners), self.me.clone()));
        self.rescan(None).await;
        while let Some(command) = rx.recv().await {
            self.command(command).await;
        }
    }

    async fn command(&mut self, command: PluginCommand) {
        match command {
            PluginCommand::Apply(settings) => self.apply(*settings).await,
            PluginCommand::Reload { id } => self.rescan(id).await,
            PluginCommand::Dispatch {
                event,
                context,
                folders,
                reply,
            } => {
                let job = dispatch::Job {
                    event,
                    context,
                    folders,
                    reply,
                };
                if let Some(jobs) = &self.jobs
                    && let Err(err) = jobs.send(job)
                {
                    let _ = err.0.reply.send(Vec::new());
                }
            }
            PluginCommand::Check { id, values, reply } => self.check(&id, &values, reply),
            PluginCommand::Install {
                file,
                allow_downgrade,
                reply,
            } => {
                let result = self.install(file, allow_downgrade).await;
                if let Ok(installed) = &result {
                    self.rescan(Some(installed.id.clone())).await;
                }
                let _ = reply.send(result);
            }
            PluginCommand::Remove { id, reply } => {
                let result = self.remove(&id).await;
                let _ = reply.send(result);
            }
            PluginCommand::Snapshot(reply) => {
                let _ = reply.send(self.infos());
            }
            PluginCommand::CheckUpdates(reply) => {
                let available = self.check_updates().await;
                let _ = reply.send(available);
            }
            PluginCommand::Reported(report) => self.reported(*report),
            PluginCommand::Touched { id } => {
                let generation = self.touched.entry(id.clone()).or_default();
                *generation += 1;
                let (me, generation) = (self.me.clone(), *generation);
                self.rt.spawn(async move {
                    tokio::time::sleep(SETTLE).await;
                    let _ = me.send(PluginCommand::Settled { id, generation });
                });
            }
            PluginCommand::Settled { id, generation } => {
                if self.touched.get(&id) == Some(&generation) {
                    tracing::info!("[Plugins] {id}: the development build changed, reloading");
                    self.rescan(Some(id)).await;
                }
            }
        }
    }

    /// Reads the folders again and (re)loads every plugin, or only `only`; the others keep their workers.
    async fn rescan(&mut self, only: Option<String>) {
        let installed = self.paths.installed.clone();
        let dev = self.settings.dev_folders.clone();
        let found = tokio::task::spawn_blocking(move || scan::scan(&installed, &dev))
            .await
            .unwrap_or_default();
        let mut old: Vec<Slot> = std::mem::take(&mut self.slots);
        let mut seen_dev = std::collections::HashSet::new();
        let dev_ids: std::collections::HashSet<String> = found
            .iter()
            .filter(|f| f.source == Source::Development && f.manifest.is_ok())
            .map(|f| f.id.clone())
            .collect();
        for found in found {
            let previous = old
                .iter()
                .position(|slot| slot.info.dir == found.dir)
                .map(|index| old.swap_remove(index));
            let keep = only.as_ref().is_some_and(|id| *id != found.id);
            let slot = match previous {
                Some(slot) if keep => slot,
                previous => {
                    let stats = previous.map(|slot| slot.info.stats).unwrap_or_default();
                    let shadowed = found.source == Source::Installed && dev_ids.contains(&found.id);
                    let twice = found.source == Source::Development && !seen_dev.insert(found.id.clone());
                    self.load(found, stats, shadowed, twice).await
                }
            };
            self.slots.push(slot);
        }
        self.sort();
        self.refresh_states();
        self.watch_dev();
        self.publish();
    }

    async fn load(&self, found: Found, stats: Stats, shadowed: bool, twice: bool) -> Slot {
        let entry = self.settings.entry(&found.id);
        let limits = limits(&entry);
        let mut info = PluginInfo {
            id: found.id,
            manifest: None,
            dir: found.dir,
            source: found.source,
            state: PluginState::Ready,
            stats,
            update: None,
        };
        let mut slot = Slot {
            info: info.clone(),
            loaded: None,
            worker: None,
            streak: 0,
            configured: None,
            refused: false,
            limits,
        };
        let manifest = match found.manifest {
            Ok(manifest) => manifest,
            Err(err) => {
                slot.info.state = PluginState::Broken(err);
                return slot;
            }
        };
        info.manifest = Some(Arc::new(manifest.clone()));
        slot.info = info;
        if twice {
            slot.info.state = PluginState::Broken("another development folder has the same id".to_owned());
        } else if shadowed {
            slot.info.state = PluginState::Shadowed;
        } else if !manifest.api_compatible() {
            slot.info.state = PluginState::Incompatible(manifest.api.clone());
        } else if let Some(runtime) = self.runtime.clone() {
            let dir = slot.info.dir.clone();
            match tokio::task::spawn_blocking(move || runtime.load(&dir, manifest)).await {
                Ok(Ok(loaded)) => slot.loaded = Some(loaded),
                Ok(Err(err)) => slot.info.state = PluginState::Broken(err.to_string()),
                Err(err) => slot.info.state = PluginState::Broken(err.to_string()),
            }
        } else {
            slot.info.state = PluginState::Broken("the plugin engine did not start".to_owned());
        }
        slot
    }

    /// The user's order first, then the rest by name.
    fn sort(&mut self) {
        let order = &self.settings.order;
        self.slots.sort_by_cached_key(|slot| {
            let place = order.iter().position(|id| *id == slot.info.id).unwrap_or(usize::MAX);
            (place, slot.info.name().to_lowercase(), slot.info.dir.clone())
        });
    }

    /// Switches workers on and off after a rescan or new settings, and configures changed values.
    fn refresh_states(&mut self) {
        for index in 0..self.slots.len() {
            let Some(slot) = self.slots.get(index) else { continue };
            let entry = self.settings.entry(&slot.info.id);
            let wanted = self.wanted_state(slot, &entry);
            let limits = limits(&entry);
            let Some(slot) = self.slots.get_mut(index) else {
                continue;
            };
            if let Some(state) = wanted {
                slot.info.state = state;
                slot.worker = None;
                slot.configured = None;
                continue;
            }
            if slot.worker.is_none()
                && let Some(loaded) = &slot.loaded
            {
                match Worker::spawn(loaded.clone(), limits) {
                    Ok(worker) => {
                        slot.worker = Some(worker);
                        slot.limits = limits;
                    }
                    Err(err) => {
                        slot.info.state = PluginState::Broken(format!("no thread: {err}"));
                        continue;
                    }
                }
            }
            if slot.limits != limits
                && let Some(worker) = &slot.worker
            {
                worker.set_limits(limits);
                slot.limits = limits;
            }
            if !matches!(slot.info.state, PluginState::Paused(_)) && !slot.refused {
                slot.info.state = PluginState::Ready;
            }
            let Some(manifest) = slot.info.manifest.clone() else {
                continue;
            };
            let effective = values::effective(&manifest, &entry.values);
            if slot.configured.as_ref() != Some(&effective)
                && let Some(worker) = &slot.worker
            {
                let me = self.me.clone();
                let id = slot.info.id.clone();
                worker.configure(
                    Value::Object(effective.clone()),
                    Box::new(move |report| {
                        let report = Report {
                            id,
                            kind: CallKind::Configure,
                            took: report.took,
                            error: report.result.err(),
                            log: report.log,
                        };
                        let _ = me.send(PluginCommand::Reported(Box::new(report)));
                    }),
                );
                slot.configured = Some(effective);
            }
        }
        self.update_runners();
    }

    /// The state a plugin must have regardless of its worker, or `None` when it may run.
    fn wanted_state(&self, slot: &Slot, entry: &PluginEntry) -> Option<PluginState> {
        if slot.loaded.is_none() {
            // Broken, incompatible, shadowed: what loading found stays.
            Some(slot.info.state.clone())
        } else if self.safe_mode {
            Some(PluginState::SafeMode)
        } else if !self.settings.enabled {
            Some(PluginState::AllOff)
        } else if !entry.enabled {
            Some(PluginState::Off)
        } else {
            None
        }
    }

    fn update_runners(&self) {
        let runners: Vec<Runner> = self
            .slots
            .iter()
            .filter(|slot| slot.info.state == PluginState::Ready && !slot.refused)
            .filter_map(|slot| {
                Some(Runner {
                    id: slot.info.id.clone(),
                    name: slot.info.name().to_owned(),
                    worker: slot.worker.clone()?,
                    limit: slot.limits.time,
                })
            })
            .collect();
        *self.runners.lock() = Arc::new(runners);
    }

    async fn apply(&mut self, settings: PluginSettings) {
        let rescan = settings.dev_folders != self.settings.dev_folders;
        let resort = settings.order != self.settings.order;
        self.settings = settings;
        if rescan {
            self.rescan(None).await;
            return;
        }
        if resort {
            self.sort();
        }
        self.refresh_states();
        self.publish();
    }

    fn reported(&mut self, report: Report) {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.info.id == report.id && slot.worker.is_some())
        else {
            return;
        };
        let id = &report.id;
        for line in &report.log {
            log_line(id, line.level, &line.text);
        }
        if matches!(report.kind, CallKind::Handle(_)) {
            let stats = &mut slot.info.stats;
            stats.calls += 1;
            stats.total += report.took;
            stats.longest = stats.longest.max(report.took);
        }
        let Some(error) = report.error else {
            slot.streak = 0;
            if report.kind == CallKind::Configure && slot.refused {
                slot.refused = false;
                slot.info.state = PluginState::Ready;
            }
            self.update_runners();
            self.publish();
            return;
        };
        let what = match report.kind {
            CallKind::Configure => "settings".to_owned(),
            CallKind::Handle(event) => event.to_owned(),
            CallKind::Check => "check".to_owned(),
        };
        let text = error_text(&error);
        tracing::warn!("[Plugins] {id}: {what} failed: {text}");
        slot.info.stats.errors += 1;
        slot.info.stats.last_error = Some((SystemTime::now(), text.clone()));
        let mut paused = false;
        if report.kind == CallKind::Configure && matches!(error, CallError::Failed(_)) {
            slot.refused = true;
            slot.info.state = PluginState::Broken(format!("the settings were refused: {text}"));
        } else {
            slot.streak += 1;
            if slot.streak >= PAUSE_AFTER && slot.info.state == PluginState::Ready {
                paused = true;
                slot.info.state = PluginState::Paused(text.clone());
                tracing::warn!("[Plugins] {id}: paused after {PAUSE_AFTER} failures in a row");
            }
        }
        let failure = Failure {
            id: id.clone(),
            name: slot.info.name().to_owned(),
            error: text,
            paused,
        };
        self.update_runners();
        self.events.publish(PluginEvent::Failed(failure));
        self.publish();
    }

    fn check(&self, id: &str, values: &Map<String, Value>, reply: crate::actor::Reply<Option<Vec<super::Issue>>>) {
        let slot = self.slots.iter().find(|slot| slot.info.id == id);
        let (Some(worker), Some(manifest)) = (
            slot.and_then(|s| s.worker.clone()),
            slot.and_then(|s| s.info.manifest.clone()),
        ) else {
            let _ = reply.send(None);
            return;
        };
        let id = id.to_owned();
        let effective = values::effective(&manifest, values);
        worker.check(
            Value::Object(effective),
            Box::new(move |report| {
                for line in &report.log {
                    log_line(&id, line.level, &line.text);
                }
                if let Err(err) = &report.result {
                    tracing::warn!("[Plugins] {id}: check failed: {}", error_text(err));
                }
                let _ = reply.send(report.result.ok());
            }),
        );
    }

    async fn install(&mut self, file: PathBuf, allow_downgrade: bool) -> Result<Installed, InstallError> {
        let Some(runtime) = self.runtime.clone() else {
            return Err(InstallError::Load("the plugin engine did not start".to_owned()));
        };
        let dir = self.paths.installed.clone();
        let task = move || -> Result<Installed, InstallError> {
            let package = Package::read(&file).map_err(|err| InstallError::Package(err.to_string()))?;
            let manifest = package.manifest.clone();
            if !manifest.api_compatible() {
                return Err(InstallError::Incompatible(manifest.api));
            }
            let previous = Manifest::read(&dir.join(&manifest.id)).ok().map(|m| m.version);
            if let Some(installed) = &previous
                && manifest.version < *installed
                && !allow_downgrade
            {
                return Err(InstallError::Downgrade {
                    installed: installed.clone(),
                    offered: manifest.version,
                });
            }
            let staged = package
                .stage(&dir)
                .map_err(|err| InstallError::Package(err.to_string()))?;
            if let Err(err) = runtime.load(&staged.dir, manifest.clone()) {
                if let Err(discard) = staged.discard() {
                    tracing::warn!("[Plugins] the staging folder stays: {discard}");
                }
                return Err(InstallError::Load(err.to_string()));
            }
            staged.commit().map_err(|err| InstallError::Package(err.to_string()))?;
            Ok(Installed {
                id: manifest.id,
                name: manifest.name,
                version: manifest.version,
                previous,
            })
        };
        let result = tokio::task::spawn_blocking(task)
            .await
            .unwrap_or_else(|err| Err(InstallError::Load(err.to_string())));
        match &result {
            Ok(done) => tracing::info!(
                "[Plugins] {}: installed {}{}",
                done.id,
                done.version,
                done.previous.as_ref().map_or(String::new(), |v| format!(" (was {v})"))
            ),
            Err(err) => tracing::warn!("[Plugins] a plugin file was not installed: {err}"),
        }
        result
    }

    async fn remove(&mut self, id: &str) -> Result<(), String> {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.info.id == id && slot.info.source == Source::Installed)
        else {
            return Err(format!("\"{id}\" is not an installed plugin"));
        };
        if let Some(slot) = self.slots.get_mut(index) {
            slot.worker = None;
        }
        self.update_runners();
        let dir = self.paths.installed.clone();
        let owned = id.to_owned();
        let result = tokio::task::spawn_blocking(move || package::remove(&dir, &owned))
            .await
            .map_err(|err| err.to_string())
            .and_then(|done| done.map_err(|err| err.to_string()));
        match &result {
            Ok(()) => tracing::info!("[Plugins] {id}: removed (its settings stay)"),
            Err(err) => tracing::warn!("[Plugins] {id}: not removed: {err}"),
        }
        self.rescan(None).await;
        result
    }

    async fn check_updates(&mut self) -> Vec<Available> {
        let folder = self.settings.update_folder.clone();
        if folder.as_os_str().is_empty() {
            return Vec::new();
        }
        let offered = tokio::task::spawn_blocking(move || newest_files(&folder))
            .await
            .unwrap_or_default();
        let mut available = Vec::new();
        for (id, (manifest, file)) in offered {
            match self
                .slots
                .iter_mut()
                .find(|slot| slot.info.id == id && slot.info.source == Source::Installed)
            {
                Some(slot) => {
                    let newer = slot.info.manifest.as_ref().is_none_or(|m| manifest.version > m.version);
                    slot.info.update = newer.then(|| (manifest.version.clone(), file));
                }
                None => available.push(Available {
                    manifest: Arc::new(manifest),
                    file,
                }),
            }
        }
        self.publish();
        available
    }

    fn infos(&self) -> Vec<PluginInfo> {
        self.slots.iter().map(|slot| slot.info.clone()).collect()
    }

    fn publish(&self) {
        self.events.publish(PluginEvent::Changed(Arc::new(self.infos())));
    }

    fn watch_dev(&mut self) {
        let watched: Vec<Watched> = self
            .slots
            .iter()
            .filter(|slot| slot.info.source == Source::Development)
            .map(|slot| {
                let manifest_file = slot.info.dir.join(rustcut_plugin_host::manifest::MANIFEST_FILE);
                let mut files = vec![normal(&manifest_file)];
                if let Some(manifest) = &slot.info.manifest {
                    files.push(normal(&slot.info.dir.join(&manifest.wasm)));
                }
                Watched {
                    id: slot.info.id.clone(),
                    files,
                }
            })
            .collect();
        self.watch = watch::watch(watched, self.me.clone());
    }
}

fn limits(entry: &PluginEntry) -> Limits {
    Limits {
        time: Duration::from_millis(u64::from(entry.time_limit_ms)),
        memory_bytes: usize::try_from(entry.memory_mb).unwrap_or(64).saturating_mul(1 << 20),
    }
}

/// A path without `..` and symbolic links, as the file watcher reports it.
fn normal(path: &Path) -> PathBuf {
    let clean = path_clean::clean(path);
    match (clean.parent().map(std::fs::canonicalize), clean.file_name()) {
        (Some(Ok(parent)), Some(name)) => parent.join(name),
        _ => clean,
    }
}

fn error_text(error: &CallError) -> String {
    error.to_string()
}

fn log_line(id: &str, level: Level, text: &str) {
    match level {
        Level::Debug => tracing::debug!("[Plugins] {id}: {text}"),
        Level::Info => tracing::info!("[Plugins] {id}: {text}"),
        Level::Warning => tracing::warn!("[Plugins] {id}: {text}"),
        Level::Error => tracing::error!("[Plugins] {id}: {text}"),
    }
}

/// The newest plugin file per id in a folder.
fn newest_files(folder: &Path) -> HashMap<String, (Manifest, PathBuf)> {
    let mut best: HashMap<String, (Manifest, PathBuf)> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return best;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|ext| ext != package::EXTENSION) {
            continue;
        }
        match Package::read(&path) {
            Ok(package) => {
                let manifest = package.manifest;
                let newer = best
                    .get(&manifest.id)
                    .is_none_or(|(known, _)| manifest.version > known.version);
                if newer {
                    best.insert(manifest.id.clone(), (manifest, path));
                }
            }
            Err(err) => tracing::debug!("[Plugins] {} is skipped: {err}", path.display()),
        }
    }
    best
}
