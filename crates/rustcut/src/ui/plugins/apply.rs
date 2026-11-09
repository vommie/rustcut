//! Applying what the plugins answered: targets, ratings and messages, in order.

use rustcut_core::plugins::{Action, Level as PluginLevel, Notice as PluginNotice, Outcome, Place, Target};

use super::{PluginsBridge, show_settings};
use crate::ui::export::{PluginMark, PluginTarget};
use crate::ui::notices::{self, Kind, Level, Notice};

impl PluginsBridge {
    pub(super) fn apply(&self, outcomes: &[Outcome]) {
        for outcome in outcomes {
            for action in &outcome.actions {
                self.apply_one(outcome, action);
            }
        }
    }

    fn apply_one(&self, outcome: &Outcome, action: &Action) {
        let id = outcome.id.as_str();
        // A plugin may hide its marks (API 1.1): the action applies without one.
        let mark = |reason: &str, shown: bool| {
            shown.then(|| PluginMark {
                id: id.to_owned(),
                plugin: outcome.name.clone(),
                reason: tidy(reason),
            })
        };
        match action {
            Action::SetTarget {
                target,
                reason,
                mark: shown,
            } => {
                let Some(export) = self.export.upgrade() else { return };
                let (folder, path) = match target {
                    Target::Folder(folder) => (Some(*folder), None),
                    Target::Path(path) => (None, Some(path.as_path())),
                };
                match export.plugin_target(folder, path, mark(reason, *shown)) {
                    PluginTarget::Set(folder) => {
                        tracing::info!("[Plugins] {id}: target folder set ({reason})");
                        if let Some(folder) = folder {
                            self.check_folder(outcome, folder);
                        }
                    }
                    PluginTarget::UserChose => {
                        tracing::info!("[Plugins] {id}: the target stays, the user chose it for this video");
                    }
                    PluginTarget::UnknownFolder(folder) => {
                        tracing::warn!("[Plugins] {id}: target folder {folder} is not in Settings > Target Folders");
                        let id = id.to_owned();
                        notices::post(
                            Notice::new(Kind::PluginTargetMissing, format!("{}: the target folder no longer exists", outcome.name))
                                .detail(format!(
                                    "{} ({reason}) names a folder that is not in Settings > Target Folders any more; the target stays.",
                                    outcome.name
                                ))
                                .key(format!("plugin-target:{id}"))
                                .action("Open plugin", move || show_settings(&id)),
                        );
                    }
                }
            }
            Action::SetRating {
                stars,
                reason,
                mark: shown,
            } => {
                let Some(tags) = self.tags.upgrade() else { return };
                if tags.plugin_rating(*stars, mark(reason, *shown)) {
                    tracing::info!("[Plugins] {id}: rating set to {stars} ({reason})");
                } else {
                    tracing::info!("[Plugins] {id}: no rating set, nothing can be rated now");
                }
            }
            Action::Notify(notice) => post(&outcome.name, notice),
        }
    }

    /// A target folder that does not exist on disk gets a banner (checked on the runtime).
    fn check_folder(&self, outcome: &Outcome, folder: std::path::PathBuf) {
        let ui = self.ui.clone();
        let (id, name) = (outcome.id.clone(), outcome.name.clone());
        let check = self.runtime.spawn_blocking(move || (folder.is_dir(), folder));
        self.runtime.spawn(async move {
            let Ok((exists, folder)) = check.await else { return };
            if exists {
                return;
            }
            super::with_bridge(&ui, move |_| {
                notices::post(
                    Notice::new(
                        Kind::PluginTargetMissing,
                        format!("{name}: the target folder does not exist"),
                    )
                    .detail(format!("{} (rendering creates it)", folder.display()))
                    .key(format!("plugin-target:{id}")),
                );
            });
        });
    }
}

/// The home folder written as `~` (plugins get absolute paths; the user typed `~`).
fn tidy(text: &str) -> String {
    match directories::BaseDirs::new() {
        Some(dirs) => {
            let home = dirs.home_dir().display().to_string();
            if home.len() > 1 {
                text.replace(&format!("{home}/"), "~/")
            } else {
                text.to_owned()
            }
        }
        None => text.to_owned(),
    }
}

/// A plugin's message, with its name in front.
fn post(plugin: &str, notice: &PluginNotice) {
    let kind = match notice.place {
        Place::None => {
            tracing::info!("[Plugins] {plugin}: {} {}", notice.title, notice.detail);
            return;
        }
        Place::Toast => Kind::PluginMessage,
        Place::Banner => Kind::PluginBanner,
    };
    let level = match notice.level {
        PluginLevel::Debug | PluginLevel::Info => Level::Info,
        PluginLevel::Warning => Level::Warning,
        PluginLevel::Error => Level::Error,
    };
    notices::post(
        Notice::new(kind, format!("{plugin}: {}", notice.title))
            .detail(tidy(&notice.detail))
            .level(level),
    );
}
