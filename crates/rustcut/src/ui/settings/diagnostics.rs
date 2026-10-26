//! Settings > Diagnostics (P9.16): the checks of `rustcut_core::diagnostics` run on the runtime when the page is first
//! shown and on "Check again"; the app's facts come from `Hooks::diagnostics`. The page shows each card's problems first
//! and folds the checks that passed (remembered in `SettingsWindowState::diagnostics_passed`); "Copy report" puts the
//! whole report on the clipboard.

use std::rc::Weak;

use rustcut_core::app_identity::{APP_NAME, VERSION};
use rustcut_core::diagnostics::{self, Check, Facts, Report, Status};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use super::SettingsBridge;
use crate::ui::notices::{self, Kind, Notice};
use crate::{DiagCheckView, DiagGroupView, SettingsUi, SettingsWindow};

/// The page's report and whether its checks run.
#[derive(Debug, Default)]
pub(super) struct DiagState {
    report: Option<Report>,
    running: bool,
}

fn check_view(check: &Check) -> DiagCheckView {
    DiagCheckView {
        id: check.id.as_str().into(),
        status: match check.status {
            Status::Ok => 0,
            Status::Info => 1,
            Status::Warning => 2,
            Status::Problem => 3,
        },
        title: check.title.as_str().into(),
        detail: check.detail.as_str().into(),
        fix: ModelRc::new(VecModel::from(
            check
                .fix
                .iter()
                .map(|c| SharedString::from(c.as_str()))
                .collect::<Vec<_>>(),
        )),
    }
}

fn group_view(group: &[Check]) -> DiagGroupView {
    let (attention, passed) = diagnostics::split(group);
    DiagGroupView {
        attention: ModelRc::new(VecModel::from(attention.iter().map(check_view).collect::<Vec<_>>())),
        passed: ModelRc::new(VecModel::from(passed.iter().map(check_view).collect::<Vec<_>>())),
        summary: diagnostics::summary(group).into(),
        alarming: group.iter().any(|c| c.status >= Status::Warning),
    }
}

impl SettingsBridge {
    /// The page is shown: the report so far, and the checks start when there is none yet.
    pub(super) fn publish_diagnostics(&self) {
        let start = {
            let diag = self.diag.borrow();
            diag.report.is_none() && !diag.running
        };
        if start {
            self.run_diagnostics();
        }
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<SettingsUi<'_>>();
        let diag = self.diag.borrow();
        let empty = Report::default();
        let report = diag.report.as_ref().unwrap_or(&empty);
        ui.set_diag_dependencies(group_view(&report.dependencies));
        ui.set_diag_health(group_view(&report.health));
        ui.set_diag_running(diag.running);
        let passed = self.state.borrow().diagnostics_passed;
        ui.set_diag_passed_shown(ModelRc::new(VecModel::from(passed.to_vec())));
    }

    /// Runs every check on the runtime; the page shows the result when it comes back.
    fn run_diagnostics(&self) {
        let Some(facts) = self.hooks.borrow().diagnostics.as_ref().map(|f| f()) else {
            tracing::debug!("[Settings] No facts for the diagnostics yet");
            return;
        };
        self.diag.borrow_mut().running = true;
        tracing::info!("[Settings] Diagnostics started");
        let task = self.runtime.spawn(diagnostics::run(facts));
        let me = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(report) = task.await else { return };
            let Some(bridge) = me.upgrade() else { return };
            let problems = report
                .dependencies
                .iter()
                .chain(&report.health)
                .filter(|c| c.status >= Status::Warning)
                .count();
            tracing::info!(attention = problems, "[Settings] Diagnostics done");
            {
                let mut diag = bridge.diag.borrow_mut();
                diag.report = Some(report);
                diag.running = false;
            }
            bridge.publish();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no diagnostics");
            self.diag.borrow_mut().running = false;
        }
    }

    /// "Check again".
    fn check_again(&self) {
        if !self.diag.borrow().running {
            self.run_diagnostics();
            self.publish();
        }
    }

    fn copy_report(&self) {
        let text = {
            let diag = self.diag.borrow();
            let Some(report) = diag.report.as_ref() else { return };
            diagnostics::report_text(report, &format!("{APP_NAME} {VERSION} diagnostics"))
        };
        if crate::util::clipboard::copy(text) {
            notices::post(Notice::new(Kind::LogCopied, "Diagnostics report copied"));
        }
    }

    fn toggle_passed(&self, group: usize) {
        if let Some(shown) = self.state.borrow_mut().diagnostics_passed.get_mut(group) {
            *shown = !*shown;
        }
        self.publish();
    }
}

/// Calls `f` with the bridge while it lives.
fn with(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge)) {
    if let Some(bridge) = this.upgrade() {
        f(&bridge);
    }
}

pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    let ui = window.global::<SettingsUi<'_>>();
    let t = this.clone();
    ui.on_diag_check(move || with(&t, SettingsBridge::check_again));
    let t = this.clone();
    ui.on_diag_copy(move || with(&t, SettingsBridge::copy_report));
    ui.on_diag_copy_line(|line| {
        if crate::util::clipboard::copy(line.to_string()) {
            notices::post(Notice::new(Kind::LogCopied, "Command copied"));
        }
    });
    let t = this.clone();
    ui.on_diag_passed_toggled(move |group| {
        if let Ok(group) = usize::try_from(group) {
            with(&t, |b| b.toggle_passed(group));
        }
    });
}

/// What the app knows, as the diagnostics' facts: the settings, the paths and what the services told.
pub fn facts(settings: &rustcut_core::settings::Settings, paths: &rustcut_core::settings::AppPaths) -> Facts {
    use rustcut_core::diagnostics::NamedPath;
    let data_home = std::env::var_os("XDG_DATA_HOME").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(|home| std::path::Path::new(&home).join(".local/share"))
                .unwrap_or_default()
        },
        std::path::PathBuf::from,
    );
    Facts {
        ffmpeg: settings.advanced.ffmpeg.clone(),
        ffprobe: settings.advanced.ffprobe.clone(),
        rustview: settings.rustview.binary.clone(),
        rustview_enabled: settings.rustview.enabled,
        config_file: paths.config_dir.join("config.toml"),
        ui_state_file: paths.config_dir.join("ui-state.toml"),
        database: paths.database(),
        folders: vec![
            NamedPath::new("Settings", &paths.config_dir),
            NamedPath::new("Data", &paths.data_dir),
            NamedPath::new("Logs", paths.log_dir()),
            NamedPath::new("Cache", &paths.cache_dir),
        ],
        targets: settings
            .export
            .target_folders
            .iter()
            .map(|f| NamedPath::new(f.label.clone(), &f.path))
            .collect(),
        cache: paths.cache_dir.clone(),
        rustview_library: data_home.join("rustview"),
        ..Facts::default()
    }
}
