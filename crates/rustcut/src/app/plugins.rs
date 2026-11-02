//! Starting the plugins (P12 block 8): the service with the settings and `--no-plugins`, the bridge, and the hooks
//! that turn opened videos and the user's tag clicks into plugin events.

use std::rc::Rc;

use rustcut_core::plugins::{PluginCommand, PluginPaths, spawn_plugin_service};
use rustcut_core::settings::{AppPaths, Settings};

use super::SettingsWriter;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::export::ExportBridge;
use crate::ui::open::FileOpener;
use crate::ui::plugins::PluginsBridge;
use crate::ui::tags::TagsBridge;

pub(super) struct Parts<'a> {
    pub ui: &'a UiDispatcher,
    pub runtime: tokio::runtime::Handle,
    pub paths: &'a AppPaths,
    pub settings: &'a Settings,
    pub safe_mode: bool,
    pub writer: &'a SettingsWriter,
    pub export: &'a Rc<ExportBridge>,
    pub tags: &'a Rc<TagsBridge>,
    pub opener: &'a Rc<FileOpener>,
}

pub(super) fn start(parts: &Parts<'_>) -> Rc<PluginsBridge> {
    if parts.safe_mode {
        tracing::info!("[Plugins] --no-plugins: no plugin runs in this start");
    }
    let paths = PluginPaths {
        installed: parts.paths.plugins_dir(),
        cache: parts.paths.plugin_cache_dir(),
    };
    let service = spawn_plugin_service(&parts.runtime, paths, parts.settings.plugins.clone(), parts.safe_mode);
    let handle = service.handle.clone();
    parts.writer.subscribe(move |before, after| {
        if before.plugins != after.plugins {
            let _ = handle.send(PluginCommand::Apply(Box::new(after.plugins.clone())));
        }
    });
    let bridge = PluginsBridge::install(parts.ui, parts.runtime.clone(), service, parts.export, parts.tags);
    let weak = Rc::downgrade(&bridge);
    parts.opener.set_on_opened(move |path, by_rustcut| {
        if let Some(bridge) = weak.upgrade() {
            bridge.video_opened(path, by_rustcut);
        }
    });
    bridge
}
