use std::path::Path;

use super::*;
use crate::folder_walk::natural::NameKey;

fn touch(dir: &Path, name: &str, size: usize, modified: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, vec![0_u8; size]).expect("file");
    let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("open")
        .set_modified(time)
        .expect("mtime");
    path
}

fn names(listing: &Listing) -> Vec<String> {
    listing
        .files
        .iter()
        .map(|f| f.file_name().unwrap_or_default().to_string_lossy().into_owned())
        .collect()
}

fn folder() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temp");
    let dir = root.path().join("home/videos");
    std::fs::create_dir_all(&dir).expect("dir");
    touch(&dir, "File a10.mp4", 10, 300);
    touch(&dir, "File a2.mkv", 30, 100);
    let current = touch(&dir, "File D.mp4", 20, 200);
    touch(&dir, ".probe.mp4", 5, 400);
    touch(&dir, "notes.txt", 1, 500);
    std::fs::create_dir_all(dir.join("sub.mp4")).expect("folder named like a video");
    (root, current)
}

fn request(current: &Path, mode: FileManagerMode, hidden: HiddenFiles) -> Request {
    Request {
        current: current.to_path_buf(),
        file_manager: mode,
        hidden,
        videos_only: true,
    }
}

#[tokio::test]
async fn names_a_to_z_videos_only_without_hidden_files() {
    let (_root, current) = folder();
    let listing = list(&request(&current, FileManagerMode::Off, HiddenFiles::Skip), None, None)
        .await
        .expect("list");
    assert_eq!(names(&listing), ["File a2.mkv", "File a10.mp4", "File D.mp4"]);
    assert_eq!(listing.current, Some(2));
    assert_eq!(listing.source, OrderSource::Names(NamesBecause::Off));
}

#[tokio::test]
async fn all_files_and_hidden_ones_when_asked() {
    let (_root, current) = folder();
    let mut req = request(&current, FileManagerMode::Off, HiddenFiles::Show);
    req.videos_only = false;
    let listing = list(&req, None, None).await.expect("list");
    assert_eq!(
        names(&listing),
        [".probe.mp4", "File a2.mkv", "File a10.mp4", "File D.mp4", "notes.txt"]
    );
}

#[tokio::test]
async fn dolphins_stored_order_and_hidden_files_without_dolphin() {
    let (root, current) = folder();
    let paths = props::DolphinPaths {
        config_home: root.path().join("config"),
        data_home: root.path().join("data"),
        home: root.path().join("home"),
        downloads: None,
    };
    std::fs::create_dir_all(&paths.config_home).expect("config");
    std::fs::write(
        paths.config_home.join("dolphinrc"),
        "[General]\nGlobalViewProps=false\n",
    )
    .expect("rc");
    let dir = current.parent().expect("dir");
    let text =
        "[Dolphin]\nSortRole=modificationtime\nSortOrder=1\nSortHiddenLast=true\n[Settings]\nHiddenFilesShown=true\n";
    xattr::set(dir, props::XATTR, text.as_bytes()).expect("xattr");
    let req = request(&current, FileManagerMode::Always, HiddenFiles::LikeFileManager);
    let listing = list(&req, Some(&paths), None).await.expect("list");
    assert_eq!(
        names(&listing),
        ["File a10.mp4", "File D.mp4", "File a2.mkv", ".probe.mp4"]
    );
    assert_eq!(listing.source, OrderSource::FileManager);
    assert_eq!(
        listing.sort,
        SortSpec {
            role: SortRole::Modified,
            descending: true,
            hidden_last: true
        }
    );

    xattr::set(dir, props::XATTR, b"[Dolphin]\nSortRole=rating\n").expect("xattr");
    let listing = list(&req, Some(&paths), None).await.expect("list");
    assert_eq!(
        listing.source,
        OrderSource::Names(NamesBecause::Unsupported("rating".into()))
    );
    assert_eq!(names(&listing), ["File a2.mkv", "File a10.mp4", "File D.mp4"]);
}

#[tokio::test]
async fn a_hidden_current_file_still_has_its_place() {
    let (_root, current) = folder();
    let hidden = current.with_file_name(".probe.mp4");
    let listing = list(&request(&hidden, FileManagerMode::Off, HiddenFiles::Skip), None, None)
        .await
        .expect("list");
    assert_eq!(listing.current, Some(0));
    assert_eq!(listing.files.len(), 4);
}

#[tokio::test]
async fn a_path_with_dots_is_cleaned() {
    let (_root, current) = folder();
    let dir = current.parent().expect("dir");
    let dotted = dir.join("../videos/File D.mp4");
    let listing = list(&request(&dotted, FileManagerMode::Off, HiddenFiles::Skip), None, None)
        .await
        .expect("list");
    assert_eq!(listing.current, Some(2));
    assert!(listing.files.iter().all(|f| !f.to_string_lossy().contains("..")));
}

/// Against a real Dolphin: `RC_DOLPHIN_VIDEO=/path/video.mp4 cargo test -p rustcut-core live_dolphin -- --ignored
/// --nocapture` prints what Dolphin says and the order.
#[tokio::test]
#[ignore = "needs a running Dolphin that shows the folder of $RC_DOLPHIN_VIDEO"]
async fn live_dolphin() {
    let video = PathBuf::from(std::env::var("RC_DOLPHIN_VIDEO").expect("RC_DOLPHIN_VIDEO"));
    let folder = video.parent().expect("folder").to_path_buf();
    println!("bus: {:?}", bus::ask(&folder, true).await);
    let paths = props::DolphinPaths::of_user();
    for mode in [
        FileManagerMode::OpenTab,
        FileManagerMode::Running,
        FileManagerMode::Always,
    ] {
        let listing = list(
            &request(&video, mode, HiddenFiles::LikeFileManager),
            paths.as_ref(),
            None,
        )
        .await
        .expect("list");
        println!(
            "{mode:?}: {:?} {:?} {:?}",
            listing.source,
            listing.sort,
            names(&listing)
        );
    }
}

/// Where the time of a large listing goes: `RC_PROFILE_DIR=<folder> cargo test -p rustcut-core --release
/// profile_listing -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement on a folder of the bench (benches/folder_walk.rs makes them)"]
fn profile_listing() {
    use std::time::Instant;
    let dir = PathBuf::from(std::env::var("RC_PROFILE_DIR").expect("RC_PROFILE_DIR"));
    let t = Instant::now();
    let names: Vec<(PathBuf, String)> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|e| (e.path(), e.file_name().to_string_lossy().into_owned()))
        .collect();
    println!("read_dir + names: {:?} ({} entries)", t.elapsed(), names.len());
    let t = Instant::now();
    let types = names.iter().filter(|(p, _)| p.is_file()).count();
    println!("stat each: {:?} ({types})", t.elapsed());
    let t = Instant::now();
    let order = NameOrder::for_environment(SortingChoice::Natural);
    let keys: Vec<NameKey> = names.iter().map(|(_, n)| order.key(n)).collect();
    println!("keys with sort keys: {:?}", t.elapsed());
    let t = Instant::now();
    let mut sorted = keys.clone();
    sorted.sort_unstable_by(|a, b| order.compare_keys(a, b));
    println!("sort, one thread: {:?}", t.elapsed());
    let t = Instant::now();
    let mut sorted = keys;
    rayon::slice::ParallelSliceMut::par_sort_unstable_by(sorted.as_mut_slice(), |a, b| order.compare_keys(a, b));
    println!("sort, all cores: {:?}", t.elapsed());
}
