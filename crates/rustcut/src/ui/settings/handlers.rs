//! What the Settings window's callbacks do: generic options, file dialogs and buttons, the target folders, the render
//! profiles and the keymap. Every change goes through the settings writer.

use std::rc::Weak;

use rustcut_core::render::{DEFAULT_PROFILE_ID, LibraryError};
use rustcut_core::settings::Settings;
use slint::ComponentHandle as _;

use super::field::{Edit, Kind, Value};
use super::hotkeys::{self, Recorded};
use super::page::Page;
use super::{SettingsBridge, catalog, folders, hotkey_view};
use crate::ui::color_picker::{self, PickerRequest};
use crate::ui::keymap::{Action, Chord, KeyPress, Keymap};
use crate::ui::notices::{self, Area, Notice, TYPING};
use crate::ui::render_settings::state::Row;
use crate::util::model::{sync, sync_with};
use crate::{ProfileItemView, RenderIssueView, SettingsUi};

impl SettingsBridge {
    pub(super) fn select_page(&self, index: Option<usize>) {
        if let Some(page) = index.and_then(Page::from_index) {
            self.hotkeys.borrow_mut().cancel();
            self.page.set(page);
            self.publish();
            notices::layout_changed();
        }
    }

    /// Every generic option of the page back to its default (the held button in the header).
    pub(super) fn reset_page(&self) {
        let page = self.page.get();
        if page == Page::Hotkeys {
            self.reset_hotkeys();
            return;
        }
        let cards = catalog::cards(page);
        self.update(|s| {
            for field in cards.iter().flat_map(|c| c.fields) {
                field.reset(s);
            }
        });
        if page == Page::Messages {
            self.reset_kinds();
        }
        let refused: Vec<String> = std::mem::take(&mut *self.errors.borrow_mut()).into_keys().collect();
        for key in refused {
            notices::clear(&bubble_key(&key));
        }
        tracing::info!(page = page.id(), "[Settings] Page reset to its defaults");
        self.publish();
    }

    /// A click, choice, typed text or wheel step on a generic option.
    pub(super) fn edit(&self, key: &str, edit: Edit) -> Result<(), String> {
        let Some(field) = catalog::field(key) else {
            return Ok(());
        };
        let typed = matches!(edit, Edit::Type(_));
        let mut result = Ok(false);
        self.update(|s| result = field.apply(s, edit));
        match &result {
            Ok(changed) => {
                if self.errors.borrow_mut().remove(key).is_some() {
                    notices::clear(&bubble_key(key));
                }
                if *changed {
                    tracing::info!(option = key, "[Settings] Changed");
                }
            }
            Err(reason) => {
                self.errors.borrow_mut().insert(key.to_owned(), reason.clone());
                if typed {
                    post_refused(key, reason, std::time::Duration::ZERO);
                }
            }
        }
        self.publish();
        result.map(|_| ())
    }

    /// The field of `key` with a refused text was typed in: checked again after the typing grace time (M6.5c).
    pub(super) fn text_typed(&self, key: &str, text: &str) {
        if !self.errors.borrow().contains_key(key) {
            return;
        }
        let Some(field) = catalog::field(key) else { return };
        let mut copy = self.writer.current();
        match field.apply(&mut copy, Edit::Type(text.to_owned())) {
            Ok(_) => {
                self.errors.borrow_mut().remove(key);
                notices::clear_soon(&bubble_key(key), TYPING);
            }
            Err(reason) => {
                post_refused(key, &reason, TYPING);
                self.errors.borrow_mut().insert(key.to_owned(), reason);
            }
        }
    }

    /// The field of `key` closed without keeping a value: its refused text is gone.
    pub(super) fn text_canceled(&self, key: &str) {
        if self.errors.borrow_mut().remove(key).is_some() {
            notices::clear(&bubble_key(key));
            self.publish();
        }
    }

    /// The browse button of a path option: the portal's dialog, then the path as if typed.
    pub(super) fn pick(&self, key: &str) {
        let Some(field) = catalog::field(key) else { return };
        let Kind::Path { folder, .. } = field.kind else { return };
        let mut dialog = rfd::AsyncFileDialog::new().set_title(format!("Choose: {}", field.label));
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = pick_path(dialog, folder);
        let weak = Weak::clone(&self.me);
        let key = key.to_owned();
        let spawned = slint::spawn_local(async move {
            let Some(path) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                let _ = bridge.edit(&key, Edit::Type(path.display().to_string()));
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no file dialog");
        }
    }

    /// A color option: the color picker over the Settings window; every change in it is applied at once, Cancel puts
    /// the color back (P8).
    pub(super) fn pick_color(&self, key: &str) {
        let Some(field) = catalog::field(key) else { return };
        if !matches!(field.kind, Kind::Color) {
            return;
        }
        let settings = self.writer.current();
        let text = |s: &Settings| match (field.get)(s) {
            Value::Text(t) => t,
            _ => String::new(),
        };
        let Some(initial) = color_picker::color_of(&text(&settings)) else {
            return;
        };
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let request = PickerRequest {
            title: format!("{} - RustCut", field.label),
            initial,
            default: color_picker::color_of(&text(&Settings::default())),
            show_alpha: false,
            tooltips: (
                settings.appearance.tooltips,
                i64::from(settings.appearance.tooltip_delay_ms),
            ),
        };
        let weak = Weak::clone(&self.me);
        let key = key.to_owned();
        // The picker is modal for the Settings window (D-P9h, review 69): dimmed until it ends.
        let dimmed = window.as_weak();
        window.global::<SettingsUi<'_>>().set_picker_open(true);
        color_picker::open(
            window,
            &request,
            move |color| {
                if let Some(bridge) = weak.upgrade() {
                    let _ = bridge.edit(&key, Edit::Type(color_picker::hex_of(color)));
                }
            },
            move || {
                if let Some(window) = dimmed.upgrade() {
                    window.global::<SettingsUi<'_>>().set_picker_open(false);
                }
            },
        );
    }

    /// A button option.
    pub(super) fn action(&self, key: &str) {
        match key {
            // Options that moved to the Messages page (M6.5b): their old places lead there.
            "page.messages" | "page.messages.desktop" => {
                self.state.borrow_mut().messages_tab = u8::from(key == "page.messages");
                self.select_page(Some(Page::Messages.index()));
            }
            "open.logs" => self.open_folder("logs"),
            "open.config" => self.open_folder("config"),
            "open.data" => self.open_folder("data"),
            "open.cache" => self.open_folder("cache"),
            "rustview.test" => {
                if let Some(probe) = &self.hooks.borrow().probe_rustview {
                    probe();
                }
            }
            _ => tracing::debug!(key, "[Settings] Unknown button"),
        }
    }

    // --- Target folders -----------------------------------------------------------------------------------------

    pub(super) fn add_folder(&self) {
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Add a Target Folder");
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = dialog.pick_folder();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(folder) = picked.await else { return };
            let Some(bridge) = weak.upgrade() else { return };
            let path = folder.path().to_path_buf();
            bridge.update(|s| {
                if folders::add(&mut s.export.target_folders, &path) {
                    tracing::info!(folder = %path.display(), "[Settings] Target folder added");
                }
            });
            bridge.publish();
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no folder dialog");
        }
    }

    pub(super) fn remove_folder(&self, index: usize) {
        self.update(|s| {
            if let Some(folder) = folders::remove(&mut s.export.target_folders, index) {
                tracing::info!(folder = %folder.path.display(), "[Settings] Target folder removed from the list");
            }
        });
        self.publish();
    }

    pub(super) fn move_folder(&self, index: usize, by: i32) {
        self.update(|s| {
            folders::shift(&mut s.export.target_folders, index, by);
        });
        self.publish();
    }

    pub(super) fn rename_folder(&self, index: usize, label: &str) -> bool {
        let mut ok = true;
        self.update(|s| ok = folders::rename(&mut s.export.target_folders, index, label).is_ok());
        self.publish();
        ok
    }

    // --- Render profiles ----------------------------------------------------------------------------------------

    pub(super) fn select_profile(&self, index: usize) {
        if self.profiles.borrow_mut().select(index) {
            self.publish();
        }
    }

    pub(super) fn duplicate_profile(&self) {
        let result = self.profiles.borrow_mut().duplicate();
        self.profiles_result(result.map(|id| format!("Profile {id} added")));
    }

    pub(super) fn rename_profile(&self, name: &str) -> bool {
        let result = self.profiles.borrow_mut().rename(name);
        let ok = result.is_ok();
        self.profiles_result(result.map(|_| "Profile renamed".to_owned()));
        ok
    }

    pub(super) fn delete_profile(&self) {
        let default = self.writer.current().render.default_profile;
        let result = self.profiles.borrow_mut().delete(&default);
        if let Ok(deleted) = &result
            && deleted.id == default
        {
            self.update(|s| DEFAULT_PROFILE_ID.clone_into(&mut s.render.default_profile));
        }
        self.profiles_result(result.map(|p| format!("Profile \"{}\" deleted", p.name)));
    }

    pub(super) fn default_profile(&self) {
        let id = self.profiles.borrow().selected_id().to_owned();
        self.update(|s| s.render.default_profile = id);
        self.publish();
    }

    /// Saves the user's profiles after a change of the list, or reports why it was refused.
    fn profiles_result(&self, result: Result<String, LibraryError>) {
        match result {
            Ok(message) => {
                tracing::info!("[Settings] {message}");
                self.save_profiles();
            }
            Err(e) => tracing::info!(error = %e, "[Settings] Profile change refused"),
        }
        self.publish();
    }

    fn save_profiles(&self) {
        let user = self.profiles.borrow().state.library.user().to_vec();
        self.update(|s| s.render.profiles = user);
    }

    /// After an edit of the shown profile's settings: an own profile keeps them at once.
    fn profile_edited(&self, changed: bool) {
        if changed && self.profiles.borrow_mut().edited() {
            self.save_profiles();
        }
        self.publish();
    }

    pub(super) fn profile_choose(&self, key: &str, index: i32) {
        let Some(row) = Row::from_key(key) else { return };
        let changed = usize::try_from(index).is_ok_and(|i| self.profiles.borrow_mut().state.choose(row, i));
        self.profile_edited(changed);
    }

    pub(super) fn profile_slide(&self, _key: &str, position: i32) {
        let changed = u16::try_from(position).is_ok_and(|p| self.profiles.borrow_mut().state.slide(p));
        self.profile_edited(changed);
    }

    pub(super) fn profile_toggle(&self, key: &str) {
        let Some(row) = Row::from_key(key) else { return };
        let changed = self.profiles.borrow_mut().state.toggle(row);
        self.profile_edited(changed);
    }

    pub(super) fn profile_type(&self, key: &str, text: &str) -> bool {
        let Some(row) = Row::from_key(key) else { return true };
        let result = self.profiles.borrow_mut().state.type_text(row, text);
        self.profile_edited(result.unwrap_or(false));
        result.is_ok()
    }

    pub(super) fn profile_step(&self, key: &str, steps: i32) {
        let Some(row) = Row::from_key(key) else { return };
        let changed = self.profiles.borrow_mut().state.step(row, steps);
        self.profile_edited(changed);
    }

    pub(super) fn publish_profiles(&self, ui: &SettingsUi<'_>, settings: &Settings) {
        let page = self.profiles.borrow();
        let items = page
            .items(&settings.render.default_profile)
            .into_iter()
            .map(|i| ProfileItemView {
                name: i.name.into(),
                summary: i.summary.into(),
                builtin: i.builtin,
                default: i.default,
            })
            .collect();
        sync(&self.models.profiles, items);
        ui.set_profile_index(i32::try_from(page.index()).unwrap_or(0));
        ui.set_profile_builtin(page.is_builtin());
        ui.set_profile_name(page.state.selection.working.name.as_str().into());
        let panel = page.view();
        sync(
            &self.models.issues,
            panel
                .issues
                .iter()
                .map(|i| RenderIssueView {
                    error: i.error,
                    text: i.text.as_str().into(),
                })
                .collect(),
        );
        ui.set_encoders_note(panel.encoders_note.as_str().into());
        sync(
            &self.models.video,
            panel.video.iter().map(crate::ui::render_settings::option_row).collect(),
        );
        sync(
            &self.models.audio,
            panel.audio.iter().map(crate::ui::render_settings::option_row).collect(),
        );
        sync(
            &self.models.expert,
            panel
                .expert
                .iter()
                .map(crate::ui::render_settings::option_row)
                .collect(),
        );
    }

    // --- Hotkeys ------------------------------------------------------------------------------------------------

    pub(super) fn hotkey_search(&self, text: &str) {
        text.clone_into(&mut self.hotkeys.borrow_mut().search);
        self.publish();
    }

    pub(super) fn record(&self, id: &str) {
        if let Some(action) = Action::from_id(id) {
            self.hotkeys.borrow_mut().start(action);
            *self.hotkey_note.borrow_mut() = (String::new(), false);
            self.publish();
        }
    }

    pub(super) fn cancel_recording(&self) {
        self.hotkeys.borrow_mut().cancel();
        self.publish();
    }

    /// A key press while an action waits for its key; `true` = taken (always, so the press does nothing else).
    pub(super) fn recorded(&self, text: &str, [control, alt, shift, meta]: [bool; 4]) -> bool {
        let press = KeyPress {
            text,
            control,
            alt,
            shift,
            meta,
            repeat: false,
        };
        let outcome = {
            let mut keymap = self.keymap.borrow_mut();
            self.hotkeys.borrow_mut().press(&mut keymap, &press)
        };
        let label = |text: &str| Chord::parse(text).map_or_else(|_| text.to_owned(), |c| c.label());
        let note = match &outcome {
            Recorded::Waiting => return true,
            Recorded::Bound(_) => (String::new(), false),
            Recorded::Taken { chord, other } => (
                format!(
                    "{} is used by \"{}\". Press it again to move it here, Esc keeps it there.",
                    label(chord),
                    other.title()
                ),
                false,
            ),
            Recorded::Moved { chord, from } => (format!("{} moved from \"{}\".", label(chord), from.title()), false),
        };
        *self.hotkey_note.borrow_mut() = note;
        if matches!(outcome, Recorded::Bound(_) | Recorded::Moved { .. }) {
            tracing::info!("[Settings] Key bound");
            self.save_keymap();
        }
        self.publish();
        true
    }

    pub(super) fn unbind(&self, id: &str, key: &str) {
        if let Some(action) = Action::from_id(id)
            && self.keymap.borrow_mut().unbind(action, key)
        {
            self.save_keymap();
        }
        self.publish();
    }

    pub(super) fn reset_hotkey(&self, id: &str) {
        let Some(action) = Action::from_id(id) else { return };
        let missing = hotkeys::reset(&mut self.keymap.borrow_mut(), action);
        *self.hotkey_note.borrow_mut() = if missing.is_empty() {
            (String::new(), false)
        } else {
            (
                format!(
                    "{} stays with another action; remove it there first.",
                    missing.join(", ")
                ),
                true,
            )
        };
        self.save_keymap();
        self.publish();
    }

    pub(super) fn reset_hotkeys(&self) {
        *self.keymap.borrow_mut() = Keymap::default();
        *self.hotkey_note.borrow_mut() = (String::new(), false);
        self.hotkeys.borrow_mut().cancel();
        tracing::info!("[Settings] Every key back to its default");
        self.save_keymap();
        self.publish();
    }

    fn save_keymap(&self) {
        let table = hotkeys::table(&self.keymap.borrow());
        self.update(|s| s.keymap = table);
    }

    pub(super) fn publish_hotkeys(&self, ui: &SettingsUi<'_>) {
        let page = self.hotkeys.borrow();
        let rows = hotkeys::rows(&self.keymap.borrow(), &page.search);
        let mut shown = self.hotkeys_shown.borrow_mut();
        sync_with(&self.models.hotkeys, &mut shown, &rows, hotkey_view);
        ui.set_recording(page.recording.map_or("", Action::id).into());
        let note = self.hotkey_note.borrow();
        ui.set_hotkey_note(note.0.as_str().into());
        ui.set_hotkey_note_error(note.1);
    }
}

/// The portal's file or folder dialog as a future of the picked path.
fn pick_path(
    dialog: rfd::AsyncFileDialog,
    folder: bool,
) -> std::pin::Pin<Box<dyn Future<Output = Option<std::path::PathBuf>>>> {
    if folder {
        let picked = dialog.pick_folder();
        Box::pin(async move { picked.await.map(|f| f.path().to_path_buf()) })
    } else {
        let picked = dialog.pick_file();
        Box::pin(async move { picked.await.map(|f| f.path().to_path_buf()) })
    }
}

/// The bubble of a refused text of the option `key` (at its field, `settings_row.slint`).
pub(super) fn bubble_key(key: &str) -> String {
    format!("settings-value:{key}")
}

pub(super) fn post_refused(key: &str, reason: &str, grace: std::time::Duration) {
    notices::post(
        Notice::new(notices::Kind::ValueInvalid, reason)
            .key(bubble_key(key))
            .at(Area::Settings, [format!("settings:{key}")])
            .grace(grace),
    );
}
