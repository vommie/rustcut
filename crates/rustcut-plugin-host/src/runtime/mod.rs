//! The WebAssembly sandbox: one engine for all plugins, a compiled component per plugin, an instance per worker.
//!
//! Limits: every call has a deadline (epoch interruption, checked every [`TICK`]) and every instance a memory limit.
//! A plugin sees no files, no network, no environment and no programs: its WASI context grants only clocks and
//! random numbers, and stdout/stderr are captured into the plugin log.

mod bindings;
mod convert;
mod instance;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine};

pub use instance::{CallError, Instance, Limits};
pub use worker::{CallReport, Reply, Worker};

use crate::manifest::Manifest;

/// How often the deadline of a running call is checked.
pub const TICK: Duration = Duration::from_millis(10);

/// The engine all plugins share, with the thread that moves its clock.
pub struct Runtime {
    engine: Engine,
    ticker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

/// Why a plugin could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("the engine cannot start: {0}")]
    Engine(String),
    #[error("cannot read {path}: {reason}")]
    Read { path: PathBuf, reason: String },
    #[error("the plugin is not a RustCut plugin component: {0}")]
    Component(String),
}

impl Runtime {
    /// Starts the engine. `cache_dir` keeps compiled plugins, so a plugin compiles once per version.
    ///
    /// # Errors
    /// The engine cannot start (unsupported CPU) or its cache cannot be set up.
    pub fn new(cache_dir: Option<&Path>) -> Result<Self, LoadError> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        config.wasm_component_model(true);
        if let Some(dir) = cache_dir {
            let mut cache = wasmtime::CacheConfig::new();
            cache.with_directory(dir);
            match wasmtime::Cache::new(cache) {
                Ok(cache) => {
                    config.cache(Some(cache));
                }
                Err(err) => tracing::warn!("[Plugins] no compile cache in {}: {err}", dir.display()),
            }
        }
        let engine = Engine::new(&config).map_err(|err| LoadError::Engine(err.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let ticker = {
            let engine = engine.clone();
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("rustcut-plugin-clock".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(TICK);
                        engine.increment_epoch();
                    }
                })
                .map_err(|err| LoadError::Engine(err.to_string()))?
        };
        Ok(Self {
            engine,
            ticker: Some(ticker),
            stop,
        })
    }

    /// Compiles the plugin in `dir` (its manifest already read) and links it against the host.
    ///
    /// # Errors
    /// The component file is missing or is not a component of the `plugin` world.
    pub fn load(&self, dir: &Path, manifest: Manifest) -> Result<Loaded, LoadError> {
        let path = dir.join(&manifest.wasm);
        let bytes = std::fs::read(&path).map_err(|err| LoadError::Read {
            path: path.clone(),
            reason: err.to_string(),
        })?;
        let component = Component::new(&self.engine, &bytes).map_err(|err| LoadError::Component(format!("{err:#}")))?;
        let mut linker: Linker<instance::State> = Linker::new(&self.engine);
        // A plugin built against an older minor API version imports older interface versions; semver matching
        // links them to this host's.
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|err| LoadError::Component(format!("{err:#}")))?;
        bindings::Plugin::add_to_linker::<instance::State, wasmtime::component::HasSelf<instance::State>>(
            &mut linker,
            |state| state,
        )
        .map_err(|err| LoadError::Component(format!("{err:#}")))?;
        let pre = linker
            .instantiate_pre(&component)
            .map_err(|err| LoadError::Component(format!("{err:#}")))?;
        let pre = bindings::PluginPre::new(pre).map_err(|err| LoadError::Component(format!("{err:#}")))?;
        Ok(Loaded {
            inner: Arc::new(LoadedInner {
                manifest,
                dir: dir.to_path_buf(),
                engine: self.engine.clone(),
                pre,
            }),
        })
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(ticker) = self.ticker.take()
            && ticker.join().is_err()
        {
            tracing::warn!("[Plugins] the clock thread ended with a panic");
        }
    }
}

/// A compiled plugin, ready for instances; cheap to clone.
#[derive(Clone)]
pub struct Loaded {
    inner: Arc<LoadedInner>,
}

struct LoadedInner {
    manifest: Manifest,
    dir: PathBuf,
    engine: Engine,
    pre: bindings::PluginPre<instance::State>,
}

impl Loaded {
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.inner.manifest
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("id", &self.inner.manifest.id)
            .field("dir", &self.inner.dir)
            .finish()
    }
}
