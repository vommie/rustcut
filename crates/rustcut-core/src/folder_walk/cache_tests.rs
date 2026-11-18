use std::time::Duration;

use super::*;
use crate::folder_walk::order::SortSpec;

fn variant() -> Variant {
    Variant {
        sort: SortSpec::default(),
        choice: SortingChoice::Natural,
        hidden: false,
        videos_only: true,
    }
}

fn listing(dir: &Path) -> Arc<Vec<PathBuf>> {
    Arc::new(vec![dir.join("a.mp4")])
}

/// The folder's times move by whole ticks of the file system's clock: a test waits a little between changes.
fn later() {
    std::thread::sleep(Duration::from_millis(20));
}

#[test]
fn a_kept_listing_is_used_until_the_folder_changes() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = FolderCache::default();
    let Some(ticket) = cache.begin(dir.path()) else {
        // No watcher here (a network /tmp): nothing is ever kept, which is right.
        assert!(!reliable(dir.path()));
        return;
    };
    cache.put(ticket, variant(), listing(dir.path()));
    assert!(cache.get(dir.path(), &variant()).is_some());
    let other = Variant {
        hidden: true,
        ..variant()
    };
    assert!(cache.get(dir.path(), &other).is_none());

    later();
    std::fs::write(dir.path().join("new.mp4"), b"").expect("file");
    // At once, before the watcher's event arrives: the folder's times moved.
    assert!(cache.get(dir.path(), &variant()).is_none());
}

#[test]
fn a_change_during_the_read_keeps_nothing_and_another_folder_drops_the_old() {
    let one = tempfile::tempdir().expect("dir");
    let two = tempfile::tempdir().expect("dir");
    let cache = FolderCache::default();
    let Some(ticket) = cache.begin(one.path()) else { return };
    later();
    std::fs::write(one.path().join("during.mp4"), b"").expect("file");
    cache.put(ticket, variant(), listing(one.path()));
    assert!(cache.get(one.path(), &variant()).is_none());
    // The watcher's events of that write arrive a moment later; they would (rightly) spoil the next read too.
    std::thread::sleep(Duration::from_millis(200));

    let ticket = cache.begin(one.path()).expect("watched");
    cache.put(ticket, variant(), listing(one.path()));
    assert!(cache.get(one.path(), &variant()).is_some());
    let ticket = cache.begin(two.path()).expect("watched");
    assert!(
        cache.get(one.path(), &variant()).is_none(),
        "the old folder's listing went with its watcher"
    );
    cache.put(ticket, variant(), listing(two.path()));
    assert!(cache.get(two.path(), &variant()).is_some());
}

#[test]
fn a_modified_file_counts_through_the_watcher() {
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("a.mp4");
    std::fs::write(&file, b"1").expect("file");
    let cache = FolderCache::default();
    let Some(ticket) = cache.begin(dir.path()) else { return };
    cache.put(ticket, variant(), listing(dir.path()));
    std::fs::write(&file, b"22").expect("write");
    // The folder's times stay; the watcher's event arrives a moment later.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while cache.get(dir.path(), &variant()).is_some() {
        assert!(std::time::Instant::now() < deadline, "the watcher reported nothing");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn only_local_file_systems_count() {
    assert!(local_kind(0xEF53));
    assert!(local_kind(0x9123_683E));
    assert!(!local_kind(0x6969), "nfs");
    assert!(!local_kind(0xFE53_4D42), "smb2");
    assert!(!local_kind(0x6573_5546), "fuse (sshfs, rclone)");
    assert!(!local_kind(0x794C_7630), "overlay");
}

#[test]
fn reading_the_folder_and_its_files_changes_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("a.mp4");
    std::fs::write(&file, b"1").expect("file");
    let cache = FolderCache::default();
    let Some(ticket) = cache.begin(dir.path()) else { return };
    let _ = std::fs::read_dir(dir.path()).expect("read").count();
    let _ = std::fs::read(&file).expect("read file");
    std::thread::sleep(Duration::from_millis(100));
    cache.put(ticket, variant(), listing(dir.path()));
    assert!(cache.get(dir.path(), &variant()).is_some());
}

#[test]
fn a_change_of_the_watched_folder_is_told() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = FolderCache::default();
    let mut changes = cache.changes();
    if cache.begin(dir.path()).is_none() {
        return;
    }
    assert_eq!(cache.watched_folder().as_deref(), Some(dir.path()));
    std::fs::write(dir.path().join("new.mp4"), b"").expect("file");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
    let told = runtime.block_on(async { tokio::time::timeout(Duration::from_secs(5), changes.changed()).await });
    assert!(matches!(told, Ok(Ok(()))), "no change was told");
}
