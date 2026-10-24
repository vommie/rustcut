//! Connects the Settings window's callbacks to the bridge (`handlers.rs` does the work).

use std::rc::Weak;

use slint::ComponentHandle as _;

use super::SettingsBridge;
use super::field::Edit;
use crate::{SettingsUi, SettingsWindow};

/// Calls `f` with the bridge while it lives.
fn with<R: Default>(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge) -> R) -> R {
    this.upgrade().map(|b| f(&b)).unwrap_or_default()
}

fn index(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    let ui = window.global::<SettingsUi<'_>>();
    let t = this.clone();
    ui.on_select_page(move |i| with(&t, |b| b.select_page(index(i))));
    let t = this.clone();
    ui.on_reset_page(move || with(&t, SettingsBridge::reset_page));
    let t = this.clone();
    ui.on_close(move || with(&t, SettingsBridge::close));

    let t = this.clone();
    ui.on_toggle(move |key| {
        with(&t, |b| b.edit(&key, Edit::Toggle).is_ok());
    });
    let t = this.clone();
    ui.on_choose(move |key, i| {
        if let Some(i) = index(i) {
            with(&t, |b| b.edit(&key, Edit::Choose(i)).is_ok());
        }
    });
    let t = this.clone();
    ui.on_type_text(move |key, text| with(&t, |b| b.edit(&key, Edit::Type(text.into())).is_ok()));
    let t = this.clone();
    ui.on_step(move |key, steps| {
        with(&t, |b| b.edit(&key, Edit::Step(steps)).is_ok());
    });
    let t = this.clone();
    ui.on_pick(move |key| with(&t, |b| b.pick(key.as_str())));
    let t = this.clone();
    ui.on_action(move |key| with(&t, |b| b.action(key.as_str())));

    let t = this.clone();
    ui.on_add_folder(move || with(&t, SettingsBridge::add_folder));
    let t = this.clone();
    ui.on_remove_folder(move |i| with(&t, |b| index(i).map(|i| b.remove_folder(i)).unwrap_or_default()));
    let t = this.clone();
    ui.on_move_folder(move |i, by| with(&t, |b| index(i).map(|i| b.move_folder(i, by)).unwrap_or_default()));
    let t = this.clone();
    ui.on_rename_folder(move |i, label| with(&t, |b| index(i).is_some_and(|i| b.rename_folder(i, &label))));

    let t = this.clone();
    ui.on_select_profile(move |i| with(&t, |b| index(i).map(|i| b.select_profile(i)).unwrap_or_default()));
    let t = this.clone();
    ui.on_duplicate_profile(move || with(&t, SettingsBridge::duplicate_profile));
    let t = this.clone();
    ui.on_rename_profile(move |name| with(&t, |b| b.rename_profile(&name)));
    let t = this.clone();
    ui.on_delete_profile(move || with(&t, SettingsBridge::delete_profile));
    let t = this.clone();
    ui.on_default_profile(move || with(&t, SettingsBridge::default_profile));
    let t = this.clone();
    ui.on_profile_choose(move |key, i| {
        with(&t, |b| b.profile_choose(&key, i));
    });
    let t = this.clone();
    ui.on_profile_slide(move |key, position| {
        with(&t, |b| b.profile_slide(&key, position));
    });
    let t = this.clone();
    ui.on_profile_toggle(move |key| {
        with(&t, |b| b.profile_toggle(&key));
    });
    let t = this.clone();
    ui.on_profile_type(move |key, text| with(&t, |b| b.profile_type(&key, &text)));
    let t = this.clone();
    ui.on_profile_step(move |key, steps| {
        with(&t, |b| b.profile_step(&key, steps));
    });

    let t = this.clone();
    ui.on_hotkey_search_edited(move |text| with(&t, |b| b.hotkey_search(&text)));
    let t = this.clone();
    ui.on_record(move |id| with(&t, |b| b.record(&id)));
    let t = this.clone();
    ui.on_recorded(move |text, control, alt, shift, meta| {
        with(&t, |b| b.recorded(&text, [control, alt, shift, meta]))
    });
    let t = this.clone();
    ui.on_cancel_recording(move || with(&t, SettingsBridge::cancel_recording));
    let t = this.clone();
    ui.on_unbind(move |id, key| with(&t, |b| b.unbind(&id, &key)));
    let t = this.clone();
    ui.on_reset_hotkey(move |id| with(&t, |b| b.reset_hotkey(&id)));
    let t = this.clone();
    ui.on_reset_hotkeys(move || with(&t, SettingsBridge::reset_hotkeys));
}
