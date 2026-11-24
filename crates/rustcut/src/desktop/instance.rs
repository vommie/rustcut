//! One running RustCut per profile: a local socket in `$XDG_RUNTIME_DIR` (only the user can reach it) carrying one JSON line
//! per request and per reply. A second start of the same profile forwards its files to the running window and exits.
//!
//! The socket name contains a stable hash of the profile's config folder, so a `--home` test profile and the user's real
//! profile never reach each other.
//!
//! Protocol 2 (0.0.120) adds two requests for RustSearch, which stays open next to RustCut: `status` answers with RustCut's
//! state (`Reply.status`), and `app_open` opens a file on behalf of another program, which may ask first when the open video
//! has unsaved sections (`Reply.outcome`). `ping` and `open` (a second start) are unchanged; an older RustCut answers the new
//! requests with `ok: false` ("bad request").

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
/// How long a request waits for the UI's answer (below `REPLY_TIMEOUT`, so a waiting client still gets a reply).
const UI_TIMEOUT: Duration = Duration::from_secs(2);
/// The protocol this RustCut speaks: 1 = `ping` and `open`; 2 = also `status` and `app_open`.
pub const PROTOCOL: u32 = 2;
/// The programs `app_open` accepts (their requests may ask first; Settings > Dialogs).
pub const ORIGINS: &[&str] = &["rustsearch"];

/// What a second start asks the running instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Answers without doing anything (checking whether an instance is alive).
    Ping,
    /// Brings the window to the front and opens the first of `files` (absolute paths); no files = only show the window.
    Open { files: Vec<PathBuf> },
    /// RustCut's state for another program (protocol 2): the answer carries `Reply.status`.
    Status,
    /// Opens the first of `files` (absolute paths) for the program `origin` (one of [`ORIGINS`], protocol 2). It opens at
    /// once like File > Open, unless the open video has unsaved sections and Settings > Dialogs says to ask: then RustCut
    /// asks and comes to the front. The answer's `outcome` says which.
    AppOpen { origin: String, files: Vec<PathBuf> },
}

/// What RustCut tells another program about itself (`status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// [`PROTOCOL`].
    pub protocol: u32,
    /// RustCut's version ("0.0.120").
    pub version: String,
    /// The open video (also when it failed to play).
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// The open video has sections that are not saved as a job.
    #[serde(default)]
    pub unsaved: bool,
    /// A dialog of RustCut's own is open or a job is being saved: an `app_open` now is refused.
    #[serde(default)]
    pub busy: bool,
    /// RustCut asks about an `app_open` (its question is open).
    #[serde(default)]
    pub asking: bool,
}

/// What became of an `app_open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The file is opening.
    Opened,
    /// RustCut asks first (unsaved sections); the user's answer decides.
    Asking,
    /// RustCut is busy with a dialog of its own; nothing happened.
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    /// The answer to `status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// The answer to `app_open`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

impl Reply {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            status: None,
            outcome: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            status: None,
            outcome: None,
        }
    }

    /// The answer to `status`.
    pub fn status(status: Status) -> Self {
        Self {
            status: Some(status),
            ..Self::ok("status")
        }
    }

    /// The answer to `app_open` (`Busy` is `ok: false`).
    pub fn outcome(outcome: Outcome, message: impl Into<String>) -> Self {
        let reply = if outcome == Outcome::Busy {
            Self::err(message)
        } else {
            Self::ok(message)
        };
        Self {
            outcome: Some(outcome),
            ..reply
        }
    }
}

/// Answers one request (from the UI thread); dropping it unanswered replies "not handled".
#[derive(Debug)]
pub struct Responder(tokio::sync::oneshot::Sender<Reply>);

impl Responder {
    pub fn reply(self, reply: Reply) {
        // The connection may be gone (the client gave up); nothing to do then.
        self.0.send(reply).ok();
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

/// Starts listening on the runtime and hands every request but `ping` to `on_request` (on a runtime thread: forward it
/// to the UI), whose [`Responder`] answers it within [`UI_TIMEOUT`]. Fails with `AddrInUse` when another instance owns
/// the socket; a socket left behind by a crash is taken over.
pub fn serve(
    path: &Path,
    runtime: &tokio::runtime::Handle,
    on_request: impl Fn(Request, Responder) + Send + Sync + 'static,
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

async fn handle(conn: AsyncStream, on_request: &(dyn Fn(Request, Responder) + Send + Sync)) {
    let (reader, mut writer) = tokio::io::split(conn);
    let mut reader = tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(reader, MAX_REQUEST));
    let mut line = String::new();
    let reply = match tokio::time::timeout(REQUEST_TIMEOUT, reader.read_line(&mut line)).await {
        Ok(Ok(_)) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(Request::Ping) => Reply::ok("pong"),
            Ok(request) => {
                if request == Request::Status {
                    // RustSearch asks every 2 s while its window shows: not worth a log line each time.
                    tracing::trace!("[Instance] Status request");
                } else {
                    tracing::info!(request = ?request, "[Instance] Request");
                }
                let (sender, answer) = tokio::sync::oneshot::channel();
                on_request(request, Responder(sender));
                match tokio::time::timeout(UI_TIMEOUT, answer).await {
                    Ok(Ok(reply)) => reply,
                    Ok(Err(_dropped)) => Reply::err("the request was not handled"),
                    Err(_elapsed) => Reply::err("RustCut did not answer in time"),
                }
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
        let server = serve(&sock, rt.handle(), move |r, answer: Responder| {
            answer.reply(Reply::ok("forwarded"));
            tx.send(r).unwrap();
        })
        .unwrap();
        assert_eq!(send(&sock, &Request::Ping).unwrap().unwrap(), Reply::ok("pong"));
        let files = vec![PathBuf::from("/v/a.mkv")];
        let reply = send(&sock, &Request::Open { files: files.clone() }).unwrap().unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Request::Open { files }
        );
        // A second server for the same profile is refused while the first one lives.
        let second = serve(&sock, rt.handle(), |_, _| {});
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
        assert!(serve(&stale, rt.handle(), |_, _| {}).is_ok());
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
    fn status_and_app_open_have_a_stable_wire_format() {
        assert_eq!(serde_json::to_string(&Request::Status).unwrap(), r#"{"cmd":"status"}"#);
        let r = Request::AppOpen {
            origin: "rustsearch".into(),
            files: vec![PathBuf::from("/v/a.mkv")],
        };
        let text = r#"{"cmd":"app_open","origin":"rustsearch","files":["/v/a.mkv"]}"#;
        assert_eq!(serde_json::to_string(&r).unwrap(), text);
        assert_eq!(serde_json::from_str::<Request>(text).unwrap(), r);
        let status = Status {
            protocol: PROTOCOL,
            version: "0.0.120".into(),
            file: Some(PathBuf::from("/v/a.mkv")),
            unsaved: true,
            busy: false,
            asking: false,
        };
        assert_eq!(
            serde_json::to_string(&Reply::status(status)).unwrap(),
            r#"{"ok":true,"message":"status","status":{"protocol":2,"version":"0.0.120","file":"/v/a.mkv","unsaved":true,"busy":false,"asking":false}}"#
        );
        assert_eq!(
            serde_json::to_string(&Reply::outcome(Outcome::Asking, "asking")).unwrap(),
            r#"{"ok":true,"message":"asking","outcome":"asking"}"#
        );
        assert!(!Reply::outcome(Outcome::Busy, "busy").ok);
        // Older replies (protocol 1) still read: the new fields are optional.
        let old: Reply = serde_json::from_str(r#"{"ok":true,"message":"pong"}"#).unwrap();
        assert_eq!(old, Reply::ok("pong"));
        // A status from a later RustCut with more fields still reads.
        let later: Reply = serde_json::from_str(
            r#"{"ok":true,"message":"status","status":{"protocol":3,"version":"1.0.0","extra":1},"more":true}"#,
        )
        .unwrap();
        assert_eq!(later.status.map(|s| (s.protocol, s.unsaved)), Some((3, false)));
    }

    #[test]
    fn the_ui_answers_status_and_app_open_and_silence_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("s.sock");
        let rt = runtime();
        let _server = serve(&sock, rt.handle(), |r, answer: Responder| match r {
            Request::Status => answer.reply(Reply::status(Status {
                protocol: PROTOCOL,
                version: "x".into(),
                file: None,
                unsaved: false,
                busy: false,
                asking: false,
            })),
            Request::AppOpen { .. } => answer.reply(Reply::outcome(Outcome::Opened, "opened")),
            // Not answered: the client hears "not handled" at once.
            _ => drop(answer),
        })
        .unwrap();
        let status = send(&sock, &Request::Status).unwrap().unwrap();
        assert_eq!(status.status.map(|s| s.protocol), Some(PROTOCOL));
        let open = Request::AppOpen {
            origin: "rustsearch".into(),
            files: vec![PathBuf::from("/v/a.mkv")],
        };
        assert_eq!(send(&sock, &open).unwrap().unwrap().outcome, Some(Outcome::Opened));
        let dropped = send(&sock, &Request::Open { files: vec![] }).unwrap().unwrap();
        assert!(!dropped.ok);
        assert_eq!(dropped.message, "the request was not handled");
    }

    #[test]
    fn relative_files_become_absolute() {
        let files = absolute_files(&[PathBuf::from("a.mkv"), PathBuf::from("/b.mkv")]);
        assert!(files.iter().all(|f| f.is_absolute()));
        assert_eq!(files[1], PathBuf::from("/b.mkv"));
    }
}
