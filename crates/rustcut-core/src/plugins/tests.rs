#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::json;

use super::dispatch::apply;
use super::*;
use crate::settings::PluginEntry;

#[test]
fn actions_change_what_the_next_plugin_sees() {
    let mut context = Context::default();
    let folders = BTreeMap::from([(4, PathBuf::from("/videos/sport"))]);
    apply(
        &mut context,
        &Action::SetTarget {
            target: Target::Folder(4),
            reason: String::new(),
            mark: true,
        },
        &folders,
    );
    assert_eq!(context.target, Some(PathBuf::from("/videos/sport")));
    assert_eq!(context.target_folder, Some(4));
    apply(
        &mut context,
        &Action::SetTarget {
            target: Target::Folder(9),
            reason: String::new(),
            mark: true,
        },
        &folders,
    );
    assert_eq!(
        context.target,
        Some(PathBuf::from("/videos/sport")),
        "an unknown folder changes nothing"
    );
    apply(
        &mut context,
        &Action::SetTarget {
            target: Target::Path("/x".into()),
            reason: String::new(),
            mark: true,
        },
        &folders,
    );
    assert_eq!(context.target, Some(PathBuf::from("/x")));
    assert_eq!(context.target_folder, None, "a path is no folder of the list");
    context.video = Some("/videos/holiday/beach.mp4".into());
    apply(
        &mut context,
        &Action::SetTarget {
            target: Target::Folder(0),
            reason: String::new(),
            mark: false,
        },
        &folders,
    );
    assert_eq!(
        context.target,
        Some(PathBuf::from("/videos/holiday")),
        "0 is next to the source"
    );
    assert_eq!(context.target_folder, Some(0));
    apply(
        &mut context,
        &Action::SetRating {
            stars: 9,
            reason: String::new(),
            mark: true,
        },
        &folders,
    );
    assert_eq!(context.rating, Some(5));
}

#[test]
fn the_scan_names_broken_folders_and_skips_hidden_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let installed = tmp.path().join("plugins");
    std::fs::create_dir_all(installed.join("good")).unwrap();
    std::fs::create_dir_all(installed.join(".staging-good")).unwrap();
    std::fs::create_dir_all(installed.join("wrong-name")).unwrap();
    std::fs::create_dir_all(installed.join("empty")).unwrap();
    let manifest = |id: &str| format!("id = \"{id}\"\nname = \"N\"\nversion = \"1.0.0\"\napi = \"1.0\"\n");
    std::fs::write(installed.join("good/plugin.toml"), manifest("good")).unwrap();
    std::fs::write(installed.join("wrong-name/plugin.toml"), manifest("other")).unwrap();
    let found = scan::scan(&installed, &[tmp.path().join("missing-dev")]);
    let summary: Vec<(String, bool)> = found.iter().map(|f| (f.id.clone(), f.manifest.is_ok())).collect();
    assert_eq!(
        summary,
        vec![
            ("empty".into(), false),
            ("good".into(), true),
            ("other".into(), false),
            ("missing-dev".into(), false)
        ]
    );
}

// ---- The whole service with the real test plugin.

fn test_plugin() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rustcut-plugin-host/test-plugin");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "the test plugin must build (rustup target add wasm32-wasip2)"
        );
        dir
    })
}

fn pack(dir: &Path, out: &Path, version: &str) -> PathBuf {
    let src = test_plugin();
    let mut manifest = Manifest::read(src).unwrap();
    manifest.version = semver::Version::parse(version).unwrap();
    let file = out.join(format!("test-plugin-{version}.rcplugin"));
    rustcut_plugin_host::package::pack(&manifest, &src.join(&manifest.wasm), dir, &file).unwrap();
    file
}

async fn changed(
    what: &str,
    events: &mut tokio::sync::broadcast::Receiver<PluginEvent>,
    want: impl Fn(&[PluginInfo]) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(PluginEvent::Changed(list)) = events.recv().await
                && want(&list)
            {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the plugin list never got {what}"));
}

fn opened() -> (Event, Context) {
    (
        Event::VideoOpened(OpenedBy::User),
        Context {
            video: Some("/v/a.mp4".into()),
            ..Context::default()
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_dispatch_pause_reload_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = PluginPaths {
        installed: tmp.path().join("plugins"),
        cache: tmp.path().join("cache"),
    };
    let mut settings = PluginSettings::default();
    let entry = PluginEntry {
        time_limit_ms: 100,
        values: json!({"mode": "ok"}).as_object().unwrap().clone(),
        ..PluginEntry::default()
    };
    settings.entries.insert("test-plugin".into(), entry);
    let service = spawn_plugin_service(&tokio::runtime::Handle::current(), paths, settings.clone(), false);
    let mut events = service.events.subscribe();

    let file = pack(tmp.path(), tmp.path(), "0.2.0");
    let installed = service
        .handle
        .request(|reply| PluginCommand::Install {
            file,
            allow_downgrade: false,
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!((installed.id.as_str(), installed.previous), ("test-plugin", None));
    let list = service.handle.request(PluginCommand::Snapshot).await.unwrap();
    assert!(list.iter().any(|p| p.state == PluginState::Ready), "{list:?}");
    changed("ready", &mut events, |list| {
        list.iter().any(|p| p.state == PluginState::Ready)
    })
    .await;

    let dispatch = || async {
        let (event, context) = opened();
        dispatch_all(&service.handle, event, context, Arc::default()).await
    };
    let outcomes = dispatch().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].actions.len(), 2);

    // An older file is refused unless asked for.
    let older = pack(tmp.path(), tmp.path(), "0.1.0");
    let refused = service
        .handle
        .request(|reply| PluginCommand::Install {
            file: older,
            allow_downgrade: false,
            reply,
        })
        .await;
    assert!(matches!(refused, Ok(Err(InstallError::Downgrade { .. }))));

    // Three timeouts in a row pause it.
    settings.entries.get_mut("test-plugin").unwrap().values = json!({"mode": "slow"}).as_object().unwrap().clone();
    service
        .handle
        .send(PluginCommand::Apply(Box::new(settings.clone())))
        .unwrap();
    let mut failures = 0;
    for _ in 0..3 {
        assert!(dispatch().await.is_empty());
    }
    while let Ok(Ok(event)) = tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
        if let PluginEvent::Failed(failure) = event {
            failures += 1;
            if failure.paused {
                break;
            }
        }
    }
    assert_eq!(failures, 3);
    let list = service.handle.request(PluginCommand::Snapshot).await.unwrap();
    assert!(matches!(list[0].state, PluginState::Paused(_)), "{:?}", list[0].state);
    assert_eq!(list[0].stats.errors, 3);

    // A reload with good settings runs it again.
    settings.entries.get_mut("test-plugin").unwrap().values = json!({"mode": "ok"}).as_object().unwrap().clone();
    service
        .handle
        .send(PluginCommand::Apply(Box::new(settings.clone())))
        .unwrap();
    service
        .handle
        .send(PluginCommand::Reload {
            id: Some("test-plugin".into()),
        })
        .unwrap();
    changed("ready again", &mut events, |list| {
        list.iter().any(|p| p.state == PluginState::Ready)
    })
    .await;
    assert_eq!(dispatch().await.len(), 1);

    // Refused settings make it broken until they change.
    settings.entries.get_mut("test-plugin").unwrap().values = json!({"mode": "refuse"}).as_object().unwrap().clone();
    service
        .handle
        .send(PluginCommand::Apply(Box::new(settings.clone())))
        .unwrap();
    changed("broken", &mut events, |list| {
        list.iter().any(|p| matches!(p.state, PluginState::Broken(_)))
    })
    .await;
    assert!(dispatch().await.is_empty());

    let removed = service
        .handle
        .request(|reply| PluginCommand::Remove {
            id: "test-plugin".into(),
            reply,
        })
        .await
        .unwrap();
    assert_eq!(removed, Ok(()));
    assert!(
        service
            .handle
            .request(PluginCommand::Snapshot)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn safe_mode_and_the_master_switch_run_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = PluginPaths {
        installed: tmp.path().join("plugins"),
        cache: tmp.path().join("cache"),
    };
    let file = pack(tmp.path(), tmp.path(), "0.1.0");
    let package = rustcut_plugin_host::package::Package::read(&file).unwrap();
    package.stage(&paths.installed).unwrap().commit().unwrap();
    let service = spawn_plugin_service(
        &tokio::runtime::Handle::current(),
        paths.clone(),
        PluginSettings::default(),
        true,
    );
    let list = service.handle.request(PluginCommand::Snapshot).await.unwrap();
    assert_eq!(list[0].state, PluginState::SafeMode);
    let off = PluginSettings {
        enabled: false,
        ..PluginSettings::default()
    };
    let service = spawn_plugin_service(&tokio::runtime::Handle::current(), paths, off, false);
    let list = service.handle.request(PluginCommand::Snapshot).await.unwrap();
    assert_eq!(list[0].state, PluginState::AllOff);
}
