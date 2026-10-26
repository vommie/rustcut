//! The run's one `Redactor` (P9.17): the shareable log file, the Logs panel's shareable view and a copied shareable log
//! use the same placeholders. The bridges teach it what they know (an opened video, the target folders, RustView's tag
//! names, a video's title); the settings switch its kinds (Settings > Advanced & Logs).

use std::path::Path;
use std::sync::OnceLock;

use parking_lot::Mutex;
use rustcut_core::redact::{RedactKinds, Redactor};

static REDACTOR: OnceLock<Mutex<Redactor>> = OnceLock::new();

fn with<R>(f: impl FnOnce(&mut Redactor) -> R) -> R {
    f(&mut REDACTOR
        .get_or_init(|| Mutex::new(Redactor::new(RedactKinds::default())))
        .lock())
}

/// `line` as the shareable copy shows it.
pub fn redact(line: &str) -> String {
    with(|r| r.redact(line))
}

pub fn set_kinds(kinds: RedactKinds) {
    with(|r| r.set_kinds(kinds));
}

pub fn learn_path(path: &Path) {
    with(|r| r.learn_path(path));
}

pub fn learn_dir(path: &Path) {
    with(|r| r.learn_dir(path));
}

pub fn learn_tag(name: &str) {
    with(|r| r.learn_tag(name));
}

pub fn learn_meta(text: &str) {
    with(|r| r.learn_meta(text));
}
