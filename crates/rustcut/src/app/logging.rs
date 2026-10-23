//! Logging setup: `tracing` to stderr, level from `RUST_LOG` (default `info`). Rotating log files arrive with the settings paths.

use tracing_subscriber::EnvFilter;

/// Installs the global subscriber. A second call (tests) is ignored.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
