//! The sidecar rules on real files in temporary folders.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code"
)]

use std::cell::RefCell;
use std::fs::{self, File, FileTimes};
use std::io;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use md5_sidecar::{
    Carried, Error, Invalid, LEFTOVER_AGE, Lookup, Md5Hex, Stamp, Written, ZERO_BYTE_MD5, copy_along, is_orphan,
    lookup, move_along, orphans_in, path_for, remove_along, trash_along, write,
};

const H: &str = "9e107d9d372bb6826bd81d3542a419d6";
const OTHER: &str = "e4d909c290d0fb1ca068ffaddf22cbd0";

fn hash(text: &str) -> Md5Hex {
    text.parse().unwrap()
}

fn set_mtime(path: &Path, time: SystemTime) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(time))
        .unwrap();
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn video(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"some video bytes").unwrap();
    set_mtime(&path, SystemTime::now() - Duration::from_secs(3600));
    path
}

fn sidecar(video: &Path, content: &str) -> PathBuf {
    let path = path_for(video).unwrap();
    fs::write(&path, content).unwrap();
    path
}

/// A trash that moves into a folder of the test and remembers what it took.
struct FakeTrash {
    dir: tempfile::TempDir,
    taken: RefCell<Vec<PathBuf>>,
}

impl FakeTrash {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            taken: RefCell::new(Vec::new()),
        }
    }

    fn take(&self, path: &Path) -> io::Result<()> {
        let n = self.taken.borrow().len();
        fs::rename(path, self.dir.path().join(n.to_string()))?;
        self.taken.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn taken(&self) -> Vec<PathBuf> {
        self.taken.borrow().clone()
    }
}

#[test]
fn lookup_tells_missing_invalid_stale_and_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    assert_eq!(lookup(&clip).unwrap(), Lookup::Missing);

    let side = sidecar(&clip, "junk");
    assert_eq!(lookup(&clip).unwrap(), Lookup::Invalid(Invalid::NotAHash));

    fs::write(&side, format!("{H}  clip.mp4\n")).unwrap();
    assert_eq!(lookup(&clip).unwrap(), Lookup::Fresh(hash(H)));

    set_mtime(&side, mtime(&clip) - Duration::from_secs(1));
    assert_eq!(lookup(&clip).unwrap(), Lookup::Stale(hash(H)));
    assert_eq!(lookup(&clip).unwrap().fresh(), None);

    set_mtime(&side, mtime(&clip));
    assert_eq!(lookup(&clip).unwrap(), Lookup::Fresh(hash(H)));
}

#[test]
fn lookup_never_follows_links_or_reads_folders() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let elsewhere = dir.path().join("elsewhere");
    fs::write(&elsewhere, H).unwrap();
    symlink(&elsewhere, path_for(&clip).unwrap()).unwrap();
    assert_eq!(lookup(&clip).unwrap(), Lookup::Invalid(Invalid::NotAFile));

    let other = video(dir.path(), "other.mp4");
    fs::create_dir(path_for(&other).unwrap()).unwrap();
    assert_eq!(lookup(&other).unwrap(), Lookup::Invalid(Invalid::NotAFile));
}

#[test]
fn lookup_fails_only_for_a_missing_video() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(lookup(&dir.path().join("gone.mp4")), Err(Error::Io { .. })));
}

#[test]
fn write_creates_the_bare_hash() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let stamp = Stamp::of(&clip).unwrap();
    assert_eq!(write(&clip, hash(H), stamp).unwrap(), Written::Created);
    let side = path_for(&clip).unwrap();
    assert_eq!(fs::read(&side).unwrap(), H.as_bytes());
    assert!(mtime(&side) >= mtime(&clip));
    assert_eq!(lookup(&clip).unwrap(), Lookup::Fresh(hash(H)));
    // Nothing but the sidecar is left in the folder (no temporary file).
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn write_leaves_a_fresh_equal_sidecar_alone() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let side = sidecar(&clip, &format!("{H}  clip.mp4\n"));
    let before = fs::metadata(&side).unwrap();
    assert_eq!(
        write(&clip, hash(H), Stamp::of(&clip).unwrap()).unwrap(),
        Written::Unchanged
    );
    let after = fs::metadata(&side).unwrap();
    assert_eq!(
        (before.ino(), before.modified().unwrap()),
        (after.ino(), after.modified().unwrap())
    );
    // An md5sum line that is fresh and right stays (readers accept both; nothing is migrated).
    assert_eq!(fs::read_to_string(&side).unwrap(), format!("{H}  clip.mp4\n"));
}

#[test]
fn write_rewrites_invalid_and_stale_and_replaces_a_differing_one() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let stamp = Stamp::of(&clip).unwrap();
    let side = sidecar(&clip, "junk");
    assert_eq!(write(&clip, hash(H), stamp).unwrap(), Written::Rewritten);
    assert_eq!(fs::read(&side).unwrap(), H.as_bytes());

    set_mtime(&side, mtime(&clip) - Duration::from_secs(10));
    assert_eq!(write(&clip, hash(H), stamp).unwrap(), Written::Rewritten);
    assert!(mtime(&side) >= mtime(&clip));

    assert_eq!(write(&clip, hash(OTHER), stamp).unwrap(), Written::Replaced(hash(H)));
    assert_eq!(fs::read(&side).unwrap(), OTHER.as_bytes());
}

#[test]
fn write_refuses_a_video_that_changed_after_the_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let stamp = Stamp::of(&clip).unwrap();
    fs::write(&clip, b"other bytes, longer than before").unwrap();
    assert!(matches!(write(&clip, hash(H), stamp), Err(Error::VideoChanged(_))));
    assert!(!path_for(&clip).unwrap().exists());
}

#[test]
fn write_refuses_an_impossible_zero_byte_hash() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    let stamp = Stamp::of(&clip).unwrap();
    assert!(matches!(
        write(&clip, hash(ZERO_BYTE_MD5), stamp),
        Err(Error::ImpossibleHash { .. })
    ));

    let empty = dir.path().join("empty.mp4");
    File::create(&empty).unwrap();
    let stamp = Stamp::of(&empty).unwrap();
    assert!(matches!(
        write(&empty, hash(H), stamp),
        Err(Error::ImpossibleHash { .. })
    ));
    // Rule 8: a zero-byte video may have a sidecar.
    assert_eq!(write(&empty, hash(ZERO_BYTE_MD5), stamp).unwrap(), Written::Created);
}

#[test]
fn a_video_from_the_future_still_gets_a_fresh_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    set_mtime(&clip, SystemTime::now() + Duration::from_hours(24));
    assert_eq!(
        write(&clip, hash(H), Stamp::of(&clip).unwrap()).unwrap(),
        Written::Created
    );
    assert_eq!(lookup(&clip).unwrap(), Lookup::Fresh(hash(H)));
}

#[test]
fn move_along_renames_the_sidecar_with_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let trash = FakeTrash::new();
    let clip = video(dir.path(), "clip.mp4");
    let side = sidecar(&clip, &format!("{H}  clip.mp4\n"));
    let time = mtime(&side);
    fs::create_dir(dir.path().join("sub")).unwrap();
    let moved = dir.path().join("sub/renamed.mp4");
    fs::rename(&clip, &moved).unwrap();
    assert_eq!(
        move_along(&clip, &moved, &mut |p| trash.take(p)).unwrap(),
        Carried::Moved
    );
    assert!(!side.exists());
    let new_side = path_for(&moved).unwrap();
    assert_eq!(mtime(&new_side), time);
    assert_eq!(lookup(&moved).unwrap(), Lookup::Fresh(hash(H)));
    assert!(trash.taken().is_empty());
}

#[test]
fn move_along_trashes_an_invalid_sidecar_and_an_orphan_at_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let trash = FakeTrash::new();
    let clip = video(dir.path(), "clip.mp4");
    let side = sidecar(&clip, "junk");
    let moved = dir.path().join("moved.mp4");
    fs::rename(&clip, &moved).unwrap();
    assert_eq!(
        move_along(&clip, &moved, &mut |p| trash.take(p)).unwrap(),
        Carried::Trashed
    );
    assert_eq!(trash.taken(), vec![side]);

    let a = video(dir.path(), "a.mp4");
    sidecar(&a, H);
    let b = dir.path().join("b.mp4");
    let orphan = sidecar(&b, OTHER);
    fs::rename(&a, &b).unwrap();
    assert_eq!(move_along(&a, &b, &mut |p| trash.take(p)).unwrap(), Carried::Moved);
    assert_eq!(trash.taken().last(), Some(&orphan));
    assert_eq!(fs::read(path_for(&b).unwrap()).unwrap(), H.as_bytes());
}

#[test]
fn move_along_keeps_a_fresh_equal_sidecar_at_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let trash = FakeTrash::new();
    let a = video(dir.path(), "a.mp4");
    let side_a = sidecar(&a, H);
    let b = dir.path().join("b.mp4");
    fs::rename(&a, &b).unwrap();
    let side_b = sidecar(&b, H);
    assert_eq!(move_along(&a, &b, &mut |p| trash.take(p)).unwrap(), Carried::Kept);
    assert!(!side_a.exists());
    assert!(side_b.exists());
    assert!(trash.taken().is_empty());
}

#[test]
fn move_along_without_a_sidecar_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let a = video(dir.path(), "a.mp4");
    let b = dir.path().join("b.mp4");
    fs::rename(&a, &b).unwrap();
    assert_eq!(
        move_along(&a, &b, &mut |_| panic!("nothing to trash")).unwrap(),
        Carried::Nothing
    );
    assert_eq!(
        move_along(&b, &b, &mut |_| panic!("nothing to trash")).unwrap(),
        Carried::Nothing
    );
}

#[test]
fn move_along_crosses_file_systems() {
    let Ok(other_fs) = tempfile::tempdir_in("/dev/shm") else {
        eprintln!("SKIPPED: no /dev/shm");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    if fs::metadata(dir.path()).unwrap().dev() == fs::metadata(other_fs.path()).unwrap().dev() {
        eprintln!("SKIPPED: the temporary folder and /dev/shm share a file system");
        return;
    }
    let clip = video(dir.path(), "clip.mp4");
    let side = sidecar(&clip, &format!("{}  clip.mp4\n", H.to_ascii_uppercase()));
    let time = mtime(&side);
    let moved = other_fs.path().join("clip.mp4");
    fs::copy(&clip, &moved).unwrap();
    set_mtime(&moved, mtime(&clip));
    fs::remove_file(&clip).unwrap();
    assert_eq!(
        move_along(&clip, &moved, &mut |_| panic!("nothing to trash")).unwrap(),
        Carried::Copied
    );
    assert!(!side.exists());
    let new_side = path_for(&moved).unwrap();
    assert_eq!(fs::read(&new_side).unwrap(), H.as_bytes());
    assert_eq!(mtime(&new_side), time);
}

#[test]
fn copy_along_writes_the_canonical_form_with_the_old_time() {
    let dir = tempfile::tempdir().unwrap();
    let trash = FakeTrash::new();
    let clip = video(dir.path(), "clip.mp4");
    let side = sidecar(&clip, &format!("{H}  clip.mp4\n"));
    let time = mtime(&side);
    let copy = dir.path().join("copy.mp4");
    fs::copy(&clip, &copy).unwrap();
    set_mtime(&copy, mtime(&clip));
    assert_eq!(
        copy_along(&clip, &copy, &mut |p| trash.take(p)).unwrap(),
        Carried::Copied
    );
    assert!(side.exists());
    let new_side = path_for(&copy).unwrap();
    assert_eq!(fs::read(&new_side).unwrap(), H.as_bytes());
    assert_eq!(mtime(&new_side), time);
    assert_eq!(lookup(&copy).unwrap(), Lookup::Fresh(hash(H)));
    // A second copy finds it there already.
    assert_eq!(copy_along(&clip, &copy, &mut |p| trash.take(p)).unwrap(), Carried::Kept);
}

#[test]
fn copy_along_skips_an_invalid_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    sidecar(&clip, "junk");
    let copy = dir.path().join("copy.mp4");
    fs::copy(&clip, &copy).unwrap();
    assert_eq!(
        copy_along(&clip, &copy, &mut |_| panic!("nothing to trash")).unwrap(),
        Carried::Nothing
    );
    assert!(!path_for(&copy).unwrap().exists());
}

#[test]
fn a_copied_video_with_a_new_time_gets_a_stale_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let clip = video(dir.path(), "clip.mp4");
    sidecar(&clip, H);
    let copy = dir.path().join("copy.mp4");
    fs::copy(&clip, &copy).unwrap();
    set_mtime(&copy, SystemTime::now());
    assert_eq!(
        copy_along(&clip, &copy, &mut |_| panic!("nothing to trash")).unwrap(),
        Carried::Copied
    );
    assert_eq!(lookup(&copy).unwrap(), Lookup::Stale(hash(H)));
}

#[test]
fn trash_and_remove_follow_the_video() {
    let dir = tempfile::tempdir().unwrap();
    let trash = FakeTrash::new();
    let a = video(dir.path(), "a.mp4");
    let side_a = sidecar(&a, H);
    trash.take(&a).unwrap();
    assert_eq!(trash_along(&a, &mut |p| trash.take(p)).unwrap(), Carried::Trashed);
    assert_eq!(trash.taken(), vec![a.clone(), side_a]);
    assert_eq!(trash_along(&a, &mut |p| trash.take(p)).unwrap(), Carried::Nothing);

    let b = video(dir.path(), "b.mp4");
    let side_b = sidecar(&b, H);
    fs::remove_file(&b).unwrap();
    assert_eq!(remove_along(&b).unwrap(), Carried::Removed);
    assert!(!side_b.exists());
    assert_eq!(remove_along(&b).unwrap(), Carried::Nothing);
}

#[test]
fn orphans_are_sidecars_without_their_video_and_old_write_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let kept = video(dir.path(), "kept.mp4");
    let kept_side = sidecar(&kept, H);
    let gone_side = sidecar(&dir.path().join("gone.mp4"), H);
    let old_leftover = dir.path().join("..gone.mp4.md5.Ab12Cd");
    fs::write(&old_leftover, "").unwrap();
    set_mtime(&old_leftover, SystemTime::now() - LEFTOVER_AGE - Duration::from_secs(1));
    let young_leftover = dir.path().join("..kept.mp4.md5.Xy34Zw");
    fs::write(&young_leftover, "").unwrap();
    fs::write(dir.path().join("notes.md5"), H).unwrap();

    assert!(!is_orphan(&kept_side).unwrap());
    assert!(is_orphan(&gone_side).unwrap());
    assert!(!is_orphan(&kept).unwrap());
    let mut want = vec![gone_side, old_leftover];
    want.sort();
    assert_eq!(orphans_in(dir.path()).unwrap(), want);
}
