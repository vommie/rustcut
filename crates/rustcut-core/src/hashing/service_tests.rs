use std::io::Write as _;

use tokio::sync::broadcast::Receiver;

use super::*;

/// MD5 of `bytes` by the reference implementation in the test (RFC 1321 test vector checked below).
fn reference_md5(bytes: &[u8]) -> String {
    hex::encode(Md5::digest(bytes))
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
    path
}

fn start() -> (HashService, JoinHandle<()>) {
    spawn_hash_service(&tokio::runtime::Handle::current())
}

/// The next event that is not progress.
async fn outcome(events: &mut Receiver<HashEvent>) -> HashEvent {
    loop {
        match events.recv().await.unwrap() {
            HashEvent::Progress { .. } => {}
            other => return other,
        }
    }
}

#[test]
fn md5_matches_the_rfc_test_vectors() {
    assert_eq!(reference_md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(reference_md5(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    let cancel = AtomicBool::new(false);
    let data = vec![7u8; CHUNK * 2 + 5];
    let mut calls = Vec::new();
    let md5 = md5_of(data.as_slice(), 0, &cancel, &mut |done, total| {
        calls.push((done, total));
    })
    .ok();
    assert_eq!(md5, Some(reference_md5(&data)));
    let len = u64::try_from(data.len()).unwrap();
    assert_eq!(calls.first(), Some(&(0, 0)));
    assert_eq!(
        calls.last(),
        Some(&(len, len)),
        "the total follows a file that is longer than announced"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sidecar_answers_without_reading_the_video() {
    let dir = tempfile::tempdir().unwrap();
    let video = write_file(dir.path(), "clip.mp4", b"not the content the sidecar describes");
    write_file(
        dir.path(),
        ".clip.mp4.md5",
        b"8F618D1EBD606025257057EE18536E64  clip.mp4\n",
    );
    let (service, task) = start();
    let mut events = service.events.subscribe();
    let md5 = service
        .handle
        .request(|reply| HashCommand::Hash {
            path: video.clone(),
            reply: Some(reply),
        })
        .await
        .unwrap();
    assert_eq!(md5, Ok("8f618d1ebd606025257057ee18536e64".to_owned()));
    assert_eq!(
        events.recv().await.unwrap(),
        HashEvent::Finished {
            path: video,
            md5: "8f618d1ebd606025257057ee18536e64".into(),
            from_sidecar: true
        },
        "no progress for a sidecar"
    );
    service
        .handle
        .request(|reply| HashCommand::Shutdown { reply })
        .await
        .unwrap();
    task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_is_hashed_with_progress_and_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| u8::try_from(i % 251).unwrap()).collect();
    let video = write_file(dir.path(), "big.mkv", &data);
    // A sidecar with garbage is ignored.
    write_file(dir.path(), ".big.mkv.md5", b"not a hash\n");
    let (service, task) = start();
    let mut events = service.events.subscribe();
    let ask = || {
        service.handle.request(|reply| HashCommand::Hash {
            path: video.clone(),
            reply: Some(reply),
        })
    };
    assert_eq!(ask().await.unwrap(), Ok(reference_md5(&data)));
    let len = u64::try_from(data.len()).unwrap();
    let mut progress = Vec::new();
    let finished = loop {
        match events.recv().await.unwrap() {
            HashEvent::Progress { done, total, .. } => progress.push((done, total)),
            other => break other,
        }
    };
    assert_eq!(progress.first(), Some(&(0, len)));
    assert_eq!(progress.last(), Some(&(len, len)));
    assert!(matches!(
        finished,
        HashEvent::Finished {
            from_sidecar: false,
            ..
        }
    ));
    // Again: answered from memory, no progress.
    assert_eq!(ask().await.unwrap(), Ok(reference_md5(&data)));
    assert!(matches!(events.recv().await.unwrap(), HashEvent::Finished { .. }));
    // A changed file is hashed again.
    write_file(dir.path(), "big.mkv", b"short now");
    assert_eq!(ask().await.unwrap(), Ok(reference_md5(b"short now")));
    service
        .handle
        .request(|reply| HashCommand::Shutdown { reply })
        .await
        .unwrap();
    task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_files_fail_and_waiting_files_can_be_canceled() {
    let dir = tempfile::tempdir().unwrap();
    let (service, task) = start();
    let mut events = service.events.subscribe();
    let missing = dir.path().join("gone.mp4");
    let answer = service
        .handle
        .request(|reply| HashCommand::Hash {
            path: missing.clone(),
            reply: Some(reply),
        })
        .await
        .unwrap();
    assert!(matches!(answer, Err(HashError::Io { .. })), "{answer:?}");
    assert!(matches!(outcome(&mut events).await, HashEvent::Failed { path, .. } if path == missing));

    // A FIFO blocks the first read until a writer comes, so the second file is still waiting when it is canceled.
    let fifo = dir.path().join("pipe.mp4");
    let status = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success());
    let second = write_file(dir.path(), "second.mp4", b"x");
    service
        .handle
        .send(HashCommand::Hash {
            path: fifo.clone(),
            reply: None,
        })
        .unwrap();
    let (reply, waiting) = tokio::sync::oneshot::channel();
    service
        .handle
        .send(HashCommand::Hash {
            path: second.clone(),
            reply: Some(reply),
        })
        .unwrap();
    service
        .handle
        .send(HashCommand::Cancel { path: second.clone() })
        .unwrap();
    assert_eq!(waiting.await.unwrap(), Err(HashError::Canceled));
    assert_eq!(outcome(&mut events).await, HashEvent::Canceled { path: second });
    // Unblock the FIFO and cancel it while it is read.
    let writer_path = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut w = std::fs::OpenOptions::new().write(true).open(writer_path).unwrap();
        // Keep the pipe open until the reader stops.
        for _ in 0..200 {
            if w.write_all(&[0u8; 4096]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    service.handle.send(HashCommand::Cancel { path: fifo.clone() }).unwrap();
    assert_eq!(outcome(&mut events).await, HashEvent::Canceled { path: fifo });
    service
        .handle
        .request(|reply| HashCommand::Shutdown { reply })
        .await
        .unwrap();
    task.await.unwrap();
    writer.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_for_the_same_file_share_one_read_and_shutdown_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let video = write_file(dir.path(), "a.mp4", b"same file");
    let (service, task) = start();
    let mut events = service.events.subscribe();
    let mut answers = Vec::new();
    for _ in 0..3 {
        let (reply, answer) = tokio::sync::oneshot::channel();
        service
            .handle
            .send(HashCommand::Hash {
                path: video.clone(),
                reply: Some(reply),
            })
            .unwrap();
        answers.push(answer);
    }
    for answer in answers {
        assert_eq!(answer.await.unwrap(), Ok(reference_md5(b"same file")));
    }
    assert!(matches!(outcome(&mut events).await, HashEvent::Finished { .. }));
    // Only one Finished for three requests.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(events.try_recv().is_err());

    // A request still waiting at shutdown is answered with Canceled.
    let fifo = dir.path().join("blocked.mp4");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    service
        .handle
        .send(HashCommand::Hash {
            path: fifo.clone(),
            reply: None,
        })
        .unwrap();
    let (reply, waiting) = tokio::sync::oneshot::channel();
    service
        .handle
        .send(HashCommand::Hash {
            path: video.with_file_name("other.mp4"),
            reply: Some(reply),
        })
        .unwrap();
    let (reply, stopped) = tokio::sync::oneshot::channel();
    service.handle.send(HashCommand::Shutdown { reply }).unwrap();
    // Waiting requests are answered before the running read is stopped...
    assert_eq!(waiting.await.unwrap(), Err(HashError::Canceled));
    // ...which ends once the FIFO gets a writer (opened and closed at once: end of file).
    let opener = std::thread::spawn(move || std::fs::OpenOptions::new().write(true).open(fifo).map(drop));
    stopped.await.unwrap();
    task.await.unwrap();
    opener.join().unwrap().unwrap();
    assert!(service.handle.is_closed());
}
