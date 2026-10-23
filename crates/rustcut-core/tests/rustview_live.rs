//! The sync actor against a real RustView (`rustview api`) and a **test** library: never the user's library. The test
//! runs only through `scripts/rustview-live-test.sh` (it is ignored otherwise, because RustView is not a build
//! dependency): the script names the binary (`RUSTCUT_RUSTVIEW_BIN`) and optionally a library to start from
//! (`RUSTCUT_RUSTVIEW_LIBRARY`, a `library.lmdb` folder that is copied, never opened in place). Every `rustview` child
//! gets `XDG_DATA_HOME`/`XDG_CONFIG_HOME` in a temporary folder, which is where RustView keeps its library.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rustcut_core::job::{JobId, JobMetadata};
use rustcut_core::rustview::{
    CliClient, JobSyncRecord, RustViewClient as _, SOURCE_APP, SyncCommand, SyncPhase, SyncRecords, SyncState,
    spawn_sync,
};
use rustcut_core::settings::RustViewSettings;

const HASH: &str = "8f618d1ebd606025257057ee18536e64";
const WAIT: Duration = Duration::from_secs(60);

struct Env {
    _home: tempfile::TempDir,
    data: PathBuf,
    config: PathBuf,
    out: PathBuf,
    settings: RustViewSettings,
}

impl Env {
    fn new() -> Self {
        let binary = std::env::var_os("RUSTCUT_RUSTVIEW_BIN")
            .map(PathBuf::from)
            .expect("RUSTCUT_RUSTVIEW_BIN names the rustview binary (run scripts/rustview-live-test.sh)");
        let home = tempfile::tempdir().unwrap();
        let data = home.path().join("data");
        let config = home.path().join("config");
        let library = data.join("rustview/library.lmdb");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        if let Some(source) = std::env::var_os("RUSTCUT_RUSTVIEW_LIBRARY").map(PathBuf::from) {
            for file in std::fs::read_dir(&source).unwrap() {
                let file = file.unwrap().path();
                std::fs::copy(&file, library.join(file.file_name().unwrap())).unwrap();
            }
        }
        let out = home.path().join("videos/out");
        std::fs::create_dir_all(&out).unwrap();
        Self {
            data,
            config,
            out,
            settings: RustViewSettings {
                binary: Some(binary),
                timeout_secs: 20,
                ..RustViewSettings::default()
            },
            _home: home,
        }
    }

    fn client(&self) -> CliClient {
        let data = self.data.clone();
        let config = self.config.clone();
        CliClient::new(&self.settings)
            .with_env("XDG_DATA_HOME", data)
            .with_env("XDG_CONFIG_HOME", config)
    }
}

async fn until(records: &mut SyncRecords, id: i64, done: impl Fn(&JobSyncRecord) -> bool) -> JobSyncRecord {
    tokio::time::timeout(WAIT, async {
        loop {
            let (job, record) = records.recv().await.unwrap();
            if job == JobId(id) && done(&record) {
                return record;
            }
        }
    })
    .await
    .unwrap()
}

fn send(sync: &rustcut_core::rustview::SyncService, id: i64, target: &Path, metadata: &JobMetadata, phase: SyncPhase) {
    sync.handle
        .send(SyncCommand::Job {
            id: JobId(id),
            target: target.to_path_buf(),
            metadata: metadata.clone(),
            phase,
            record: JobSyncRecord::default(),
        })
        .unwrap();
}

fn cleanup(sync: &rustcut_core::rustview::SyncService, id: i64) {
    sync.handle
        .send(SyncCommand::Cleanup {
            id: JobId(id),
            record: JobSyncRecord::default(),
        })
        .unwrap();
}

#[tokio::test]
#[ignore = "needs a RustView binary and a test library: scripts/rustview-live-test.sh"]
async fn the_sync_actor_keeps_a_real_rustview_library_in_step() {
    let env = Env::new();
    let client = env.client();
    let tree = client.tag_tree().await.unwrap();
    let tags: Vec<u32> = tree.tags.iter().map(|t| t.id).take(2).collect();
    eprintln!("[live] RustView answers; the test library has {} tags", tree.tags.len());

    let make = {
        let client = env.client();
        move |_: &RustViewSettings| client.clone()
    };
    let (sync, mut records, _task) = spawn_sync(&tokio::runtime::Handle::current(), env.settings.clone(), make);
    let metadata = JobMetadata {
        tags: tags.clone(),
        rating: Some(4),
        source_hash: Some(HASH.into()),
    };

    // 1. Queued: a pending stub with RustCut as its source and the source hash; the hash is in the hash list.
    let a = env.out.join("clip - 01.mkv");
    send(&sync, 1, &a, &metadata, SyncPhase::Enqueued);
    let record = until(&mut records, 1, |r| r.state == SyncState::Synced).await;
    assert_eq!(record.preexisting, Some(false));
    assert!(record.hash_registered);
    let stub = client.get_media(&a).await.unwrap().media.unwrap();
    assert_eq!(stub.source.as_deref(), Some(SOURCE_APP));
    assert_eq!(stub.file_hash, HASH);
    assert_eq!(stub.rating, 4);
    let mut stored = stub.tags.clone();
    stored.sort_unstable();
    assert_eq!(stored, tags);
    let check = client.check_hash(HASH).await.unwrap();
    assert!(check.known && check.hash_id.is_some());

    // 2. Rendered: the file exists now and the record is asserted once more (RustView reads its size).
    std::fs::write(&a, vec![0_u8; 4096]).unwrap();
    send(&sync, 1, &a, &metadata, SyncPhase::Finished);
    until(&mut records, 1, |r| r.finished).await;
    assert_eq!(client.get_media(&a).await.unwrap().media.unwrap().file_size, 4096);

    // 3. A record that existed before (a file RustView knows): written, then given back on cleanup, never deleted.
    let b = env.out.join("existing.mkv");
    client.set_media(&b, Some(1), Some(&[]), None).await.unwrap();
    send(&sync, 2, &b, &metadata, SyncPhase::Enqueued);
    let record = until(&mut records, 2, |r| r.state == SyncState::Synced).await;
    assert_eq!((record.preexisting, record.previous_rating), (Some(true), Some(1)));
    assert_eq!(client.get_media(&b).await.unwrap().media.unwrap().rating, 4);
    cleanup(&sync, 2);
    until(&mut records, 2, |r| r.state == SyncState::CleanedUp).await;
    let back = client.get_media(&b).await.unwrap().media.unwrap();
    assert_eq!((back.rating, back.tags.len()), (1, 0));
    assert_ne!(back.source.as_deref(), Some(SOURCE_APP));

    // 4. A queued job whose target is renamed, then canceled: the stub moves along, then it is gone.
    let c = env.out.join("clip - 02.mkv");
    let d = env.out.join("clip - 03.mkv");
    send(&sync, 3, &c, &metadata, SyncPhase::Enqueued);
    until(&mut records, 3, |r| r.state == SyncState::Synced).await;
    send(&sync, 3, &d, &metadata, SyncPhase::Edited);
    until(&mut records, 3, |r| {
        r.state == SyncState::Synced && r.synced_path.as_deref() == Some(d.as_path())
    })
    .await;
    assert!(!client.get_media(&c).await.unwrap().found);
    assert!(client.get_media(&d).await.unwrap().found);
    cleanup(&sync, 3);
    until(&mut records, 3, |r| r.state == SyncState::CleanedUp).await;
    assert!(!client.get_media(&d).await.unwrap().found);

    // 5. A known source: the lookup the banner uses lists the stub of job 1's source only while it is pending; the
    // hash list keeps the source known.
    let (tx, rx) = tokio::sync::oneshot::channel();
    sync.handle
        .send(SyncCommand::CheckHash {
            md5: HASH.into(),
            reply: tx,
        })
        .unwrap();
    let known = rx.await.unwrap().unwrap();
    assert!(known.known);
    eprintln!("[live] check-hash lists {:?}", known.associated_files);

    sync.handle
        .request(|reply| SyncCommand::Shutdown { reply })
        .await
        .unwrap();
}
