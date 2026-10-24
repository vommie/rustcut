//! Starting the external programs (ffmpeg, ffprobe, rustview) on the runtime.

use std::time::Duration;

/// Waits between the tries while the program is busy (about 0.3 s in all).
const BUSY_WAITS: [Duration; 4] = [
    Duration::from_millis(20),
    Duration::from_millis(40),
    Duration::from_millis(80),
    Duration::from_millis(160),
];

/// Spawns `command`, trying again for a moment while the system reports the program as busy ("Text file busy": the
/// file is still open for writing somewhere, e.g. while a package update replaces it, or while a parallel test's child
/// inherited the descriptor of a stand-in script; rust-lang/rust#114554). Every other error returns at once.
pub async fn spawn(command: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
    for wait in BUSY_WAITS {
        match command.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                tracing::debug!(error = %e, "[Process] The program is busy; trying again");
                tokio::time::sleep(wait).await;
            }
            result => return result,
        }
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_program_starts_and_a_missing_one_fails_at_once() {
        let mut ok = tokio::process::Command::new("true");
        assert!(spawn(&mut ok).await.unwrap().wait().await.unwrap().success());
        let started = std::time::Instant::now();
        let mut missing = tokio::process::Command::new("/nonexistent/rustcut-test-program");
        let error = spawn(&mut missing).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            started.elapsed() < Duration::from_millis(20),
            "no waiting for other errors"
        );
    }

    #[tokio::test]
    async fn a_script_still_open_for_writing_starts_once_it_is_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stand-in");
        let mut file = std::fs::File::create(&path).unwrap();
        std::io::Write::write_all(&mut file, b"#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        file.set_permissions(permissions).unwrap();
        // The descriptor stays open for 50 ms, like one inherited by another thread's child.
        let closer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(file);
        });
        let mut command = tokio::process::Command::new(&path);
        let status = spawn(&mut command).await.unwrap().wait().await.unwrap();
        assert!(status.success());
        closer.join().unwrap();
    }
}
