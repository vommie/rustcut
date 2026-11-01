//! Jump marks (user request Q-N6): a message about an option offers a button that leads the user straight to it. The
//! panel or window is shown, a folded card unfolds, and the element with the target's key scrolls into view and
//! flashes (`ui/jump.slint`). Any bridge can lead there with [`go`]; the places are [`JumpTarget`]s.

mod target;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use slint::ComponentHandle as _;

pub use target::JumpTarget;

use crate::ui::dock::{DockBridge, PanelId};
use crate::ui::queue::QueueBridge;
use crate::ui::settings::SettingsBridge;
use crate::{Jump, MainWindow};

thread_local! {
    /// The bridge, for [`go`] from any other bridge on the UI thread.
    static BRIDGE: RefCell<Weak<JumpBridge>> = const { RefCell::new(Weak::new()) };
}

/// Leads the user to `target` (UI thread); nothing happens before the bridge is installed.
pub fn go(target: JumpTarget) {
    if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) {
        bridge.go(target);
    }
}

/// Lets the element `key` of the window whose `Jump` global this is flash and scroll into view.
pub fn flash(jump: &Jump<'_>, key: &str) {
    jump.set_key(key.into());
    jump.set_serial(jump.get_serial().wrapping_add(1));
}

pub struct JumpBridge {
    window: slint::Weak<MainWindow>,
    dock: Weak<DockBridge>,
    settings: Weak<SettingsBridge>,
    queue: Weak<QueueBridge>,
}

impl JumpBridge {
    /// Makes [`go`] work; the app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        dock: &Rc<DockBridge>,
        settings: &Rc<SettingsBridge>,
        queue: &Rc<QueueBridge>,
    ) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            dock: Rc::downgrade(dock),
            settings: Rc::downgrade(settings),
            queue: Rc::downgrade(queue),
        });
        BRIDGE.with(|b| *b.borrow_mut() = Rc::downgrade(&bridge));
        bridge
    }

    fn flash(&self, key: &str) {
        if let Some(window) = self.window.upgrade() {
            flash(&window.global::<Jump<'_>>(), key);
        }
    }

    fn go(&self, target: JumpTarget) {
        let key = target.element_key();
        tracing::debug!(key, "[Jump] Showing an option");
        match target {
            JumpTarget::Codecs(row) => {
                if let Some(dock) = self.dock.upgrade() {
                    dock.show_card(PanelId::RenderSettings, row.card());
                }
                self.flash(&key);
            }
            JumpTarget::Settings { page, .. } => {
                if let Some(settings) = self.settings.upgrade() {
                    settings.show_option(page, &key);
                }
            }
            JumpTarget::QueueJob(id) => {
                if let Some(dock) = self.dock.upgrade() {
                    dock.reveal(PanelId::Queue);
                }
                if let Some(queue) = self.queue.upgrade() {
                    queue.show_job(id);
                }
                self.flash(&key);
            }
            JumpTarget::FilterCard => {
                if let Some(dock) = self.dock.upgrade() {
                    dock.show_card(PanelId::Export, "filters");
                }
            }
            JumpTarget::Sections | JumpTarget::SectionRow(_) => {
                if let Some(dock) = self.dock.upgrade() {
                    dock.show_card(PanelId::Export, "sections");
                }
                self.flash(&key);
            }
        }
    }
}
