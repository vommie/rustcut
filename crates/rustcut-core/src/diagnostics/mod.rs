//! Settings > Diagnostics (P9.16, the review's note 28#1; modeled on RustView's Debug page): what RustCut needs and how
//! it is doing, as checks in two groups. Dependencies: ffmpeg, ffprobe, mpv, vid.stab and the encoders of the capability
//! check, RustView's program. Health: the settings files, the queue database, the folders RustCut writes, the free
//! space of the targets and the cache, RustView's connection and library. The app gathers what it knows ([`Facts`]),
//! [`run`] adds what only a look at the system tells (on the tokio runtime), and the page shows the problems first.
//! Each check's `id` is its jump mark (`settings:diagnostics.<id>`), so messages and panels can lead to it.

mod probes;

use std::path::PathBuf;
use std::sync::Arc;

use crate::render::{Availability, Capabilities, EncoderBackend};
use crate::rustview::ConnectionState;

pub use probes::{FreeSpace, first_line, free_space, program_version, writable};

/// How a check came out (the order is the severity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Ok,
    Info,
    Warning,
    Problem,
}

impl Status {
    /// The word of the copied report.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Problem => "problem",
        }
    }
}

/// One check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Stable id: the jump mark (`settings:diagnostics.<id>`).
    pub id: String,
    pub status: Status,
    pub title: String,
    pub detail: String,
    /// Commands that fix it (shown with a copy button).
    pub fix: Vec<String>,
}

impl Check {
    pub fn new(id: impl Into<String>, status: Status, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status,
            title: title.into(),
            detail: String::new(),
            fix: Vec::new(),
        }
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    #[must_use]
    pub fn fix(mut self, command: impl Into<String>) -> Self {
        self.fix.push(command.into());
        self
    }
}

/// The checks of both groups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub dependencies: Vec<Check>,
    pub health: Vec<Check>,
}

/// A folder RustCut writes or a target it renders into, with the name the page uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPath {
    pub name: String,
    pub path: PathBuf,
}

impl NamedPath {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }
}

/// What the app knows when the checks start (gathered on the UI thread).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The programs as configured (`None` = found in `PATH`).
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub rustview: Option<PathBuf>,
    pub rustview_enabled: bool,
    /// The capability check's result (`None` while it runs).
    pub capabilities: Option<Arc<Capabilities>>,
    pub connection: Option<ConnectionState>,
    /// The settings files and whether RustCut writes them (`false` = a newer or unreadable file).
    pub config_file: PathBuf,
    pub config_writable: bool,
    pub ui_state_file: PathBuf,
    pub ui_state_writable: bool,
    pub database: PathBuf,
    /// The queue runs in memory (the database could not be used).
    pub database_in_memory: bool,
    /// The folders RustCut writes (config, data, logs, cache).
    pub folders: Vec<NamedPath>,
    /// The target folders of Settings > Export & Naming (their space and whether they can be written).
    pub targets: Vec<NamedPath>,
    pub cache: PathBuf,
    /// RustView's library folder (`$XDG_DATA_HOME/rustview`).
    pub rustview_library: PathBuf,
}

/// The space below which a folder is a warning (5 GB) or a problem (1 GB).
pub const LOW_SPACE: u64 = 5_000_000_000;
pub const NO_SPACE: u64 = 1_000_000_000;

/// Runs every check. Must run on the tokio runtime (programs are started with a time limit).
pub async fn run(facts: Facts) -> Report {
    let mut dependencies = Vec::new();
    dependencies.push(tool("ffmpeg", "ffmpeg", facts.ffmpeg.as_deref(), "-version").await);
    dependencies.push(tool("ffprobe", "ffprobe", facts.ffprobe.as_deref(), "-version").await);
    dependencies.push(mpv().await);
    dependencies.extend(capabilities(facts.capabilities.as_deref()));
    dependencies.push(rustview_program(&facts).await);
    let mut health = probes::settings_files(&facts);
    health.push(probes::database(&facts.database, facts.database_in_memory).await);
    health.extend(probes::folders(&facts.folders, &facts.targets));
    health.extend(probes::spaces(&facts.targets, &facts.cache));
    health.push(rustview_connection(&facts));
    Report { dependencies, health }
}

/// A program of ffmpeg's family: its version, or how to install it.
async fn tool(id: &str, name: &str, configured: Option<&std::path::Path>, flag: &str) -> Check {
    let program = configured.map_or_else(|| PathBuf::from(name), std::path::Path::to_path_buf);
    match program_version(&program, flag).await {
        Ok(version) => Check::new(id, Status::Ok, version).detail(program.display().to_string()),
        Err(reason) => Check::new(id, Status::Problem, format!("{name} cannot be run"))
            .detail(format!(
                "{}: {reason}. Renders and the media checks need it.",
                program.display()
            ))
            .fix("sudo pacman -S ffmpeg"),
    }
}

async fn mpv() -> Check {
    match program_version(std::path::Path::new("mpv"), "--version").await {
        Ok(version) => Check::new("mpv", Status::Ok, version).detail("The player is libmpv, mpv's library."),
        Err(_) => Check::new("mpv", Status::Info, "The mpv program is not installed")
            .detail("RustCut plays with libmpv; mpv itself only tells its version here.")
            .fix("sudo pacman -S mpv"),
    }
}

/// vid.stab and the encoders the capability check tried with one frame each.
fn capabilities(caps: Option<&Capabilities>) -> Vec<Check> {
    let Some(caps) = caps else {
        return vec![
            Check::new("encoders", Status::Info, "The encoder check is still running")
                .detail("RustCut tries every encoder with one frame at its start."),
        ];
    };
    let vidstab = if caps.vidstab {
        Check::new("vidstab", Status::Ok, "vid.stab is built into ffmpeg").detail("Deshake works.")
    } else {
        Check::new("vidstab", Status::Warning, "ffmpeg has no vid.stab")
            .detail("Deshake cannot render; the other filters work.")
            .fix("sudo pacman -S vid.stab ffmpeg")
    };
    let works = |s: &&crate::render::EncoderStatus| s.availability == Availability::Works;
    let software: Vec<&str> = caps
        .encoders
        .iter()
        .filter(|s| s.encoder.spec().backend == EncoderBackend::Software)
        .filter(works)
        .map(|s| s.encoder.spec().ffmpeg_name)
        .collect();
    let hardware: Vec<&str> = caps
        .encoders
        .iter()
        .filter(|s| s.encoder.spec().backend != EncoderBackend::Software)
        .filter(works)
        .map(|s| s.encoder.spec().ffmpeg_name)
        .collect();
    let encoders = if software.is_empty() {
        Check::new("encoders", Status::Problem, "No software encoder works")
            .detail("ffmpeg could not encode a frame with libsvtav1, libx265 or libx264.")
            .fix("sudo pacman -S ffmpeg")
    } else {
        Check::new(
            "encoders",
            Status::Ok,
            format!("{} software encoders work", software.len()),
        )
        .detail(software.join(", "))
    };
    let hw = if hardware.is_empty() {
        Check::new("hardware", Status::Info, "No hardware encoder works here")
            .detail("VAAPI, NVENC and QSV need a matching graphics card and driver; the software encoders are used.")
    } else {
        Check::new(
            "hardware",
            Status::Ok,
            format!("{} hardware encoders work", hardware.len()),
        )
        .detail(hardware.join(", "))
    };
    vec![vidstab, encoders, hw]
}

async fn rustview_program(facts: &Facts) -> Check {
    if !facts.rustview_enabled {
        return Check::new("rustview-program", Status::Info, "The RustView link is switched off")
            .detail("Jobs are saved without tags and rating (Settings > RustView).");
    }
    let program = facts.rustview.clone().unwrap_or_else(|| PathBuf::from("rustview"));
    let version = program_version(&program, "--version").await;
    // A wrapper that only forwards `api` (RustCut's test environment) still shows that RustView answers.
    let answer = match version {
        Ok(version) => Ok(version),
        Err(reason) => first_line(&program, &["api", "--help"])
            .await
            .map(|_| "RustView answers (its version is not known)".to_owned())
            .map_err(|_fallback| reason),
    };
    match answer {
        Ok(text) => Check::new("rustview-program", Status::Ok, text).detail(program.display().to_string()),
        Err(reason) => Check::new("rustview-program", Status::Problem, "RustView cannot be run").detail(format!(
            "{}: {reason}. Set its path in Settings > RustView or install RustView.",
            program.display()
        )),
    }
}

fn rustview_connection(facts: &Facts) -> Check {
    let library = facts.rustview_library.display().to_string();
    if !facts.rustview_enabled {
        return Check::new("rustview", Status::Info, "RustView is not used").detail("The link is switched off.");
    }
    match &facts.connection {
        Some(ConnectionState::Connected) => {
            let library_known = facts.rustview_library.is_dir();
            Check::new(
                "rustview",
                if library_known { Status::Ok } else { Status::Info },
                "RustView is connected",
            )
            .detail(if library_known {
                format!("Library: {library}")
            } else {
                format!("No RustView library at {library}: RustView uses another folder or has none yet")
            })
        }
        Some(ConnectionState::Unavailable { reason, .. }) => {
            Check::new("rustview", Status::Problem, "RustView cannot be reached").detail(format!(
                "{reason}. RustCut keeps trying; saved jobs reach RustView later."
            ))
        }
        Some(ConnectionState::Disabled) => {
            Check::new("rustview", Status::Info, "RustView is not used").detail("The link is switched off.")
        }
        Some(ConnectionState::Unknown) | None => {
            Check::new("rustview", Status::Info, "Connecting to RustView").detail(format!("Library: {library}"))
        }
    }
}

/// The checks of `group` that need attention (most severe first, else in their order) and those that passed.
pub fn split(group: &[Check]) -> (Vec<Check>, Vec<Check>) {
    let mut attention: Vec<Check> = group.iter().filter(|c| c.status >= Status::Warning).cloned().collect();
    attention.sort_by_key(|c| std::cmp::Reverse(c.status));
    let passed = group.iter().filter(|c| c.status < Status::Warning).cloned().collect();
    (attention, passed)
}

/// The one-line summary of a group ("All 7 checks passed", "1 problem, 2 warnings").
pub fn summary(group: &[Check]) -> String {
    let count = |s: Status| group.iter().filter(|c| c.status == s).count();
    let (problems, warnings) = (count(Status::Problem), count(Status::Warning));
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    match (problems, warnings) {
        (0, 0) if group.is_empty() => "Not checked yet".into(),
        (0, 0) => format!("All {} checks passed", group.len()),
        (0, w) => plural(w, "warning"),
        (p, 0) => plural(p, "problem"),
        (p, w) => format!("{}, {}", plural(p, "problem"), plural(w, "warning")),
    }
}

/// The report as text to paste into a message (Markdown).
pub fn report_text(report: &Report, header: &str) -> String {
    use std::fmt::Write as _;
    // Writing into a String cannot fail.
    let mut text = format!("# {header}\n");
    for (title, group) in [("Dependencies", &report.dependencies), ("Health", &report.health)] {
        let _ = write!(text, "\n## {title} ({})\n\n", summary(group));
        for check in group {
            let _ = write!(text, "- [{}] {}", check.status.word(), check.title);
            if !check.detail.is_empty() {
                let _ = write!(text, " — {}", check.detail);
            }
            text.push('\n');
            for command in &check.fix {
                let _ = writeln!(text, "  - fix: `{command}`");
            }
        }
    }
    text
}

#[cfg(test)]
mod tests;
