use std::path::{Path, PathBuf};

use super::*;

struct Env {
    _root: tempfile::TempDir,
    paths: DolphinPaths,
}

fn env(dolphinrc: &str) -> Env {
    let root = tempfile::tempdir().expect("temp dir");
    let paths = DolphinPaths {
        config_home: root.path().join("config"),
        data_home: root.path().join("data"),
        home: root.path().join("home"),
        downloads: Some(root.path().join("home/Downloads")),
    };
    for dir in [&paths.config_home, &paths.data_home, &paths.home] {
        std::fs::create_dir_all(dir).expect("dir");
    }
    std::fs::write(paths.config_home.join("dolphinrc"), dolphinrc).expect("rc");
    Env { _root: root, paths }
}

fn folder(env: &Env, name: &str) -> PathBuf {
    let dir = env.paths.home.join(name);
    std::fs::create_dir_all(&dir).expect("folder");
    dir
}

fn store(place: &Path, text: &str) {
    std::fs::create_dir_all(place).expect("place");
    xattr::set(place, XATTR, text.as_bytes()).expect("xattr (tmpfs and ext4/btrfs support user attributes)");
}

const PER_FOLDER: &str = "[General]\nGlobalViewProps=false\nVersion=202\n";

#[test]
fn global_mode_uses_the_global_store_for_every_folder() {
    let env = env("[General]\nVersion=202\n");
    let a = folder(&env, "A");
    store(&a, "[Dolphin]\nSortRole=size\n");
    store(
        &env.paths.data_home.join("dolphin/view_properties/global"),
        "[Dolphin]\nSortOrder=1\nSortRole=modificationtime\nVersion=4\n",
    );
    let view = stored_view(&a, &env.paths);
    assert_eq!(view.sort.role, SortRole::Modified);
    assert!(view.sort.descending);
}

#[test]
fn a_folder_keeps_its_own_order_and_hidden_files() {
    let env = env(PER_FOLDER);
    let c = folder(&env, "C");
    store(
        &c,
        "[Dolphin]\nSortRole=size\nSortHiddenLast=true\nVersion=4\n\n[Settings]\nHiddenFilesShown=true\n",
    );
    let view = stored_view(&c, &env.paths);
    assert_eq!(
        view.sort,
        SortSpec {
            role: SortRole::Size,
            descending: false,
            hidden_last: true
        }
    );
    assert!(view.hidden_shown);
}

#[test]
fn a_directory_file_wins_over_the_attribute() {
    let env = env(PER_FOLDER);
    let c = folder(&env, "C");
    store(&c, "[Dolphin]\nSortRole=size\n");
    std::fs::write(c.join(".directory"), "[Dolphin]\nSortRole=type\nSortOrder=1\n").expect("file");
    let view = stored_view(&c, &env.paths);
    assert_eq!(view.sort.role, SortRole::Type);
    assert!(view.sort.descending);
}

#[test]
fn nothing_stored_or_stale_shows_the_global_style() {
    let env = env("[General]\nGlobalViewProps=false\nViewPropsTimestamp=2026,11,17,18,0,0\n");
    store(
        &env.paths.data_home.join("dolphin/view_properties/global"),
        "[Dolphin]\nSortRole=size\nSortOrder=1\n",
    );
    let fresh = folder(&env, "fresh");
    assert_eq!(stored_view(&fresh, &env.paths).sort.role, SortRole::Size);
    let stale = folder(&env, "stale");
    store(&stale, "[Dolphin]\nSortRole=type\nTimestamp=2026,11,17,17,59,59.5\n");
    assert_eq!(stored_view(&stale, &env.paths).sort.role, SortRole::Size);
    let newer = folder(&env, "newer");
    store(&newer, "[Dolphin]\nSortRole=type\nTimestamp=2026,11,17,18,0,1.25\n");
    assert_eq!(stored_view(&newer, &env.paths).sort.role, SortRole::Type);
}

#[test]
fn downloads_start_newest_first_and_defaults_are_names() {
    let env = env(PER_FOLDER);
    let downloads = folder(&env, "Downloads");
    let view = stored_view(&downloads, &env.paths);
    assert_eq!(
        view.sort,
        SortSpec {
            role: SortRole::Modified,
            descending: true,
            hidden_last: false
        }
    );
    let plain = folder(&env, "plain");
    let view = stored_view(&plain, &env.paths);
    assert_eq!(view.sort, SortSpec::default());
    assert!(!view.hidden_shown);
    assert_eq!(view.choice, SortingChoice::Natural);
}

#[test]
fn folders_outside_home_use_the_hashed_store() {
    let env = env("[General]\nGlobalViewProps=false\nSortingChoice=2\n");
    let outside = env.paths.config_home.join("media");
    std::fs::create_dir_all(&outside).expect("outside");
    let hash = folder_hash(&outside).expect("hash");
    store(
        &env.paths.data_home.join("dolphin/view_properties/local").join(hash),
        "[Dolphin]\nSortRole=accesstime\n",
    );
    let view = stored_view(&outside, &env.paths);
    assert_eq!(view.sort.role, SortRole::Accessed);
    assert_eq!(view.choice, SortingChoice::CaseSensitive);
}

#[test]
fn kconfig_groups_keys_and_escapes() {
    let groups = parse_kconfig("[MainWindow][Toolbar mainToolBar]\nIconSize=16\n[General]\nName[$e]=a\\sb\\\\c\n# x\n");
    assert_eq!(groups["MainWindow][Toolbar mainToolBar"]["IconSize"], "16");
    assert_eq!(groups["General"]["Name"], "a b\\c");
}
