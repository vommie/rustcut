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
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use rustcut_plugin_host::commands::{
    CommandCall, FileManagerMode, FolderFile, FolderListing, FolderOrder, FolderRequest, Navigator, OrderSource,
};
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

#[test]
fn api_1_1_hides_marks_and_tells_the_target_folder_for_one_call() {
    let mut plugin = instance("quiet", Limits::default());
    let (event, mut context) = opened();
    context.target_folder = Some(4);
    let actions = plugin.handle(&event, &context).unwrap();
    assert!(
        matches!(
            &actions[0],
            Action::SetRating {
                stars: 3,
                mark: false,
                ..
            }
        ),
        "{actions:?}"
    );
    assert!(
        matches!(&actions[1], Action::Notify(n) if n.detail == "folder Some(4)"),
        "{actions:?}"
    );
    context.target_folder = None;
    let actions = plugin.handle(&event, &context).unwrap();
    assert!(
        matches!(&actions[1], Action::Notify(n) if n.detail == "folder None"),
        "{actions:?}"
    );
    // A plugin that does not hide its marks keeps them, also after a quiet call on the same instance.
    plugin.configure(&json!({ "mode": "ok" })).unwrap();
    let actions = plugin.handle(&event, &context).unwrap();
    assert!(
        matches!(&actions[1], Action::SetTarget { mark: true, .. }),
        "{actions:?}"
    );
}

#[test]
fn a_plugin_built_against_api_1_0_still_loads_and_answers() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api-1.0.0");
    let manifest = Manifest::read(&dir).unwrap();
    assert_eq!(manifest.api, "1.0");
    let loaded = runtime().load(&dir, manifest).unwrap();
    let mut plugin = Instance::new(&loaded, Limits::default()).unwrap();
    plugin.configure(&json!({ "mode": "ok" })).unwrap();
    let (event, context) = opened();
    let actions = plugin.handle(&event, &context).unwrap();
    assert!(
        matches!(&actions[1], Action::SetTarget { target: Target::Path(p), mark: true, .. } if p == Path::new("/videos/holiday")),
        "{actions:?}"
    );
}

#[test]
fn a_plugin_built_against_api_1_1_still_loads_and_answers() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api-1.1.0");
    let manifest = Manifest::read(&dir).unwrap();
    assert_eq!(manifest.api, "1.1");
    let loaded = runtime().load(&dir, manifest).unwrap();
    assert!(!loaded.has_commands());
    let mut plugin = Instance::new(&loaded, Limits::default()).unwrap();
    plugin.configure(&json!({ "mode": "quiet" })).unwrap();
    let (event, mut context) = opened();
    context.target_folder = Some(4);
    let actions = plugin.handle(&event, &context).unwrap();
    assert!(
        matches!(
            &actions[0],
            Action::SetRating {
                stars: 3,
                mark: false,
                ..
            }
        ),
        "{actions:?}"
    );
    let call = CommandCall {
        command: "next".into(),
        context,
        navigator: None,
    };
    assert!(matches!(plugin.run_command(&call), Err(CallError::Failed(_))));
}

/// Lists a fixed folder: `b.mp4` (the open video), `a.mp4`, `c.mp4` by name.
struct FixedFolder;

impl Navigator for FixedFolder {
    fn list_folder(&self, video: &Path, request: FolderRequest) -> Result<FolderListing, String> {
        assert_eq!(request.file_manager, FileManagerMode::Off);
        assert!(request.videos_only);
        let dir = video.parent().ok_or("no folder")?;
        let files = ["a.mp4", "b.mp4", "c.mp4"]
            .iter()
            .map(|name| FolderFile {
                path: dir.join(name),
                name: (*name).to_owned(),
                has_job: false,
            })
            .collect();
        let order = FolderOrder {
            source: OrderSource::NamesOff,
            role: "text".into(),
            descending: false,
            detail: String::new(),
        };
        Ok(FolderListing {
            files,
            current: Some(1),
            order,
        })
    }
}

#[test]
fn api_1_2_commands_list_the_folder_open_a_video_and_bubble() {
    let loaded = loaded();
    assert!(loaded.has_commands());
    assert_eq!(loaded.manifest().commands[0].id, "next");
    let mut plugin = instance("ok", Limits::default());
    let context = Context {
        video: Some("/videos/b.mp4".into()),
        ..Context::default()
    };
    let call = CommandCall {
        command: "next".into(),
        context: context.clone(),
        navigator: Some(Arc::new(FixedFolder)),
    };
    let outcome = plugin.run_command(&call).unwrap();
    assert_eq!(outcome.open.as_deref(), Some(Path::new("/videos/c.mp4")));
    assert_eq!(outcome.bubbles, ["c.mp4 of 3"]);
    assert!(outcome.actions.is_empty());

    // Without a navigator list-folder is an error the plugin returns.
    let call = CommandCall {
        command: "next".into(),
        context,
        navigator: None,
    };
    assert!(
        matches!(plugin.run_command(&call), Err(CallError::Failed(text)) if text.contains("no video")
        || text.contains("list"))
    );
    // An event never opens anything: open-video works only inside `run`.
    let (event, context) = opened();
    assert!(plugin.handle(&event, &context).is_ok());

    let call = CommandCall {
        command: "slow".into(),
        context: Context::default(),
        navigator: None,
    };
    let mut limited = instance(
        "ok",
        Limits {
            time: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    assert!(matches!(limited.run_command(&call), Err(CallError::Timeout(_))));
}

#[test]
fn a_worker_runs_commands() {
    let worker = Worker::spawn(loaded(), Limits::default()).unwrap();
    configure(&worker, "ok").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let context = Context {
        video: Some("/videos/b.mp4".into()),
        ..Context::default()
    };
    let call = CommandCall {
        command: "next".into(),
        context,
        navigator: Some(Arc::new(FixedFolder)),
    };
    worker.run_command(call, Box::new(move |report| tx.send(report.result).unwrap()));
    let outcome = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(outcome.open.as_deref(), Some(Path::new("/videos/c.mp4")));
}
