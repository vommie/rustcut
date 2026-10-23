//! Logging setup: `tracing` to stderr. The level comes from `RUST_LOG` when set, otherwise from the setting
//! `advanced.log_level` once the settings are read (`info` until then). Rotating log files arrive with M1.3.

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

/// Changes the log filter after startup.
pub struct LogControl {
    handle: Option<reload::Handle<EnvFilter, Registry>>,
    from_env: bool,
}

/// Installs the global subscriber. A second call (tests) leaves the first one in place.
pub fn init() -> LogControl {
    let from_env = std::env::var_os("RUST_LOG").is_some();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (layer, handle) = reload::Layer::new(filter);
    let installed = tracing_subscriber::registry()
        .with(layer)
        .with(fmt::layer().with_target(false))
        .try_init()
        .is_ok();
    LogControl {
        handle: installed.then_some(handle),
        from_env,
    }
}

impl LogControl {
    /// Uses the configured level unless `RUST_LOG` was given (the environment wins).
    pub fn apply_setting(&self, level: &str) {
        if self.from_env {
            return;
        }
        let Some(handle) = &self.handle else { return };
        match EnvFilter::try_new(level) {
            Ok(filter) => {
                if let Err(e) = handle.reload(filter) {
                    tracing::warn!(error = %e, "[App] The log level could not be changed");
                } else {
                    tracing::debug!(level, "[App] Log level from the settings");
                }
            }
            Err(e) => tracing::warn!(level, error = %e, "[App] The configured log level is not valid; keeping info"),
        }
    }
}
