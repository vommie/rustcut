//! Contract tests of the RustView CLI transport: the answers recorded from the real RustView (0.3.86,
//! `tests/fixtures/rustview/`, paths shortened to `/videos/...`) decode into RustCut's models with the exit codes RustView
//! uses, and the transport runs a scripted `rustview` with the exact command line, exit codes, the time limit and a
//! missing program. The same calls against a real RustView and a test library are in `rustview_live.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use rustcut_core::rustview::{
    BatchHashCheck, CliClient, DeleteMedia, GetMedia, HashCheck, HashInsert, RustViewClient as _, RustViewError,
    SetMedia, SetMediaAction, TagTree, decode,
};
use rustcut_core::settings::RustViewSettings;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rustview")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn ok<T: serde::de::DeserializeOwned>(name: &str, code: i32) -> T {
    decode(Some(code), &fixture(&format!("{name}.json")), b"").unwrap()
}

#[test]
fn the_recorded_answers_decode() {
    let tree: TagTree = ok("tag_tree", 0);
    assert_eq!((tree.success, tree.total_tags, tree.tags.len()), (true, 10, 10));
    assert_eq!(tree.tags[3].parent_id, Some(2));
    assert_eq!(tree.tags[3].name, "Japan");

    let missing: GetMedia = ok("get_media_missing", 2);
    assert!(missing.success && !missing.found && missing.media.is_none());
    assert!(missing.message.unwrap().contains("not registered"));

    let created: SetMedia = ok("set_media_created", 0);
    assert_eq!(created.action, SetMediaAction::Created);
    let mut tags = created.tags.clone();
    tags.sort_unstable();
    assert_eq!(tags, [4, 5], "the unknown tag 99 was left out by RustView");
    assert_eq!(created.source.as_deref(), Some("RustCut"));
    assert_eq!(created.file_hash, "8f618d1ebd606025257057ee18536e64");

    let stub: GetMedia = ok("get_media_stub", 0);
    let media = stub.media.unwrap();
    assert_eq!(
        (
            media.full_path.as_str(),
            media.file_size,
            media.width,
            media.is_corrupted
        ),
        ("/videos/out/clip - 01.mkv", 0, 0, false),
        "a pending stub"
    );

    let updated: SetMedia = ok("set_media_updated", 0);
    assert_eq!(
        (updated.action, updated.rating, updated.tags.len()),
        (SetMediaAction::Updated, 2, 0)
    );

    let file: GetMedia = ok("get_media_existing_file", 0);
    let media = file.media.unwrap();
    assert_eq!((media.width, media.height, media.file_size), (1280, 720, 10_341_341));
    assert_eq!(
        media.file_hash, "ccec6b36fb7daf1f41d7eb2437de81a0",
        "RustView's MD5 of the file"
    );

    let known: HashCheck = ok("check_hash_known", 0);
    assert!(
        known.known && known.hash_id.is_none(),
        "known through a record, not the hash list"
    );
    assert_eq!(known.associated_files, ["/videos/out/clip - 01.mkv"]);
    let unknown: HashCheck = ok("check_hash_unknown", 2);
    assert!(!unknown.known && unknown.associated_files.is_empty());
    let registered: HashCheck = ok("check_hash_registered", 0);
    assert_eq!(
        (registered.hash_id, registered.date_time.as_deref()),
        (Some(1), Some("2026-10-23 18:31:49"))
    );

    let batch: BatchHashCheck = ok("check_hashes", 0);
    assert_eq!((batch.known_count, batch.unknown_count, batch.results.len()), (1, 1, 2));

    let new: HashInsert = ok("insert_hash_new", 0);
    let again: HashInsert = ok("insert_hash_again", 0);
    assert!(new.is_new && !again.is_new && new.hash_id == again.hash_id);

    let deleted: DeleteMedia = ok("delete_media_deleted", 0);
    assert!(deleted.deleted && deleted.media_id.is_some());
    let not_there: DeleteMedia = ok("delete_media_missing", 2);
    assert!(!not_there.deleted && not_there.media_id.is_none());
}

#[test]
fn refusals_carry_rustviews_reason() {
    let refused = decode::<HashInsert>(Some(1), b"", &fixture("insert_hash_invalid.stderr")).unwrap_err();
    assert_eq!(
        refused,
        RustViewError::Api(
            "Invalid argument: Invalid MD5 hash format. Expected 32-character hexadecimal string.".into()
        )
    );
    assert!(!refused.is_transient());
    let rating = decode::<SetMedia>(Some(1), b"", &fixture("set_media_bad_rating.stderr")).unwrap_err();
    assert!(rating.to_string().contains("Rating must be an integer"), "{rating}");
}

/// A `rustview` stand-in: it logs its arguments (one per line) and answers with `stdout`, `stderr` and `code`.
fn script(dir: &Path, stdout: &str, stderr: &str, code: i32, sleep: u32) -> PathBuf {
    let log = dir.join("args.log");
    let out = dir.join("answer.json");
    std::fs::write(&out, stdout).unwrap();
    let path = dir.join("rustview");
    let body = format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{log}'\nsleep {sleep}\ncat '{out}'\nprintf '%s' '{stderr}' >&2\nexit {code}\n",
        log = log.display(),
        out = out.display(),
    );
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn client(program: PathBuf, timeout_secs: u8) -> CliClient {
    CliClient::new(&RustViewSettings {
        binary: Some(program),
        timeout_secs,
        ..RustViewSettings::default()
    })
}

fn args(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("args.log"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn the_transport_runs_rustview_api_with_the_exact_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let answer = String::from_utf8(fixture("set_media_created.json")).unwrap();
    let program = script(dir.path(), &answer, "", 0, 0);
    let set = client(program.clone(), 5)
        .set_own_media(
            Path::new("/videos/out/-clip 01.mkv"),
            Some(4),
            Some(&[4, 5]),
            Some("8f618d1ebd606025257057ee18536e64"),
        )
        .await
        .unwrap();
    assert_eq!(set.media_id, 657);
    assert_eq!(
        args(dir.path()),
        [
            "api",
            "set-media",
            "--path=/videos/out/-clip 01.mkv",
            "--rating=4",
            "--tags=4,5",
            "--hash=8f618d1ebd606025257057ee18536e64",
            "--source=RustCut",
            "--json"
        ]
    );

    let missing = String::from_utf8(fixture("get_media_missing.json")).unwrap();
    let program = script(dir.path(), &missing, "", 2, 0);
    let found = client(program, 5)
        .get_media(Path::new("/videos/out/clip - 01.mkv"))
        .await
        .unwrap();
    assert!(!found.found, "exit code 2 is an answer");
    assert_eq!(
        args(dir.path()),
        ["api", "get-media", "--path=/videos/out/clip - 01.mkv", "--json"]
    );
}

#[tokio::test]
async fn refusals_timeouts_and_a_missing_program_are_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(
        dir.path(),
        "",
        "API Error: Invalid argument: Invalid MD5 hash format.",
        1,
        0,
    );
    let refused = client(program, 5).insert_hash("xyz").await.unwrap_err();
    assert_eq!(
        refused,
        RustViewError::Api("Invalid argument: Invalid MD5 hash format.".into())
    );

    let program = script(dir.path(), "{}", "", 0, 30);
    let started = std::time::Instant::now();
    let slow = client(program, 1).tag_tree().await.unwrap_err();
    assert_eq!(slow, RustViewError::Timeout(1));
    assert!(slow.is_transient());
    assert!(started.elapsed().as_secs() < 5, "the child was killed at the limit");

    let gone = client(dir.path().join("not-there"), 5).tag_tree().await.unwrap_err();
    assert!(
        gone.is_transient() && gone.to_string().contains("not installed"),
        "{gone}"
    );
}
