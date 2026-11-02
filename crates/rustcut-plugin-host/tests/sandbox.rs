//! The real sandbox with the test plugin (`test-plugin/`, built for wasm32-wasip2 here; fails, never skips, when
//! the target is missing: `rustup target add wasm32-wasip2`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed step should stop the test"
)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rustcut_plugin_host::manifest::Manifest;
use rustcut_plugin_host::package::{Package, pack};
use rustcut_plugin_host::runtime::{CallError, Instance, Limits, Loaded, Runtime, Worker};
use rustcut_plugin_host::types::{Action, Context, Event, Level, OpenedBy, Target};
use serde_json::json;

fn plugin_dir() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-plugin");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the test plugin must build (rustup target add wasm32-wasip2)"
        );
        dir
    })
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new(None).unwrap())
}

fn loaded() -> Loaded {
    let dir = plugin_dir();
    runtime().load(dir, Manifest::read(dir).unwrap()).unwrap()
}

fn instance(mode: &str, limits: Limits) -> Instance {
    let mut instance = Instance::new(&loaded(), limits).unwrap();
    instance.configure(&json!({ "mode": mode })).unwrap();
    instance
}

fn opened() -> (Event, Context) {
    let context = Context {
        video: Some("/videos/holiday/beach.mp4".into()),
        ..Context::default()
    };
    (Event::VideoOpened(OpenedBy::User), context)
}

#[test]
fn a_good_plugin_answers_with_actions_and_logs() {
    let mut plugin = instance("ok", Limits::default());
    let log = plugin.take_log();
    assert!(
        log.iter()
            .any(|line| line.level == Level::Info && line.text == "configured, mode ok"),
        "{log:?}"
    );
    let (event, context) = opened();
    let actions = plugin.handle(&event, &context).unwrap();
    assert_eq!(actions.len(), 2);
    assert!(
        matches!(&actions[1], Action::SetTarget { target: Target::Path(p), .. } if p == Path::new("/videos/holiday"))
    );
    let start = Instant::now();
    for _ in 0..1000 {
        plugin.handle(&event, &context).unwrap();
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "1000 calls took {:?}",
        start.elapsed()
    );
}

#[test]
fn errors_panics_timeouts_and_memory_are_caught() {
    let (event, context) = opened();
    let fast = Limits {
        time: Duration::from_millis(100),
        memory_bytes: 32 << 20,
    };
    assert_eq!(
        instance("fail", fast).handle(&event, &context),
        Err(CallError::Failed("asked to fail".into()))
    );

    let mut panicking = instance("panic", fast);
    assert!(matches!(panicking.handle(&event, &context), Err(CallError::Trap(_))));
    assert!(
        panicking
            .take_log()
            .iter()
            .any(|line| line.text.contains("asked to panic"))
    );

    let start = Instant::now();
    assert_eq!(
        instance("slow", fast).handle(&event, &context),
        Err(CallError::Timeout(fast.time))
    );
    let took = start.elapsed();
    assert!(
        took >= Duration::from_millis(100) && took < Duration::from_millis(400),
        "{took:?}"
    );

    assert_eq!(
        instance("memory", fast).handle(&event, &context),
        Err(CallError::OutOfMemory(32))
    );

    let mut refusing = Instance::new(&loaded(), fast).unwrap();
    assert!(matches!(
        refusing.configure(&json!({ "mode": "refuse" })),
        Err(CallError::Failed(_))
    ));
}

#[test]
fn check_reports_issues() {
    let mut plugin = instance("ok", Limits::default());
    let issues = plugin.check(&json!({ "mode": "dance" })).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].field, "mode");
}

fn configure(worker: &Worker, mode: &str) -> Result<(), CallError> {
    let (tx, rx) = std::sync::mpsc::channel();
    worker.configure(
        json!({ "mode": mode }),
        Box::new(move |report| tx.send(report.result).unwrap()),
    );
    rx.recv().unwrap()
}

fn handle(worker: &Worker) -> Result<Vec<Action>, CallError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (event, context) = opened();
    worker.handle(event, context, Box::new(move |report| tx.send(report.result).unwrap()));
    rx.recv().unwrap()
}

#[test]
fn a_worker_recovers_after_a_crash_and_keeps_the_settings() {
    let worker = Worker::spawn(
        loaded(),
        Limits {
            time: Duration::from_millis(100),
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(configure(&worker, "slow"), Ok(()));
    assert!(matches!(handle(&worker), Err(CallError::Timeout(_))));
    // The next call gets a fresh instance, configured again with "slow".
    assert!(matches!(handle(&worker), Err(CallError::Timeout(_))));
    assert_eq!(configure(&worker, "ok"), Ok(()));
    assert_eq!(handle(&worker).unwrap().len(), 2);
}

#[test]
fn a_packed_plugin_installs_and_loads() {
    let dir = plugin_dir();
    let manifest = Manifest::read(dir).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("test-plugin-0.1.0.rcplugin");
    pack(&manifest, &dir.join(&manifest.wasm), dir, &file).unwrap();
    let package = Package::read(&file).unwrap();
    let staged = package.stage(&tmp.path().join("plugins")).unwrap();
    let installed = staged.commit().unwrap();
    let manifest = Manifest::read(&installed).unwrap();
    let loaded = runtime().load(&installed, manifest).unwrap();
    let mut plugin = Instance::new(&loaded, Limits::default()).unwrap();
    plugin.configure(&json!({ "mode": "ok" })).unwrap();
}

#[test]
fn a_file_that_is_no_component_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let manifest = Manifest::parse("id = \"bad\"\nname = \"Bad\"\nversion = \"1.0.0\"\napi = \"1.0\"\n").unwrap();
    assert!(runtime().load(tmp.path(), manifest).is_err());
}
