//! The callbacks of the `RenderSettings` global: profile selection and management, and the edits of the option rows.

use std::rc::Rc;

use rustcut_core::render::LibraryError;
use slint::ComponentHandle as _;

use super::RenderSettingsBridge;
use super::state::Row;
use crate::ui::status::Level;
use crate::{MainWindow, RenderSettings};

pub(super) fn connect(bridge: &Rc<RenderSettingsBridge>, window: &MainWindow) {
    let rs = window.global::<RenderSettings<'_>>();
    let this = Rc::downgrade(bridge);
    rs.on_select_profile(move |index| {
        if let Some(b) = this.upgrade() {
            b.select_profile(index);
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_reset_profile(move || {
        if let Some(b) = this.upgrade() {
            b.reset_profile();
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_save_profile(move || {
        if let Some(b) = this.upgrade() {
            b.save_profile();
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_save_profile_as(move |name| this.upgrade().is_some_and(|b| b.save_profile_as(&name)));
    let this = Rc::downgrade(bridge);
    rs.on_rename_profile(move |name| this.upgrade().is_some_and(|b| b.rename_profile(&name)));
    let this = Rc::downgrade(bridge);
    rs.on_delete_profile(move || {
        if let Some(b) = this.upgrade() {
            b.delete_profile();
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_choose(move |key, index| {
        if let (Some(b), Some(row), Ok(index)) = (this.upgrade(), Row::from_key(&key), usize::try_from(index)) {
            let changed = b.state.borrow_mut().choose(row, index);
            b.edited(changed);
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_slide(move |_key, position| {
        if let (Some(b), Ok(position)) = (this.upgrade(), u16::try_from(position)) {
            let changed = b.state.borrow_mut().slide(position);
            b.edited(changed);
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_toggle(move |key| {
        if let (Some(b), Some(row)) = (this.upgrade(), Row::from_key(&key)) {
            let changed = b.state.borrow_mut().toggle(row);
            b.edited(changed);
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_type_text(move |key, text| {
        let (Some(b), Some(row)) = (this.upgrade(), Row::from_key(&key)) else {
            return false;
        };
        let result = b.state.borrow_mut().type_text(row, &text);
        match result {
            Ok(changed) => {
                b.edited(changed);
                true
            }
            Err(_not_a_value) => false,
        }
    });
    let this = Rc::downgrade(bridge);
    rs.on_step(move |key, steps| {
        if let (Some(b), Some(row)) = (this.upgrade(), Row::from_key(&key)) {
            let changed = b.state.borrow_mut().step(row, steps);
            b.edited(changed);
        }
    });
}

impl RenderSettingsBridge {
    fn edited(&self, changed: bool) {
        if changed {
            self.changed(false);
        }
    }

    fn select_profile(&self, index: i32) {
        let selected = {
            let mut st = self.state.borrow_mut();
            let id = usize::try_from(index)
                .ok()
                .and_then(|i| st.library.all().nth(i))
                .map(|p| p.id.clone());
            let state = &mut *st;
            id.is_some_and(|id| state.selection.select(&state.library, &id))
        };
        if selected {
            self.changed(false);
        }
    }

    fn reset_profile(&self) {
        {
            let mut st = self.state.borrow_mut();
            let state = &mut *st;
            state.selection.reset(&state.library);
        }
        self.changed(false);
    }

    fn save_profile(&self) {
        let result = {
            let mut st = self.state.borrow_mut();
            let state = &mut *st;
            let base = state.selection.base.clone();
            state.library.overwrite(&base, &state.selection.working)
        };
        self.library_result(result.map(|()| "Profile saved".to_owned()));
    }

    fn save_profile_as(&self, name: &str) -> bool {
        let result = {
            let mut st = self.state.borrow_mut();
            let state = &mut *st;
            let saved = state.library.save_as(name, &state.selection.working);
            if let Ok(id) = &saved {
                state.selection.select(&state.library, id);
            }
            saved
        };
        let ok = result.is_ok();
        self.library_result(result.map(|_| format!("Profile \"{}\" saved", name.trim())));
        ok
    }

    fn rename_profile(&self, name: &str) -> bool {
        let result = {
            let mut st = self.state.borrow_mut();
            let state = &mut *st;
            let base = state.selection.base.clone();
            let renamed = state.library.rename(&base, name);
            state.selection.follow(&state.library);
            renamed
        };
        let ok = result.is_ok();
        self.library_result(result.map(|()| "Profile renamed".to_owned()));
        ok
    }

    fn delete_profile(&self) {
        let result = {
            let mut st = self.state.borrow_mut();
            let state = &mut *st;
            let base = state.selection.base.clone();
            state.library.delete(&base)
        };
        self.library_result(result.map(|p| format!("Profile \"{}\" deleted", p.name)));
    }

    /// Shows the outcome of a change of the user's profiles and saves them when it worked.
    fn library_result(&self, result: Result<String, LibraryError>) {
        match result {
            Ok(message) => {
                self.status(Level::Info, &message);
                self.changed(true);
            }
            Err(e) => {
                self.status(Level::Warning, &format!("{}.", capitalized(&e.to_string())));
                self.publish();
            }
        }
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
