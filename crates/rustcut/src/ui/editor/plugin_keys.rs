//! The editor's side of plugin commands (plugin API 1.2, Q-NF1): keys the keymap does not know go to the plugin
//! commands, key releases end a held key, and the sections tell whether they are saved as a job.

use std::cell::RefCell;

use rustcut_core::edit::SectionList;

use super::{Editor, State};
use crate::ui::keymap::KeyPress;

/// A key the keymap does not know: `true` when a plugin command took it.
pub type KeyHook = Box<dyn Fn(&KeyPress<'_>) -> bool>;
/// A key was released (its text).
pub type ReleaseHook = Box<dyn Fn(&str)>;
/// Whether the sections are unsaved now (told whenever the sections change).
pub type UnsavedHook = Box<dyn Fn(bool)>;

#[derive(Default)]
pub(super) struct Extras {
    keys: RefCell<Option<KeyHook>>,
    release: RefCell<Option<ReleaseHook>>,
    unsaved: RefCell<Option<UnsavedHook>>,
    /// The section list of the last saved job of this video (`None`: nothing saved since it opened).
    saved: RefCell<Option<SectionList>>,
}

impl Editor {
    /// Plugin commands get the keys the keymap does not know, and every key release.
    pub fn set_key_hooks(&self, keys: KeyHook, release: ReleaseHook) {
        *self.extras.keys.borrow_mut() = Some(keys);
        *self.extras.release.borrow_mut() = Some(release);
    }

    /// `hook` hears whether the sections are unsaved, at once and after every change.
    pub fn set_unsaved_hook(&self, hook: UnsavedHook) {
        hook(self.has_unsaved_sections());
        *self.extras.unsaved.borrow_mut() = Some(hook);
    }

    /// The list holds sections that are not saved as a job yet (a save that keeps the list counts as saved).
    pub fn has_unsaved_sections(&self) -> bool {
        unsaved(&self.state.borrow(), self.extras.saved.borrow().as_ref())
    }

    /// A job was saved from the current sections.
    pub fn sections_saved(&self) {
        let list = self.state.borrow().sections.list().clone();
        *self.extras.saved.borrow_mut() = Some(list);
        self.tell_unsaved();
    }

    /// Another video opened: nothing of it is saved yet.
    pub(super) fn forget_saved_sections(&self) {
        self.extras.saved.borrow_mut().take();
    }

    pub(super) fn tell_unsaved_with(&self, st: &State) {
        if let Some(hook) = self.extras.unsaved.borrow().as_ref() {
            hook(unsaved(st, self.extras.saved.borrow().as_ref()));
        }
    }

    fn tell_unsaved(&self) {
        let st = self.state.borrow();
        self.tell_unsaved_with(&st);
    }

    /// A modal dialog of the main window is open (`modal-open` in `main.slint`; also RustSearch's `status`).
    pub fn modal_open(&self) -> bool {
        self.menu_modal.get()
    }

    /// A key the keymap does not know (not behind a modal dialog).
    pub(super) fn plugin_key(&self, press: &KeyPress<'_>) -> bool {
        !self.menu_modal.get() && self.extras.keys.borrow().as_ref().is_some_and(|hook| hook(press))
    }

    pub(super) fn key_released(&self, text: &str) {
        if let Some(hook) = self.extras.release.borrow().as_ref() {
            hook(text);
        }
    }
}

fn unsaved(st: &State, saved: Option<&SectionList>) -> bool {
    let list = st.sections.list();
    !list.is_empty() && saved != Some(list)
}
