//! Settings > Plugins and each plugin's page (P12 block 8, Q-P12g): the list with its management (install, update,
//! remove, switch, order, reload, development folders, details and statistics) and the plugin's settings, which its
//! `plugin.toml` declares and RustCut draws with its own controls.

mod edit;
mod files;
mod page;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use rustcut_core::plugins::{Available, Issue, PluginCommand, PluginInfo, Source};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::SettingsBridge;
use super::page::Page;
use super::pages::PageRef;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::plugins::PluginsBridge;
use crate::{PluginAvailableView, PluginsUi, SettingsWindow};

/// A key of a plugin page's option: `plugin:<id>:<field>`; RustCut's own options of the page start with `#`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginKey {
    pub id: String,
    pub field: String,
}

impl PluginKey {
    pub fn parse(key: &str) -> Option<Self> {
        let (id, field) = key.strip_prefix("plugin:")?.split_once(':')?;
        (!id.is_empty() && !field.is_empty()).then(|| Self {
            id: id.to_owned(),
            field: field.to_owned(),
        })
    }

    pub fn text(id: &str, field: &str) -> String {
        format!("plugin:{id}:{field}")
    }
}

/// What the plugin pages keep while RustCut runs.
#[derive(Default)]
pub struct PluginPages {
    /// The plugins whose details are unfolded in the list.
    expanded: RefCell<HashSet<String>>,
    /// Each plugin's findings about its settings (its own `check` plus RustCut's), by plugin id.
    issues: RefCell<HashMap<String, Vec<Issue>>>,
    /// Plugin files in the update folder that are not installed.
    available: RefCell<Vec<Available>>,
    checking: Cell<bool>,
    installing: Cell<bool>,
    /// Counts the checks asked for, so an older answer never replaces a newer one.
    check_serial: Cell<u64>,
    /// The plugins' pictures, by plugin id and icon.
    icons: RefCell<HashMap<(String, String), slint::Image>>,
}

fn with<R: Default>(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge) -> R) -> R {
    this.upgrade().map(|b| f(&b)).unwrap_or_default()
}

/// The callbacks of `PluginsUi`.
pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    let ui = window.global::<PluginsUi<'_>>();
    let t = this.clone();
    ui.on_toggle(move |id| with(&t, |b| b.plugin_toggle(&id)));
    let t = this.clone();
    ui.on_expand(move |id| with(&t, |b| b.plugin_expand(&id)));
    let t = this.clone();
    ui.on_move(move |id, delta| with(&t, |b| b.plugin_move(&id, delta)));
    let t = this.clone();
    ui.on_install(move || with(&t, SettingsBridge::plugin_install_dialog));
    let t = this.clone();
    ui.on_add_dev(move || with(&t, SettingsBridge::plugin_add_dev_dialog));
    let t = this.clone();
    ui.on_remove_dev(move |index| with(&t, |b| b.plugin_remove_dev(index)));
    let t = this.clone();
    ui.on_check_updates(move || with(&t, SettingsBridge::plugin_check_updates));
    let t = this.clone();
    ui.on_install_available(move |index| with(&t, |b| b.plugin_install_available(index)));
    let t = this.clone();
    ui.on_update(move |id| with(&t, |b| b.plugin_update(&id)));
    let t = this.clone();
    ui.on_reload(move |id| with(&t, |b| b.plugin_reload(&id)));
    let t = this.clone();
    ui.on_open_folder(move |id| with(&t, |b| b.plugin_open_folder(&id)));
    let t = this.clone();
    ui.on_remove(move |id| with(&t, |b| b.plugin_remove(&id)));
    let t = this.clone();
    ui.on_export_settings(move |id| with(&t, |b| b.plugin_export(&id)));
    let t = this.clone();
    ui.on_import_settings(move |id| with(&t, |b| b.plugin_import(&id)));
    let t = this.clone();
    ui.on_open_page(move |id| with(&t, |b| b.show_page(PageRef::Plugin(id.to_string()))));
    edit::connect(this, &ui);
}

impl SettingsBridge {
    /// Connects the plugins: their pages follow the list, and the app's marks and messages open a plugin's page.
    pub fn attach_plugins(&self, plugins: &Rc<PluginsBridge>) {
        self.hooks.borrow_mut().plugins = Some(Rc::clone(plugins));
        let weak = Weak::clone(&self.me);
        plugins.listen(move || {
            if let Some(b) = weak.upgrade()
                && b.window.borrow().as_ref().is_some_and(|w| w.window().is_visible())
            {
                b.publish();
            }
        });
        let weak = Weak::clone(&self.me);
        plugins.set_open_page(move |id| {
            if let Some(b) = weak.upgrade() {
                b.open(None);
                b.show_page(PageRef::Plugin(id.to_owned()));
            }
        });
    }

    fn plugins(&self) -> Option<Rc<PluginsBridge>> {
        self.hooks.borrow().plugins.clone()
    }

    fn plugin_info(&self, id: &str) -> Option<PluginInfo> {
        self.plugins()?.list().iter().find(|p| p.id == id).cloned()
    }

    fn plugin_command(&self, command: PluginCommand) {
        if let Some(plugins) = self.plugins() {
            let _ = plugins.service().handle.send(command);
        }
    }

    /// Settings > Plugins.
    pub(in crate::ui::settings) fn publish_plugins(
        &self,
        window: &SettingsWindow,
        settings: &rustcut_core::settings::Settings,
    ) {
        let ui = window.global::<PluginsUi<'_>>();
        let list = self.plugins().map(|p| p.list()).unwrap_or_default();
        let expanded = self.plugin_pages.expanded.borrow().clone();
        let items: Vec<_> = list
            .iter()
            .enumerate()
            .map(|(i, info)| {
                let mut item = view::item(info, settings, &expanded, self.plugin_icon(info));
                item.can_move_up = i > 0;
                item.can_move_down = i + 1 < list.len();
                item
            })
            .collect();
        ui.set_plugins(ModelRc::new(VecModel::from(items)));
        let folders: Vec<slint::SharedString> = settings
            .plugins
            .dev_folders
            .iter()
            .map(|f| f.display().to_string().into())
            .collect();
        ui.set_dev_folders(ModelRc::new(VecModel::from(folders)));
        let available: Vec<PluginAvailableView> = self
            .plugin_pages
            .available
            .borrow()
            .iter()
            .map(|a| PluginAvailableView {
                name: a.manifest.name.as_str().into(),
                version: a.manifest.version.to_string().into(),
                description: a.manifest.description.as_str().into(),
            })
            .collect();
        ui.set_available(ModelRc::new(VecModel::from(available)));
        ui.set_checking(self.plugin_pages.checking.get());
        ui.set_installing(self.plugin_pages.installing.get());
    }

    /// The picture of a plugin: a Tabler name RustCut knows, its own `icon.svg` (read on the runtime), or the puzzle.
    fn plugin_icon(&self, info: &PluginInfo) -> slint::Image {
        let icon = info.manifest.as_ref().map(|m| m.icon.clone()).unwrap_or_default();
        let key = (info.id.clone(), icon.clone());
        if let Some(image) = self.plugin_pages.icons.borrow().get(&key) {
            return image.clone();
        }
        let image = if icon == "icon.svg" {
            self.load_icon_file(key.clone(), info.dir.join("icon.svg"));
            view::svg_image(view::PUZZLE_SVG)
        } else {
            view::svg_image(crate::ui::folders::pictures::icon_svg(&icon).unwrap_or(view::PUZZLE_SVG))
        };
        self.plugin_pages.icons.borrow_mut().insert(key, image.clone());
        image
    }

    fn load_icon_file(&self, key: (String, String), path: std::path::PathBuf) {
        let read = self.runtime.spawn_blocking(move || std::fs::read_to_string(path).ok());
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Ok(Some(svg)) = read.await else { return };
            let Some(b) = weak.upgrade() else { return };
            b.plugin_pages.icons.borrow_mut().insert(key, view::svg_image(&svg));
            b.publish();
        });
        if let Err(e) = spawned {
            tracing::debug!(error = %e, "[Plugins] The event loop is gone; no plugin picture");
        }
    }

    fn plugin_toggle(&self, id: &str) {
        let id = id.to_owned();
        self.update(|s| {
            let entry = s.plugins.entries.entry(id.clone()).or_default();
            entry.enabled = !entry.enabled;
            tracing::info!("[Plugins] {id}: switched {}", if entry.enabled { "on" } else { "off" });
        });
    }

    fn plugin_expand(&self, id: &str) {
        let mut expanded = self.plugin_pages.expanded.borrow_mut();
        if !expanded.remove(id) {
            expanded.insert(id.to_owned());
        }
        drop(expanded);
        self.publish();
    }

    /// Moves a plugin up (`delta` -1) or down in the order (the order lists every plugin then).
    fn plugin_move(&self, id: &str, delta: i32) {
        let ids: Vec<String> = self
            .plugins()
            .map(|p| p.list().iter().map(|i| i.id.clone()).collect())
            .unwrap_or_default();
        let Some(from) = ids.iter().position(|i| i == id) else {
            return;
        };
        let to = if delta < 0 {
            from.checked_sub(1)
        } else {
            Some(from + 1).filter(|to| *to < ids.len())
        };
        let Some(to) = to else { return };
        let mut order = ids;
        order.swap(from, to);
        self.update(|s| s.plugins.order = order);
    }

    fn plugin_reload(&self, id: &str) {
        let id = (!id.is_empty()).then(|| id.to_owned());
        tracing::info!("[Plugins] Reload {}", id.as_deref().unwrap_or("every plugin"));
        self.plugin_command(PluginCommand::Reload { id });
    }

    fn plugin_open_folder(&self, id: &str) {
        let folder = if id.is_empty() {
            Some(self.paths.plugins_dir())
        } else {
            self.plugin_info(id).map(|info| info.dir)
        };
        let Some(folder) = folder else { return };
        self.runtime.spawn_blocking(move || {
            if let Err(e) =
                std::fs::create_dir_all(&folder).and_then(|()| opener::open(&folder).map_err(std::io::Error::other))
            {
                tracing::warn!(folder = %folder.display(), error = %e, "[Plugins] The folder could not be opened");
            }
        });
    }

    /// Removes an installed plugin (its settings stay) or disconnects a development folder (its files stay).
    fn plugin_remove(&self, id: &str) {
        let Some(info) = self.plugin_info(id) else { return };
        if info.source == Source::Development {
            let dir = info.dir.clone();
            self.update(|s| s.plugins.dev_folders.retain(|f| *f != dir));
            return;
        }
        let Some(plugins) = self.plugins() else { return };
        let handle = plugins.service().handle.clone();
        let (id, name) = (id.to_owned(), info.name().to_owned());
        let removing = self
            .runtime
            .spawn(async move { handle.request(|reply| PluginCommand::Remove { id, reply }).await });
        let spawned = slint::spawn_local(async move {
            match removing.await {
                Ok(Ok(Ok(()))) => notices::post(
                    Notice::new(Kind::PluginInstalled, format!("{name} removed"))
                        .detail("Its settings stay for a reinstall."),
                ),
                Ok(Ok(Err(err))) => {
                    notices::post(
                        Notice::new(Kind::PluginInstallFailed, format!("{name} was not removed")).detail(err),
                    );
                }
                _ => {}
            }
        });
        if let Err(e) = spawned {
            tracing::debug!(error = %e, "[Plugins] The event loop is gone");
        }
    }

    fn plugin_remove_dev(&self, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        self.update(|s| {
            if index < s.plugins.dev_folders.len() {
                let folder = s.plugins.dev_folders.remove(index);
                tracing::info!("[Plugins] Development folder disconnected: {}", folder.display());
            }
        });
    }

    fn plugin_check_updates(&self) {
        let Some(plugins) = self.plugins() else { return };
        if self.writer.current().plugins.update_folder.is_none() {
            notices::post(
                Notice::new(Kind::PluginMessage, "No update folder")
                    .detail("Choose one under Settings > Plugins > Update folder.")
                    .jump_action(
                        "Update folder",
                        crate::ui::jump::JumpTarget::Settings {
                            page: Page::Plugins,
                            key: "plugins.update_folder",
                        },
                    ),
            );
            return;
        }
        self.plugin_pages.checking.set(true);
        self.publish();
        let handle = plugins.service().handle.clone();
        let asking = self
            .runtime
            .spawn(async move { handle.request(PluginCommand::CheckUpdates).await });
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let available = asking.await.ok().and_then(Result::ok).unwrap_or_default();
            let Some(b) = weak.upgrade() else { return };
            b.plugin_pages.checking.set(false);
            let updates = b
                .plugins()
                .map(|p| p.list().iter().filter(|i| i.update.is_some()).count())
                .unwrap_or(0);
            let text = match (updates, available.len()) {
                (0, 0) => "Every plugin is up to date".to_owned(),
                (u, a) => format!("{u} update(s), {a} new plugin(s) in the update folder"),
            };
            tracing::info!("[Plugins] Update check: {text}");
            notices::post(Notice::new(Kind::PluginMessage, text));
            b.plugin_pages.available.replace(available);
            b.publish();
        });
        if let Err(e) = spawned {
            tracing::debug!(error = %e, "[Plugins] The event loop is gone");
        }
    }

    fn plugin_install_available(&self, index: i32) {
        let file = usize::try_from(index)
            .ok()
            .and_then(|i| self.plugin_pages.available.borrow().get(i).map(|a| a.file.clone()));
        if let Some(file) = file {
            self.plugin_install(file, false);
        }
    }

    fn plugin_update(&self, id: &str) {
        if let Some((_, file)) = self.plugin_info(id).and_then(|info| info.update) {
            self.plugin_install(file, false);
        }
    }

    /// A plugin file is dragged over the window.
    pub(in crate::ui::settings) fn plugin_file_over(&self, over: bool) {
        if let Some(window) = self.window.borrow().as_ref() {
            window
                .global::<PluginsUi<'_>>()
                .set_drop_hover(over && self.page.get() == Page::Plugins);
        }
    }

    /// Files dropped on a plugin page: every plugin file is installed.
    pub(in crate::ui::settings) fn plugins_dropped(&self, paths: Vec<std::path::PathBuf>) {
        let ext = rustcut_core::plugins::package_extension();
        for path in paths {
            if path.extension().is_some_and(|e| e == ext) {
                self.plugin_install(path, false);
            } else {
                tracing::info!("[Plugins] Not a plugin file: {}", path.display());
            }
        }
    }
}
