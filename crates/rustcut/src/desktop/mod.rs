//! Desktop integration: single instance with file forwarding, and (P5) the sleep lock, suspend and power off through
//! logind, the taskbar progress and notifications (`service` is the actor behind them).

pub mod color_scheme;
pub mod instance;
pub mod logind;
pub mod notify;
pub mod service;
pub mod taskbar;
pub mod x11_dialog;
