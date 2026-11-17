//! Plugin commands in the main window (plugin API 1.2, Q-NF1): their keys (after the keymap), their File menu entries
//! and menu bar buttons, the hold when they would discard unsaved sections, and what their run brings back (actions,
//! a video to open, notes at the button).
//!
//! The list of commands (`registry`) is rebuilt when the plugins, their settings or the keymap change; the Settings
//! window reads it through [`entries`] and [`listen`] for the key conflicts and the Hotkeys page's "Plugins" group.

mod registry;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use rustcut_core::plugins::{Outcome, PluginCommand};
use rustcut_core::settings::Settings;
use slint::{ComponentHandle, ModelRc, VecModel};

pub use registry::{CommandEntry, Holder, build, holder_of, lookup};

use super::PluginsBridge;
use crate::app::SettingsWriter;
use crate::icons::Icon;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::editor::Editor;
use crate::ui::keymap::{KeyPress, Keymap};
use crate::ui::notices::{self, Area, Kind, Notice};
use crate::ui::open::FileOpener;
use crate::ui::queue::QueueBridge;
use crate::{IconId, MainWindow, PluginCommandView, PluginCommands};

/// The key of the message while a command's key is held.
const HOLD_NOTICE: &str = "plugin-command-hold";
/// How long a note stays at its button.
const NOTE_TIME: Duration = Duration::from_secs(4);

thread_local! {
    static BRIDGE: RefCell<Weak<CommandsBridge>> = const { RefCell::new(Weak::new()) };
}

/// The commands of the running plugins now (empty before the bridge exists).
pub fn entries() -> Rc<Vec<CommandEntry>> {
    BRIDGE
        .with(|b| b.borrow().upgrade())
        .map_or_else(Rc::default, |b| Rc::clone(&b.entries.borrow()))
}

/// `hook` hears every change of [`entries`].
pub fn listen(hook: impl Fn() + 'static) {
    if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) {
        bridge.listeners.borrow_mut().push(Box::new(hook));
    }
}

/// What the bridge needs.
pub struct Parts<'a> {
    pub window: &'a MainWindow,
    pub ui: &'a UiDispatcher,
    pub runtime: tokio::runtime::Handle,
    pub writer: &'a SettingsWriter,
    pub plugins: &'a Rc<PluginsBridge>,
    pub editor: &'a Rc<Editor>,
    pub opener: &'a Rc<FileOpener>,
    pub queue: &'a Rc<QueueBridge>,
}

/// A key held because the command would discard unsaved sections.
struct Held {
    plugin: String,
    id: String,
    timer: slint::Timer,
}

pub struct CommandsBridge {
    me: Weak<Self>,
    ui: UiDispatcher,
    runtime: tokio::runtime::Handle,
    plugins: Weak<PluginsBridge>,
    editor: Weak<Editor>,
    opener: Weak<FileOpener>,
    queue: Weak<QueueBridge>,
    settings: RefCell<Settings>,
    entries: RefCell<Rc<Vec<CommandEntry>>>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
    held: RefCell<Option<Held>>,
    hold_ms: Cell<u64>,
    menu: Rc<VecModel<PluginCommandView>>,
    buttons: Rc<VecModel<PluginCommandView>>,
}

impl CommandsBridge {
    pub fn install(parts: &Parts<'_>) -> Rc<Self> {
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            ui: parts.ui.clone(),
            runtime: parts.runtime.clone(),
            plugins: Rc::downgrade(parts.plugins),
            editor: Rc::downgrade(parts.editor),
            opener: Rc::downgrade(parts.opener),
            queue: Rc::downgrade(parts.queue),
            settings: RefCell::new(parts.writer.current()),
            entries: RefCell::new(Rc::new(Vec::new())),
            listeners: RefCell::new(Vec::new()),
            held: RefCell::new(None),
            hold_ms: Cell::new(0),
            menu: Rc::new(VecModel::default()),
            buttons: Rc::new(VecModel::default()),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        let global = parts.window.global::<PluginCommands<'_>>();
        global.set_menu(ModelRc::from(Rc::clone(&bridge.menu)));
        global.set_buttons(ModelRc::from(Rc::clone(&bridge.buttons)));
        let weak = Rc::downgrade(&bridge);
        global.on_run(move |plugin, id, from_button| {
            if let Some(bridge) = weak.upgrade() {
                bridge.run(plugin.as_str(), id.as_str(), from_button);
            }
        });
        let weak = Rc::downgrade(&bridge);
        parts.writer.subscribe(move |before, after| {
            let relevant = before.plugins != after.plugins
                || before.keymap != after.keymap
                || before.general.hold_to_confirm_ms != after.general.hold_to_confirm_ms;
            if relevant && let Some(bridge) = weak.upgrade() {
                bridge.settings.replace(after.clone());
                bridge.rebuild();
            }
        });
        let weak = Rc::downgrade(&bridge);
        parts.plugins.listen(move || {
            if let Some(bridge) = weak.upgrade() {
                bridge.rebuild();
            }
        });
        let (keys, release) = (Rc::downgrade(&bridge), Rc::downgrade(&bridge));
        parts.editor.set_key_hooks(
            Box::new(move |press| keys.upgrade().is_some_and(|b| b.key(press))),
            Box::new(move |_| {
                if let Some(bridge) = release.upgrade() {
                    bridge.released();
                }
            }),
        );
        let window = parts.window.as_weak();
        parts.editor.set_unsaved_hook(Box::new(move |unsaved| {
            if let Some(window) = window.upgrade() {
                window.global::<PluginCommands<'_>>().set_unsaved(unsaved);
            }
        }));
        bridge.rebuild();
        bridge
    }

    fn rebuild(&self) {
        let (entries, hold_ms) = {
            let settings = self.settings.borrow();
            let (keymap, _) = Keymap::from_table(&settings.keymap);
            let list = self.plugins.upgrade().map(|p| p.list()).unwrap_or_default();
            (
                build(&list, &settings.plugins, &keymap),
                u64::from(settings.general.hold_to_confirm_ms),
            )
        };
        self.hold_ms.set(hold_ms);
        let view = |e: &CommandEntry| PluginCommandView {
            plugin: e.plugin.as_str().into(),
            id: e.id.as_str().into(),
            title: e.title.as_str().into(),
            tooltip: if e.description.is_empty() {
                e.title.as_str()
            } else {
                e.description.as_str()
            }
            .into(),
            shortcut: e.key_label().into(),
            icon: icon(&e.icon),
            discards: e.discards_sections,
        };
        crate::util::model::sync(
            &self.menu,
            entries.iter().filter(|e| e.menu).take(6).map(view).collect(),
        );
        crate::util::model::sync(&self.buttons, entries.iter().filter(|e| e.button).map(view).collect());
        tell_taken(&self.entries.borrow(), &entries);
        self.entries.replace(Rc::new(entries));
        for listener in self.listeners.borrow().iter() {
            listener();
        }
    }

    /// A key the keymap does not know.
    fn key(&self, press: &KeyPress<'_>) -> bool {
        let entries = Rc::clone(&self.entries.borrow());
        let Some(entry) = lookup(&entries, press) else {
            return false;
        };
        if press.repeat {
            return true;
        }
        let unsaved = self
            .editor
            .upgrade()
            .filter(|e| e.has_unsaved_sections())
            .map(|e| e.section_count());
        match unsaved {
            Some(count) if entry.discards_sections && self.hold_ms.get() > 0 => self.start_hold(entry, count),
            _ => self.run(&entry.plugin, &entry.id, false),
        }
        true
    }

    /// The key must stay down for the hold time; releasing it earlier cancels.
    fn start_hold(&self, entry: &CommandEntry, sections: usize) {
        let timer = slint::Timer::default();
        let weak = Weak::clone(&self.me);
        timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(self.hold_ms.get()),
            move || {
                if let Some(bridge) = weak.upgrade()
                    && let Some(held) = bridge.held.take()
                {
                    notices::clear(HOLD_NOTICE);
                    bridge.run(&held.plugin, &held.id, false);
                }
            },
        );
        let noun = if sections == 1 { "section is" } else { "sections are" };
        notices::post(
            Notice::new(
                Kind::PluginMessage,
                format!("Keep holding {} to go on", entry.key_label()),
            )
            .detail(format!(
                "{sections} {noun} not saved as a job; {} discards them.",
                entry.title
            ))
            .live(HOLD_NOTICE),
        );
        tracing::info!(
            "[Plugins] {}: {} waits for the held key ({sections} unsaved sections)",
            entry.plugin,
            entry.id
        );
        self.held.replace(Some(Held {
            plugin: entry.plugin.clone(),
            id: entry.id.clone(),
            timer,
        }));
    }

    fn released(&self) {
        if let Some(held) = self.held.take() {
            held.timer.stop();
            notices::clear(HOLD_NOTICE);
            tracing::info!(
                "[Plugins] {}: {} canceled (the key was released early)",
                held.plugin,
                held.id
            );
        }
    }

    /// Runs a command; `from_button`: its menu bar button (its notes appear there).
    pub fn run(&self, plugin: &str, id: &str, from_button: bool) {
        let entries = Rc::clone(&self.entries.borrow());
        let Some(entry) = entries.iter().find(|e| e.plugin == plugin && e.id == id).cloned() else {
            tracing::warn!("[Plugins] {plugin}: the command {id} is gone");
            return;
        };
        let Some(plugins) = self.plugins.upgrade() else { return };
        let context = plugins.context();
        let job_sources: Arc<Vec<PathBuf>> = Arc::new(self.queue.upgrade().map(|q| q.sources()).unwrap_or_default());
        let (reply, answer) = tokio::sync::oneshot::channel();
        let sent = plugins.service().handle.send(PluginCommand::RunCommand {
            id: plugin.to_owned(),
            command: id.to_owned(),
            context,
            job_sources,
            reply,
        });
        if sent.is_err() {
            return;
        }
        tracing::info!("[Plugins] {plugin}: command {id}");
        let ui = self.ui.clone();
        let at_button = from_button || entry.button;
        self.runtime.spawn(async move {
            let Ok(result) = answer.await else { return };
            ui.post(move || {
                let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) else {
                    return;
                };
                match result {
                    Ok(result) => bridge.finished(&entry, at_button, result),
                    Err(error) => tracing::warn!("[Plugins] {}: command {} failed: {error}", entry.plugin, entry.id),
                }
            });
        });
    }

    fn finished(&self, entry: &CommandEntry, at_button: bool, result: rustcut_core::plugins::CommandResult) {
        let outcome = result.outcome;
        if let Some(plugins) = self.plugins.upgrade() {
            plugins.apply(&[Outcome {
                id: result.id.clone(),
                name: result.name.clone(),
                actions: outcome.actions,
            }]);
        }
        let anchor = format!("plugin-button:{}:{}", entry.plugin, entry.id);
        for text in outcome.bubbles {
            let title = format!("{}: {text}", result.name);
            if at_button {
                notices::post(
                    Notice::new(Kind::PluginBubble, title)
                        .at(Area::Window, [anchor.clone()])
                        .key(anchor.clone()),
                );
                notices::clear_soon(&anchor, NOTE_TIME);
            } else {
                notices::post(Notice::new(Kind::PluginMessage, title));
            }
        }
        if let Some(path) = outcome.open
            && let Some(opener) = self.opener.upgrade()
        {
            tracing::info!("[Plugins] {}: opens {}", entry.plugin, path.display());
            opener.open(path);
        }
    }
}

/// A command whose key became taken (its plugin was switched on, or a key changed) gets a banner with the way to its
/// page; the banner goes when the key works again (Q-NF1: "enabling a plugin always checks its keys").
fn tell_taken(before: &[CommandEntry], after: &[CommandEntry]) {
    for entry in after {
        let key = format!("plugin-key-taken:{}:{}", entry.plugin, entry.id);
        let Some(holder) = &entry.taken_by else {
            notices::clear(&key);
            continue;
        };
        let was = before.iter().find(|e| e.plugin == entry.plugin && e.id == entry.id);
        if was.is_some_and(|e| e.taken_by.as_ref() == Some(holder) && e.key_text == entry.key_text) {
            continue;
        }
        let label = entry
            .chord
            .as_ref()
            .map(crate::ui::keymap::Chord::label)
            .unwrap_or_default();
        let plugin = entry.plugin.clone();
        notices::post(
            Notice::new(
                Kind::PluginBanner,
                format!("{}: {} has no key", entry.plugin_name, entry.title),
            )
            .detail(format!(
                "{label} already belongs to {}. Record another key on the plugin's page.",
                holder.describe()
            ))
            .level(notices::Level::Warning)
            .key(key)
            .action("Open plugin", move || super::show_settings(&plugin)),
        );
    }
    for gone in before
        .iter()
        .filter(|b| !after.iter().any(|a| a.plugin == b.plugin && a.id == b.id))
    {
        notices::clear(&format!("plugin-key-taken:{}:{}", gone.plugin, gone.id));
    }
}

/// The registry's symbol for a Tabler name (`player-track-next`), the plugin symbol when RustCut has none.
fn icon(name: &str) -> IconId {
    let wanted = format!("/{name}.svg");
    Icon::ALL
        .iter()
        .find(|icon| !name.is_empty() && icon.spec().is_some_and(|spec| spec.source.ends_with(&wanted)))
        .map_or(IconId::Plugin, |icon| icon.to_slint())
}
