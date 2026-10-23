//! One running RustCut per profile: a local socket in `$XDG_RUNTIME_DIR` (only the user can reach it) carrying one JSON line
//! per request and per reply. A second start of the same profile forwards its files to the running window and exits.
//!
//! The socket name contains a stable hash of the profile's config folder, so a `--home` test profile and the user's real
//! profile never reach each other.

use std::hash::Hasher as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use interprocess::local_socket::tokio::Stream as AsyncStream;
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

/// How long a second start waits for the running instance's answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the running instance waits for a request line after a connection.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// Longest request line accepted (file paths of a big selection).
const MAX_REQUEST: u64 = 1 << 20;

/// What a second start asks the running instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Answers without doing anything (checking whether an instance is alive).
    Ping,
    /// Brings the window to the front and opens the first of `files` (absolute paths); no files = only show the window.
    Open { files: Vec<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
}

impl Reply {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

/// The socket of the profile whose settings live in `config_dir`.
pub fn socket_path(config_dir: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    let mut hasher = fnv::FnvHasher::default();
    hasher.write(config_dir.as_os_str().as_bytes());
    let folder = directories::BaseDirs::new()
        .and_then(|d| d.runtime_dir().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    folder.join(format!("rustcut-{:016x}.sock", hasher.finish()))
}

fn name(path: &Path) -> std::io::Result<Name<'static>> {
    path.to_path_buf().to_fs_name::<GenericFilePath>()
}

/// Sends `request` to the running instance. `Ok(None)` means nobody is listening.
pub fn send(path: &Path, request: &Request) -> std::io::Result<Option<Reply>> {
    let stream = match Stream::connect(name(path)?) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    stream.set_recv_timeout(Some(REPLY_TIMEOUT))?;
    let mut reader = BufReader::new(stream);
    let mut line = serde_json::to_string(request).map_err(std::io::Error::other)?;
    line.push('\n');
    reader.get_mut().write_all(line.as_bytes())?;
    reader.get_mut().flush()?;
    let mut answer = String::new();
    reader.read_line(&mut answer)?;
    serde_json::from_str(answer.trim())
        .map(Some)
        .map_err(std::io::Error::other)
}

/// Owns the socket while the app runs; dropping it removes the socket file.
pub struct Server {
    listener_task: tokio::task::JoinHandle<()>,
}

/// Starts listening on the runtime and hands every request to `on_request` (on a runtime thread: forward it to the UI).
/// Fails with `AddrInUse` when another instance owns the socket; a socket left behind by a crash is taken over.
pub fn serve(
    path: &Path,
    runtime: &tokio::runtime::Handle,
    on_request: impl Fn(Request) + Send + Sync + 'static,
) -> std::io::Result<Server> {
    let _entered = runtime.enter();
    let listener = match ListenerOptions::new().name(name(path)?).create_tokio() {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if send(path, &Request::Ping)?.is_some() {
                return Err(e);
            }
            tracing::info!(socket = %path.display(), "[Instance] Taking over a socket left behind");
            std::fs::remove_file(path)?;
            ListenerOptions::new().name(name(path)?).create_tokio()?
        }
        Err(e) => return Err(e),
    };
    let on_request = std::sync::Arc::new(on_request);
    let task = runtime.spawn(async move {
        loop {
            match listener.accept().await {
                Ok(conn) => {
                    let on_request = std::sync::Arc::clone(&on_request);
                    tokio::spawn(async move { handle(conn, &*on_request).await });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "[Instance] Accepting a connection failed");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    });
    tracing::info!(socket = %path.display(), "[Instance] Listening for a second start");
    Ok(Server { listener_task: task })
}

impl Drop for Server {
    fn drop(&mut self) {
        // The listener lives in the task; aborting it drops the listener, which removes the socket file.
        self.listener_task.abort();
    }
}

async fn handle(conn: AsyncStream, on_request: &(dyn Fn(Request) + Send + Sync)) {
    let (reader, mut writer) = conn.split();
    let mut reader = tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(reader, MAX_REQUEST));
    let mut line = String::new();
    let reply = match tokio::time::timeout(REQUEST_TIMEOUT, reader.read_line(&mut line)).await {
        Ok(Ok(_)) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(Request::Ping) => Reply::ok("pong"),
            Ok(request) => {
                tracing::info!(request = ?request, "[Instance] Request from a second start");
                on_request(request);
                Reply::ok("forwarded")
            }
            Err(e) => Reply::err(format!("bad request: {e}")),
        },
        Ok(Err(e)) => Reply::err(format!("read failed: {e}")),
        Err(_elapsed) => Reply::err("no request in time"),
    };
    if let Ok(mut text) = serde_json::to_string(&reply) {
        text.push('\n');
        // The other side may have given up already; nothing to do then.
        writer.write_all(text.as_bytes()).await.ok();
    }
}

/// The files of a second start as absolute paths (its working folder may differ from the running instance's).
pub fn absolute_files(files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|f| std::path::absolute(f).unwrap_or_else(|_| f.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn profiles_get_their_own_socket() {
        let a = socket_path(Path::new("/home/u/.config/rustcut"));
        let b = socket_path(Path::new("/tmp/test-profile/config"));
        assert_ne!(a, b);
        assert_eq!(a, socket_path(Path::new("/home/u/.config/rustcut")));
        // FNV-1a is a fixed algorithm: the name stays the same across versions and builds.
        assert_eq!(
            a.file_name().and_then(|n| n.to_str()),
            Some("rustcut-8dc1797fc8be052b.sock")
        );
    }

    #[test]
    fn a_second_start_forwards_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("i.sock");
        let rt = runtime();
        let (tx, rx) = mpsc::channel();
        let server = serve(&sock, rt.handle(), move |r| tx.send(r).unwrap()).unwrap();
        assert_eq!(send(&sock, &Request::Ping).unwrap().unwrap(), Reply::ok("pong"));
        let files = vec![PathBuf::from("/v/a.mkv")];
        let reply = send(&sock, &Request::Open { files: files.clone() }).unwrap().unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Request::Open { files }
        );
        // A second server for the same profile is refused while the first one lives.
        let second = serve(&sock, rt.handle(), |_| {});
        assert_eq!(second.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
        drop(server);
    }

    #[test]
    fn nobody_listening_is_not_an_error_and_a_stale_socket_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(send(&dir.path().join("none.sock"), &Request::Ping).unwrap(), None);
        let stale = dir.path().join("stale.sock");
        {
            let l = ListenerOptions::new()
                .name(name(&stale).unwrap())
                .reclaim_name(false)
                .create_sync()
                .unwrap();
            drop(l);
        }
        assert!(stale.exists());
        let rt = runtime();
        assert!(serve(&stale, rt.handle(), |_| {}).is_ok());
    }

    #[test]
    fn requests_have_a_stable_wire_format() {
        let r = Request::Open {
            files: vec![PathBuf::from("/v/a b.mkv")],
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"cmd":"open","files":["/v/a b.mkv"]}"#
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"ping"}"#).unwrap(),
            Request::Ping
        );
    }

    #[test]
    fn relative_files_become_absolute() {
        let files = absolute_files(&[PathBuf::from("a.mkv"), PathBuf::from("/b.mkv")]);
        assert!(files.iter().all(|f| f.is_absolute()));
        assert_eq!(files[1], PathBuf::from("/b.mkv"));
    }
}
