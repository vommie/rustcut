//! The looks at the system behind the checks: a program's version line (started with a time limit), whether a folder
//! can be written (`access`, nothing is created), its free space (`statvfs`), the settings files' format versions and
//! the queue database's schema (opened read-only beside the store's connection).

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

use super::{Check, Facts, LOW_SPACE, NO_SPACE, NamedPath, Status};
use crate::settings::{Settings, UiState, VersionedFile};
use crate::store::SCHEMA_VERSION;

/// How long a program may take to print its version.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// The first line `program flag` prints (its version), or why it could not be run.
pub async fn program_version(program: &Path, flag: &str) -> Result<String, String> {
    first_line(program, &[flag]).await
}

/// The first line `program args` prints, or why it could not be run.
pub async fn first_line(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .output();
    match tokio::time::timeout(VERSION_TIMEOUT, output).await {
        Err(_elapsed) => Err("it did not answer within 5 s".into()),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Err("not found".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(out)) if !out.status.success() => Err(format!("it ended with {}", out.status)),
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
            // ffmpeg's line carries its whole copyright after the version.
            Ok(line.split(" Copyright").next().unwrap_or(line).to_owned())
        }
    }
}

/// Whether the folder exists and can be written (`None` = it does not exist).
pub fn writable(path: &Path) -> Option<bool> {
    if !path.is_dir() {
        return None;
    }
    Some(rustix::fs::access(path, rustix::fs::Access::WRITE_OK | rustix::fs::Access::EXEC_OK).is_ok())
}

/// The free space of the file system a folder lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeSpace {
    pub available: u64,
    pub total: u64,
}

/// The space a non-root user can use on `path`'s file system; `None` when it cannot be read.
pub fn free_space(path: &Path) -> Option<FreeSpace> {
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(FreeSpace {
        available: stat.f_bavail.saturating_mul(stat.f_frsize),
        total: stat.f_blocks.saturating_mul(stat.f_frsize),
    })
}

fn size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::DECIMAL)
}

/// config.toml and ui-state.toml: readable, and of a format this version writes.
pub(super) fn settings_files(facts: &Facts) -> Vec<Check> {
    let file = |id: &str, path: &Path, current: u32, writable: bool| -> Check {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let Ok(text) = std::fs::read_to_string(path) else {
            return if path.exists() {
                Check::new(id, Status::Problem, format!("{name} cannot be read")).detail(path.display().to_string())
            } else {
                Check::new(id, Status::Info, format!("{name} is not written yet"))
                    .detail("RustCut writes it with the first change.")
            };
        };
        let version = toml::from_str::<toml::Table>(&text)
            .ok()
            .and_then(|t| t.get("version").and_then(toml::Value::as_integer));
        match version.and_then(|v| u32::try_from(v).ok()) {
            Some(v) if v > current || !writable => {
                Check::new(id, Status::Problem, format!("{name} is from a newer RustCut")).detail(format!(
                    "Format {v}; this version writes {current}. Changes hold until RustCut quits and are not saved."
                ))
            }
            Some(v) => {
                Check::new(id, Status::Ok, format!("{name} is fine")).detail(format!("Format {v}, {}", path.display()))
            }
            None => Check::new(
                id,
                Status::Problem,
                format!("{name} is no settings file RustCut can read"),
            )
            .detail(path.display().to_string()),
        }
    };
    vec![
        file("config", &facts.config_file, Settings::VERSION, facts.config_writable),
        file(
            "ui-state",
            &facts.ui_state_file,
            UiState::VERSION,
            facts.ui_state_writable,
        ),
    ]
}

/// The queue database: its schema, a quick check of its pages, or that the queue runs in memory.
pub(super) async fn database(path: &Path, in_memory: bool) -> Check {
    if in_memory {
        return Check::new("database", Status::Problem, "The queue runs in memory").detail(format!(
            "{} could not be used: jobs are lost when RustCut quits.",
            path.display()
        ));
    }
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let check: String = conn.pragma_query_value(None, "quick_check", |r| r.get(0))?;
        Ok::<_, rusqlite::Error>((version, check))
    })
    .await;
    match result {
        Ok(Ok((version, check))) if check == "ok" => Check::new("database", Status::Ok, "The queue database is fine")
            .detail(format!("Schema {version} (this version: {SCHEMA_VERSION})")),
        Ok(Ok((_, check))) => Check::new("database", Status::Problem, "The queue database is damaged").detail(check),
        Ok(Err(e)) => {
            Check::new("database", Status::Warning, "The queue database cannot be read").detail(e.to_string())
        }
        Err(e) => Check::new("database", Status::Warning, "The queue database was not checked").detail(e.to_string()),
    }
}

/// The folders RustCut writes and the target folders: they exist and can be written.
pub(super) fn folders(own: &[NamedPath], targets: &[NamedPath]) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut ok = Vec::new();
    for folder in own {
        match writable(&folder.path) {
            Some(true) => ok.push(folder.name.clone()),
            Some(false) => checks.push(
                Check::new(
                    format!("folder-{}", folder.name),
                    Status::Problem,
                    format!("{} cannot be written", folder.name),
                )
                .detail(folder.path.display().to_string()),
            ),
            None => checks.push(
                Check::new(
                    format!("folder-{}", folder.name),
                    Status::Info,
                    format!("{} does not exist yet", folder.name),
                )
                .detail(format!("{} (made when it is needed)", folder.path.display())),
            ),
        }
    }
    if !ok.is_empty() {
        checks.insert(
            0,
            Check::new("folders", Status::Ok, "RustCut's folders can be written").detail(ok.join(", ")),
        );
    }
    for target in targets {
        let id = format!("target-{}", target.name);
        checks.push(match writable(&target.path) {
            Some(true) => Check::new(id, Status::Ok, format!("Target \"{}\" can be written", target.name))
                .detail(target.path.display().to_string()),
            Some(false) => Check::new(
                id,
                Status::Problem,
                format!("Target \"{}\" cannot be written", target.name),
            )
            .detail(target.path.display().to_string()),
            None => Check::new(
                id,
                Status::Warning,
                format!("Target \"{}\" does not exist", target.name),
            )
            .detail(format!(
                "{}: saving a job there makes it (Settings > Export & Naming).",
                target.path.display()
            )),
        });
    }
    checks
}

/// The free space where renders go (each file system once) and of the cache.
pub(super) fn spaces(targets: &[NamedPath], cache: &Path) -> Vec<Check> {
    let mut seen: Vec<FreeSpace> = Vec::new();
    let mut checks = Vec::new();
    let places = targets
        .iter()
        .map(|t| ("Space for renders".to_owned(), t.path.as_path()))
        .chain(std::iter::once(("Space for the cache".to_owned(), cache)));
    for (name, path) in places {
        // A folder that does not exist yet has the space of the nearest one that does.
        let existing = path.ancestors().find(|p| p.is_dir()).unwrap_or(path);
        let Some(space) = free_space(existing) else { continue };
        if seen.contains(&space) {
            continue;
        }
        seen.push(space);
        let status = if space.available < NO_SPACE {
            Status::Problem
        } else if space.available < LOW_SPACE {
            Status::Warning
        } else {
            Status::Ok
        };
        let id = format!("space-{}", seen.len());
        checks.push(
            Check::new(id, status, format!("{name}: {} free", size(space.available))).detail(format!(
                "of {} on the drive of {}",
                size(space.total),
                existing.display()
            )),
        );
    }
    checks
}
