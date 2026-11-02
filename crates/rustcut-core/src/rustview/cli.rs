//! The CLI transport of the [`RustViewClient`] (ADR-6): every call is one `rustview api <command> ... --json` child on
//! the tokio runtime with a time limit (`Settings.rustview.timeout_secs`); the child is killed when the call is dropped.
//!
//! Exit codes (RustView 0.3.86, `crates/rustview-core/src/cli_api/`): 0 = done, 2 = a normal "no" (`get-media` not
//! found, `delete-media` nothing deleted, `check-hash` unknown), both with the JSON answer on stdout; 1 = refused, with
//! `API Error: <reason>` on stderr and nothing on stdout. Paths go as `--path=<path>` so a name that starts with `-` is
//! never read as an option; RustView needs absolute paths (a relative one fails inside its database).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::de::DeserializeOwned;

use super::client::{Result, RustViewClient, RustViewError, SOURCE_APP};
use super::models::{BatchHashCheck, DeleteMedia, GetMedia, HashCheck, HashInsert, MoveMedia, SetMedia, TagTree};
use crate::settings::RustViewSettings;

/// The program started when `Settings.rustview.binary` is not set (found in `PATH`).
pub const DEFAULT_BINARY: &str = "rustview";
/// Longest error text taken from RustView's stderr.
const MAX_REASON: usize = 300;

/// Calls `rustview api` as a child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliClient {
    program: PathBuf,
    timeout_secs: u8,
    envs: Vec<(OsString, OsString)>,
}

impl CliClient {
    pub fn new(settings: &RustViewSettings) -> Self {
        Self {
            program: settings.binary.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_BINARY)),
            timeout_secs: settings.timeout_secs.max(1),
            envs: Vec::new(),
        }
    }

    /// An environment variable for the child (tests point RustView at a test library through `XDG_DATA_HOME`).
    #[must_use]
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Runs `rustview api <command> <args> --json` and decodes its answer.
    async fn call<T: DeserializeOwned>(&self, command: &str, args: Vec<OsString>) -> Result<T> {
        let mut child = tokio::process::Command::new(&self.program);
        child
            .arg("api")
            .arg(command)
            .args(args)
            .arg("--json")
            .envs(self.envs.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let started = std::time::Instant::now();
        let child = crate::process::spawn(&mut child)
            .await
            .map_err(|e| spawn_error(&self.program, &e))?;
        let limit = Duration::from_secs(u64::from(self.timeout_secs));
        let output = match tokio::time::timeout(limit, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(RustViewError::Unavailable(format!(
                    "RustView's answer could not be read: {e}"
                )));
            }
            // Dropping the future killed the child (`kill_on_drop`).
            Err(_) => return Err(RustViewError::Timeout(self.timeout_secs)),
        };
        let answer = decode(output.status.code(), &output.stdout, &output.stderr);
        tracing::debug!(
            command,
            code = ?output.status.code(),
            millis = started.elapsed().as_millis(),
            ok = answer.is_ok(),
            "[RustView] api call"
        );
        answer
    }
}

/// Why the program could not be started; always worth a retry later (it may be installed or fixed meanwhile).
fn spawn_error(program: &Path, error: &std::io::Error) -> RustViewError {
    RustViewError::Unavailable(match error.kind() {
        std::io::ErrorKind::NotFound => format!("{} is not installed (or not in PATH)", program.display()),
        std::io::ErrorKind::PermissionDenied => format!("{} may not be run (permission denied)", program.display()),
        _ => format!("{} could not be started: {error}", program.display()),
    })
}

/// Turns the exit code and the output of `rustview api ... --json` into the answer (pure; the contract tests feed it the
/// fixtures recorded from the real RustView).
pub fn decode<T: DeserializeOwned>(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<T> {
    let json = serde_json::from_slice::<serde_json::Value>(stdout).ok();
    if let Some(reason) = json.as_ref().and_then(reported_failure) {
        return Err(RustViewError::Api(reason));
    }
    match code {
        Some(0 | 2) => {
            let json = json.ok_or_else(|| {
                RustViewError::Decode(if stdout.iter().all(u8::is_ascii_whitespace) {
                    "RustView printed nothing".to_owned()
                } else {
                    "RustView did not print JSON".to_owned()
                })
            })?;
            serde_json::from_value(json).map_err(|e| RustViewError::Decode(e.to_string()))
        }
        // A launcher script (a wrapper, a container's `rustview`) whose program is missing or not executable ends
        // like a shell: 127 or 126. That is no answer of RustView, so it counts as "cannot be started".
        Some(code @ (126 | 127)) => {
            Err(RustViewError::Unavailable(stderr_reason(stderr).unwrap_or_else(|| {
                format!("the rustview launcher ended with exit code {code}")
            })))
        }
        Some(code) => Err(RustViewError::Api(
            stderr_reason(stderr).unwrap_or_else(|| format!("RustView ended with exit code {code}")),
        )),
        None => Err(RustViewError::Unavailable(
            "RustView was stopped by a signal".to_owned(),
        )),
    }
}

/// The reason of a `{"success": false, "error": ...}` answer.
fn reported_failure(json: &serde_json::Value) -> Option<String> {
    (json.get("success")? == &serde_json::Value::Bool(false)).then(|| {
        json.get("error")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| "RustView reported a failure".to_owned(), str::to_owned)
    })
}

/// The first line of stderr without RustView's `API Error: ` prefix.
fn stderr_reason(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("API Error:").map_or(line, str::trim_start);
    Some(line.chars().take(MAX_REASON).collect())
}

fn absolute(path: &Path) -> Result<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(RustViewError::Api(format!("not an absolute path: {}", path.display())))
    }
}

/// `--name=value` as one argument (a value that starts with `-` stays a value).
fn option(name: &str, value: impl AsRef<std::ffi::OsStr>) -> OsString {
    let mut arg = OsString::from(format!("--{name}="));
    arg.push(value);
    arg
}

fn tag_list(tags: &[u32]) -> String {
    tags.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// The arguments of `set-media`.
fn set_media_args(
    path: &Path,
    rating: Option<u8>,
    tags: Option<&[u32]>,
    hash: Option<&str>,
    source: Option<&str>,
) -> Vec<OsString> {
    let mut args = vec![option("path", path)];
    if let Some(rating) = rating {
        args.push(option("rating", rating.min(5).to_string()));
    }
    if let Some(tags) = tags {
        // An empty list clears the record's tags.
        args.push(option("tags", tag_list(tags)));
    }
    if let Some(hash) = hash {
        args.push(option("hash", hash));
    }
    if let Some(source) = source {
        args.push(option("source", source));
    }
    args
}

impl RustViewClient for CliClient {
    async fn tag_tree(&self) -> Result<TagTree> {
        self.call("tag-tree", Vec::new()).await
    }

    async fn get_media(&self, path: &Path) -> Result<GetMedia> {
        absolute(path)?;
        self.call("get-media", vec![option("path", path)]).await
    }

    async fn set_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> Result<SetMedia> {
        absolute(path)?;
        self.call("set-media", set_media_args(path, rating, tags, hash, None))
            .await
    }

    async fn set_own_media(
        &self,
        path: &Path,
        rating: Option<u8>,
        tags: Option<&[u32]>,
        hash: Option<&str>,
    ) -> Result<SetMedia> {
        absolute(path)?;
        self.call("set-media", set_media_args(path, rating, tags, hash, Some(SOURCE_APP)))
            .await
    }

    async fn delete_media(&self, path: &Path) -> Result<DeleteMedia> {
        absolute(path)?;
        self.call("delete-media", vec![option("path", path)]).await
    }

    async fn move_media(&self, from: &Path, to: &Path, replace: bool) -> Result<MoveMedia> {
        absolute(from)?;
        absolute(to)?;
        let mut args = vec![option("from", from), option("to", to)];
        if replace {
            args.push("--replace".into());
        }
        self.call("move-media", args).await.map_err(|e| match e {
            // RustView before 0.6.9 answers clap's "unrecognized subcommand" (exit code 2, no JSON).
            RustViewError::Decode(_) => RustViewError::Api(
                "this RustView version cannot move entries yet (RustView 0.6.9 or newer is needed)".into(),
            ),
            other => other,
        })
    }

    async fn check_hash(&self, md5: &str) -> Result<HashCheck> {
        self.call("check-hash", vec![option("hash", md5)]).await
    }

    async fn check_hashes(&self, md5s: &[String]) -> Result<BatchHashCheck> {
        self.call("check-hashes", vec![option("hashes", md5s.join(","))]).await
    }

    async fn insert_hash(&self, md5: &str) -> Result<HashInsert> {
        self.call("insert-hash", vec![option("hash", md5)]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_media_passes_only_what_is_given() {
        let args = set_media_args(Path::new("/v/-x.mkv"), Some(9), Some(&[3, 1]), None, Some(SOURCE_APP));
        assert_eq!(
            args,
            ["--path=/v/-x.mkv", "--rating=5", "--tags=3,1", "--source=RustCut"].map(OsString::from)
        );
        let clear = set_media_args(Path::new("/v/a.mkv"), None, Some(&[]), Some("ab"), None);
        assert_eq!(clear, ["--path=/v/a.mkv", "--tags=", "--hash=ab"].map(OsString::from));
    }

    #[test]
    fn exit_codes_and_failures_are_mapped() {
        let missing: GetMedia = decode(Some(2), br#"{"success":true,"found":false}"#, b"").unwrap();
        assert!(!missing.found);
        assert_eq!(
            decode::<HashInsert>(Some(1), b"", b"API Error: Invalid argument: Invalid MD5 hash format.\n"),
            Err(RustViewError::Api("Invalid argument: Invalid MD5 hash format.".into()))
        );
        assert_eq!(
            decode::<GetMedia>(Some(0), br#"{"success":false,"error":"locked"}"#, b""),
            Err(RustViewError::Api("locked".into()))
        );
        assert_eq!(
            decode::<GetMedia>(Some(3), b"", b""),
            Err(RustViewError::Api("RustView ended with exit code 3".into()))
        );
        assert!(matches!(
            decode::<GetMedia>(Some(0), b"  \n", b""),
            Err(RustViewError::Decode(_))
        ));
        assert!(matches!(
            decode::<TagTree>(Some(0), br#"{"success":true,"tags":"many"}"#, b""),
            Err(RustViewError::Decode(_))
        ));
        assert!(decode::<GetMedia>(None, b"", b"").unwrap_err().is_transient());
        let missing = decode::<TagTree>(
            Some(127),
            b"",
            b"rustview: /usr/bin/rustview: No such file or directory\n",
        );
        assert_eq!(
            missing,
            Err(RustViewError::Unavailable(
                "rustview: /usr/bin/rustview: No such file or directory".into()
            ))
        );
        assert!(missing.unwrap_err().is_transient());
    }

    #[tokio::test]
    async fn a_missing_program_or_a_relative_path_fails_before_any_call() {
        let client = CliClient::new(&RustViewSettings {
            binary: Some("/nonexistent/rustview".into()),
            ..RustViewSettings::default()
        });
        let error = client.tag_tree().await.unwrap_err();
        assert!(error.is_transient(), "{error}");
        assert!(error.to_string().contains("not installed"), "{error}");
        assert!(matches!(
            client.get_media(Path::new("out/a.mkv")).await,
            Err(RustViewError::Api(_))
        ));
    }
}
