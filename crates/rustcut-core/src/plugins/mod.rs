//! The plugin service (P12 block 8, ADR-17): finds the installed plugins and the development folders, keeps one worker
//! thread per running plugin (`rustcut-plugin-host`), hands them RustCut's events in the user's order and reports what
//! they want done. A plugin that fails three times in a row is paused until it is reloaded or RustCut starts again.
//!
//! The UI thread never waits: [`PluginCommand::Dispatch`] answers through a reply, and the events run on their own
//! task, one after the other, so a slow plugin delays only the plugins after it for that one event (at most its time
//! limit). What the plugins write goes into RustCut's log with the prefix `[Plugins] <id>:`.

mod actor;
mod dispatch;
mod scan;
#[cfg(test)]
mod tests;
mod watch;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub use rustcut_plugin_host::manifest::Manifest;
pub use rustcut_plugin_host::types::{Action, Context, Event, Issue, Level, Notice, OpenedBy, Place, Tag, Target};
use serde_json::{Map, Value};

use crate::actor::{EventBus, Handle, Reply, mailbox};

/// Collects every answer of one dispatched event (tests and simple callers).
pub async fn dispatch_all(
    handle: &Handle<PluginCommand>,
    event: Event,
    context: Context,
    folders: Arc<BTreeMap<u32, PathBuf>>,
) -> Vec<Outcome> {
    let (outcomes, mut rx) = tokio::sync::mpsc::unbounded_channel();
    if handle
        .send(PluginCommand::Dispatch {
            event,
            context,
            folders,
            outcomes,
        })
        .is_err()
    {
        return Vec::new();
    }
    let mut all = Vec::new();
    while let Some(outcome) = rx.recv().await {
        all.push(outcome);
    }
    all
}
use crate::settings::PluginSettings;

/// The file name extension of plugin files (`rcplugin`).
pub fn package_extension() -> &'static str {
    rustcut_plugin_host::package::EXTENSION
}

/// The plugin's manifest fields and their values (re-exported for the Settings pages).
pub use rustcut_plugin_host::manifest::{Card, ChoiceOption, Field, FieldKind, PathSelect};
pub use rustcut_plugin_host::values;

/// How many failures in a row pause a plugin.
pub const PAUSE_AFTER: u32 = 3;

/// Where plugins live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPaths {
    /// Installed plugins (`<data>/plugins/<id>/`).
    pub installed: PathBuf,
    /// wasmtime's compile cache.
    pub cache: PathBuf,
}

/// Where a plugin comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Installed,
    /// A development folder of `[plugins] dev_folders`.
    Development,
}

/// Whether a plugin runs, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    Ready,
    /// Its switch is off.
    Off,
    /// The plugins' master switch is off.
    AllOff,
    /// RustCut started with `--no-plugins`.
    SafeMode,
    /// It failed [`PAUSE_AFTER`] times in a row; the last failure.
    Paused(String),
    /// It cannot run: the manifest, the component or its settings were refused.
    Broken(String),
    /// It needs a newer plugin API than this RustCut has.
    Incompatible(String),
    /// A development folder has the same id and runs instead.
    Shadowed,
}

/// What a plugin cost since RustCut started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub calls: u64,
    pub total: Duration,
    pub longest: Duration,
    pub errors: u64,
    pub last_error: Option<(SystemTime, String)>,
}

impl Stats {
    pub fn average(&self) -> Duration {
        u32::try_from(self.calls)
            .ok()
            .filter(|n| *n > 0)
            .map_or(Duration::ZERO, |n| self.total / n)
    }
}

/// One plugin as the Settings pages show it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    /// The manifest's id, or the folder's name when the manifest cannot be read.
    pub id: String,
    pub manifest: Option<Arc<Manifest>>,
    pub dir: PathBuf,
    pub source: Source,
    pub state: PluginState,
    pub stats: Stats,
    /// A newer version in the update folder: its version and file.
    pub update: Option<(semver::Version, PathBuf)>,
}

impl PluginInfo {
    /// The name to show.
    pub fn name(&self) -> &str {
        self.manifest.as_ref().map_or(self.id.as_str(), |m| m.name.as_str())
    }
}

/// A plugin file in the update folder that is not installed.
#[derive(Debug, Clone, PartialEq)]
pub struct Available {
    pub manifest: Arc<Manifest>,
    pub file: PathBuf,
}

/// What one plugin wants done after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub id: String,
    pub name: String,
    pub actions: Vec<Action>,
}

/// A plugin failed (an error, a crash, a timeout, too much memory, refused settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub id: String,
    pub name: String,
    pub error: String,
    /// It failed [`PAUSE_AFTER`] times in a row and is paused now.
    pub paused: bool,
}

/// A plugin was installed or updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: semver::Version,
    pub previous: Option<semver::Version>,
}

/// Why a plugin file was not installed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Package(String),
    #[error("version {offered} is older than the installed {installed}")]
    Downgrade {
        installed: semver::Version,
        offered: semver::Version,
    },
    #[error("it needs plugin API {0}, this RustCut has {api}", api = rustcut_plugin_host::manifest::API_VERSION)]
    Incompatible(String),
    #[error("the plugin cannot be loaded: {0}")]
    Load(String),
    #[error("the plugin service has stopped")]
    Gone,
}

pub enum PluginCommand {
    /// New settings (the master switch, order, switches, limits, values, development folders).
    Apply(Box<PluginSettings>),
    /// Reads the plugin folders again and reloads one plugin or all (also resumes a paused one).
    Reload {
        id: Option<String>,
    },
    /// Hands an event to every running plugin in order. `folders` maps target folder ids to paths, so a later
    /// plugin's context shows an earlier one's target. Each plugin's answer goes to `outcomes` as soon as it is there
    /// (a slow plugin never holds back the ones before it); the sender is dropped when the event is through.
    Dispatch {
        event: Event,
        context: Context,
        folders: Arc<BTreeMap<u32, PathBuf>>,
        outcomes: tokio::sync::mpsc::UnboundedSender<Outcome>,
    },
    /// Lets a plugin check settings being edited; `None` when it does not run.
    Check {
        id: String,
        values: Map<String, Value>,
        reply: Reply<Option<Vec<Issue>>>,
    },
    Install {
        file: PathBuf,
        allow_downgrade: bool,
        reply: Reply<Result<Installed, InstallError>>,
    },
    /// Removes an installed plugin's folder (its settings stay).
    Remove {
        id: String,
        reply: Reply<Result<(), String>>,
    },
    Snapshot(Reply<Vec<PluginInfo>>),
    /// Looks into the update folder.
    CheckUpdates(Reply<Vec<Available>>),
    /// Internal: a call's result (from the dispatcher or a reply).
    Reported(Box<dispatch::Report>),
    /// Internal: a development folder's build changed.
    Touched {
        id: String,
    },
    /// Internal: the debounce of a change ran out.
    Settled {
        id: String,
        generation: u64,
    },
}

impl std::fmt::Debug for PluginCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Apply(_) => "Apply",
            Self::Reload { .. } => "Reload",
            Self::Dispatch { .. } => "Dispatch",
            Self::Check { .. } => "Check",
            Self::Install { .. } => "Install",
            Self::Remove { .. } => "Remove",
            Self::Snapshot(_) => "Snapshot",
            Self::CheckUpdates(_) => "CheckUpdates",
            Self::Reported(_) => "Reported",
            Self::Touched { .. } => "Touched",
            Self::Settled { .. } => "Settled",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginEvent {
    /// The list of plugins or a plugin's state or statistics changed.
    Changed(Arc<Vec<PluginInfo>>),
    Failed(Failure),
}

/// The running service.
#[derive(Debug, Clone)]
pub struct PluginService {
    pub handle: Handle<PluginCommand>,
    pub events: EventBus<PluginEvent>,
}

/// Starts the service. `safe_mode` (`--no-plugins`) lists the plugins but runs none.
pub fn spawn_plugin_service(
    runtime: &tokio::runtime::Handle,
    paths: PluginPaths,
    settings: PluginSettings,
    safe_mode: bool,
) -> PluginService {
    let (handle, rx) = mailbox();
    let events = EventBus::new();
    let actor = actor::Actor::new(
        paths,
        settings,
        safe_mode,
        handle.clone(),
        events.clone(),
        runtime.clone(),
    );
    runtime.spawn(actor.run(rx));
    PluginService { handle, events }
}
