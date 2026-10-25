//! Whether ffmpeg and ffprobe can be started at all (M6.5, user decision Q-N10): without them nothing can be probed or
//! rendered, so a critical banner says so and leads to the programs' settings. Checked on the runtime at the start and
//! whenever the programs change in the Settings window; the banner goes once both start.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::ui::dispatcher::UiDispatcher;
use crate::ui::jump::JumpTarget;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::settings::page::Page;

/// Longer than a cold start of ffmpeg from a slow disk; a program that starts and then hangs is no "missing" program.
const LIMIT: Duration = Duration::from_secs(10);

/// Checks both programs (the settings' paths, else the names in `PATH`) and shows or clears the banner.
pub fn check(runtime: &tokio::runtime::Handle, ui: &UiDispatcher, ffmpeg: Option<PathBuf>, ffprobe: Option<PathBuf>) {
    let ui = ui.clone();
    runtime.spawn(async move {
        let (ffmpeg, ffprobe) = tokio::join!(cannot_start("ffmpeg", ffmpeg), cannot_start("ffprobe", ffprobe));
        let failed: Vec<(&'static str, String)> = [ffmpeg, ffprobe].into_iter().flatten().collect();
        ui.post(move || show(&failed));
    });
}

/// Why `program` (or `name` from `PATH`) cannot be started; `None` when it starts.
async fn cannot_start(name: &'static str, program: Option<PathBuf>) -> Option<(&'static str, String)> {
    let program = program.unwrap_or_else(|| PathBuf::from(name));
    let mut command = tokio::process::Command::new(&program);
    command
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    match rustcut_core::process::spawn(&mut command).await {
        Ok(mut child) => {
            let _ = tokio::time::timeout(LIMIT, child.wait()).await;
            None
        }
        Err(e) => {
            tracing::error!(program = %program.display(), error = %e, "[App] A needed program cannot be started");
            Some((name, format!("{}: {e}", program.display())))
        }
    }
}

fn show(failed: &[(&'static str, String)]) {
    if failed.is_empty() {
        notices::clear_kind(Kind::ToolsMissing);
        return;
    }
    let names: Vec<&str> = failed.iter().map(|(name, _)| *name).collect();
    let reasons: Vec<&str> = failed.iter().map(|(_, why)| why.as_str()).collect();
    let target = JumpTarget::Settings {
        page: Page::Advanced,
        key: if names.first() == Some(&"ffprobe") {
            "advanced.ffprobe"
        } else {
            "advanced.ffmpeg"
        },
    };
    notices::post(
        Notice::new(Kind::ToolsMissing, format!("{} cannot be started", names.join(" and ")))
            .detail(format!(
                "{}. Nothing can be probed or rendered: install it (sudo pacman -S ffmpeg) or choose the program in \
                 the settings.",
                reasons.join("; ")
            ))
            .jump(target)
            .jump_action("Show the option", target),
    );
}
