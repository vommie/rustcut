//! What the Target Folders page does (P11.3): selecting, folding, renaming, adding (dialog or dropped from the file
//! manager), deleting (held), stars and wheel places, pictures (built-in, own file through the picture store), colors
//! and dragging. Edits that cannot be done say why in a `folder_edit` message.

use std::path::PathBuf;
use std::rc::Weak;

use rustcut_core::folders::pictures::PictureCommand;
use rustcut_core::folders::{EditError, FolderId, LookKind, Spot};
use slint::ComponentHandle as _;
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor as _};

use super::details;
use super::rows::drop_target;
use super::{Drag, ROW_HEIGHT, id_of};
use crate::ui::color_picker::{self, PickerRequest};
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::settings::SettingsBridge;
use crate::{FolderTreeUi, SettingsUi, SettingsWindow};

/// The name a new group starts with (it is renamed at once).
const NEW_GROUP: &str = "New group";
/// The color the picker starts with when the entry has none.
const START_COLOR: &str = "#89b4fa";

fn tell(title: &str, detail: &str) {
    tracing::info!(message = title, detail, "[Settings]");
    notices::post(Notice::new(Kind::FolderEdit, title).detail(detail));
}

impl SettingsBridge {
    fn folders_changed(&self) {
        self.publish_folder_tree(&self.writer.current());
    }

    /// Where a new entry goes: into the selected group (unfolded), after the selected entry, else at the end.
    fn new_spot(&self) -> Spot {
        let settings = self.writer.current();
        match self.folder_page.selected.get().filter(|id| !id.is_source()) {
            Some(id)
                if settings
                    .export
                    .folders
                    .find(id)
                    .is_some_and(rustcut_core::folders::FolderNode::is_group) =>
            {
                self.folder_page.collapsed.borrow_mut().remove(&id);
                Spot::Into(id)
            }
            Some(id) if settings.export.folders.find(id).is_some() => Spot::After(id),
            _ => Spot::End,
        }
    }

    fn folder_select(&self, id: Option<FolderId>) {
        let page = &self.folder_page;
        if page.selected.get() != id {
            page.renaming.set(None);
        }
        page.selected.set(id);
        self.folders_changed();
    }

    /// Selects an entry and scrolls nothing: the tree's row and the details follow.
    pub(super) fn folder_select_entry(&self, id: FolderId) {
        self.folder_select(Some(id));
    }

    fn folder_toggle(&self, id: FolderId) {
        {
            let mut collapsed = self.folder_page.collapsed.borrow_mut();
            if !collapsed.remove(&id) {
                collapsed.insert(id);
            }
        }
        self.folders_changed();
    }

    fn folder_rename_start(&self, id: FolderId) {
        if id.is_source() {
            return;
        }
        self.folder_page.selected.set(Some(id));
        self.folder_page.renaming.set(Some(id));
        self.folders_changed();
    }

    /// `false` = refused (an empty name): the field stays open.
    fn folder_rename(&self, id: FolderId, name: &str) -> bool {
        let mut result = Ok(false);
        self.update(|s| result = s.export.folders.rename(id, name));
        if result == Err(EditError::EmptyName) {
            return false;
        }
        if result == Ok(true) {
            tracing::info!(id = id.0, "[Settings] Target folder entry renamed");
        }
        self.folder_page.renaming.set(None);
        self.folders_changed();
        true
    }

    /// The wheel's short name of the selected entry (empty: the name again).
    fn folder_set_short_name(&self, text: &str) -> bool {
        let Some(id) = self.folder_page.selected.get() else {
            return true;
        };
        let mut result = Ok(false);
        self.update(|s| result = s.export.folders.set_short_name(id, text));
        if result == Ok(true) {
            tracing::info!(id = id.0, "[Settings] Short name of a target folder entry changed");
        }
        self.folders_changed();
        true
    }

    fn folder_rename_end(&self) {
        self.folder_page.renaming.set(None);
        self.folders_changed();
    }

    fn folder_star(&self, id: FolderId) {
        let settings = self.writer.current();
        let on = settings.export.folders.entry(id).is_some_and(|e| e.place().is_none());
        let places = settings.folder_wheel.places;
        let mut result = Ok(false);
        self.update(|s| result = s.export.folders.set_favorite(id, on, places));
        if result == Err(EditError::NoFreePlace) {
            tell(
                "Every place on the wheel is taken",
                "Select the entry and click a place in its details: the favorite there loses its star.",
            );
        }
        self.folder_page.selected.set(Some(id));
        self.folders_changed();
    }

    pub(super) fn folder_put_on_place(&self, place: i32) {
        let (Some(id), Ok(place)) = (self.folder_page.selected.get(), u8::try_from(place)) else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.put_on_place(id, place);
        });
        self.folders_changed();
    }

    /// The "x" badge of a place on the small wheel: its favorite loses the star.
    fn folder_clear_place(&self, place: i32) {
        let Ok(place) = u8::try_from(place) else { return };
        let settings = self.writer.current();
        let Some(id) = settings
            .export
            .folders
            .favorites()
            .into_iter()
            .find(|(_, p)| *p == place)
            .map(|(id, _)| id)
        else {
            return;
        };
        let places = settings.folder_wheel.places;
        self.update(|s| {
            let _ = s.export.folders.set_favorite(id, false, places);
        });
        tracing::info!(id = id.0, place, "[Settings] Wheel place freed");
        self.folders_changed();
    }

    /// Adds the folders at `paths` (the dialog's or dropped ones) where new entries go; the last one is selected.
    fn folder_add_paths(&self, paths: Vec<PathBuf>) {
        let spot = self.new_spot();
        let mut added = None;
        let mut known = Vec::new();
        self.update(|s| {
            let mut at = spot;
            for path in &paths {
                match s.export.folders.add_folder(path, at) {
                    Ok(id) => {
                        added = Some(id);
                        at = Spot::After(id);
                    }
                    Err(_) => known.push(path.clone()),
                }
            }
        });
        if let Some(id) = added {
            tracing::info!(count = paths.len() - known.len(), "[Settings] Target folders added");
            self.folder_page.selected.set(Some(id));
        }
        if let Some(path) = known.first() {
            tell("This folder is in the list already", &path.display().to_string());
        }
        self.folders_changed();
    }

    fn folder_add(&self) {
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Add a Target Folder");
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = dialog.pick_folder();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(folder) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                bridge.folder_add_paths(vec![folder.path().to_path_buf()]);
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no folder dialog");
        }
    }

    fn folder_add_group(&self) {
        let spot = self.new_spot();
        let mut added = None;
        self.update(|s| added = s.export.folders.add_group(NEW_GROUP, spot).ok());
        if let Some(id) = added {
            tracing::info!(id = id.0, "[Settings] Target folder group added");
            self.folder_page.selected.set(Some(id));
            self.folder_page.renaming.set(Some(id));
        }
        self.folders_changed();
    }

    /// The held delete button: the selected entry goes (a group with everything in it); the next row is selected.
    fn folder_delete(&self) {
        let Some(id) = self.folder_page.selected.get().filter(|id| !id.is_source()) else {
            return;
        };
        let next = {
            let shown = self.folder_page.shown.borrow();
            let tree = self.writer.current().export.folders;
            let index = shown.iter().position(|r| r.id == id);
            // The next row that is not inside the deleted entry, else the one before it.
            index.and_then(|i| {
                shown
                    .iter()
                    .skip(i + 1)
                    .find(|r| !tree.groups_of(r.id).contains(&id))
                    .or_else(|| i.checked_sub(1).and_then(|p| shown.get(p)))
                    .map(|r| r.id)
            })
        };
        let mut removed = None;
        self.update(|s| removed = s.export.folders.remove(id).ok());
        if let Some(node) = removed {
            tracing::info!(name = %node.name, group = node.is_group(), "[Settings] Target folder entry deleted");
        }
        self.folder_page.selected.set(next);
        self.folders_changed();
    }

    /// A key on the tree: F2 renames, Up/Down move the selection, Left folds (or goes to the group), Right unfolds.
    fn folder_key(&self, text: &str) -> bool {
        let page = &self.folder_page;
        let selected = page.selected.get();
        let shown: Vec<(FolderId, bool, bool, usize)> = page
            .shown
            .borrow()
            .iter()
            .map(|r| (r.id, r.group, r.expanded, r.depth))
            .collect();
        let index = selected.and_then(|id| shown.iter().position(|r| r.0 == id));
        let f2 = slint::SharedString::from(slint::platform::Key::F2);
        let up = slint::SharedString::from(slint::platform::Key::UpArrow);
        let down = slint::SharedString::from(slint::platform::Key::DownArrow);
        let left = slint::SharedString::from(slint::platform::Key::LeftArrow);
        let right = slint::SharedString::from(slint::platform::Key::RightArrow);
        if text == f2.as_str() {
            if let Some(id) = selected {
                self.folder_rename_start(id);
            }
        } else if text == up.as_str() || text == down.as_str() {
            let next = match index {
                None => 0,
                Some(i) if text == up.as_str() => i.saturating_sub(1),
                Some(i) => (i + 1).min(shown.len().saturating_sub(1)),
            };
            self.folder_select(shown.get(next).map(|r| r.0));
        } else if text == left.as_str() {
            let Some((id, group, expanded, _)) = index.and_then(|i| shown.get(i)).copied() else {
                return true;
            };
            if group && expanded {
                self.folder_toggle(id);
            } else if let Some(parent) = self.writer.current().export.folders.groups_of(id).last() {
                self.folder_select(Some(*parent));
            }
        } else if text == right.as_str() {
            if let Some((id, true, false, _)) = index.and_then(|i| shown.get(i)).copied() {
                self.folder_toggle(id);
            }
        } else {
            return false;
        }
        true
    }

    /// The choice changes what shows and which part the details show; nothing kept is lost, and no dialog opens.
    fn folder_picture_kind(&self, kind: i32) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_picture_kind(id, details::kind_of(kind));
        });
        self.folders_changed();
    }

    fn folder_pick_icon(&self, name: &str) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_icon(id, name);
        });
        self.folders_changed();
    }

    /// The held X on the own picture: it is deleted, the choice stays and the letter shows.
    fn folder_delete_image(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_image(id, None);
        });
        tracing::info!(id = id.0, "[Settings] Own picture of a target folder entry deleted");
        self.folders_changed();
    }

    /// An own image or SVG: the file dialog, then the picture store prepares it (off the UI thread).
    fn folder_pick_file(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose a Picture").add_filter(
            "Images and SVGs",
            &[
                "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "ico", "qoi", "svg",
            ],
        );
        if let Some(window) = self.window.borrow().as_ref() {
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = dialog.pick_file();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(file) = picked.await else { return };
            if let Some(bridge) = weak.upgrade() {
                bridge.folder_import_for(id, file.path().to_path_buf());
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; no file dialog");
        }
    }

    /// The selected entry has "Own" chosen: a dropped file is its picture.
    fn folder_takes_picture(&self) -> bool {
        self.folder_page.selected.get().is_some_and(|id| {
            self.writer
                .current()
                .export
                .folders
                .entry(id)
                .is_some_and(|e| e.look().kind == LookKind::Own)
        })
    }

    fn folder_import(&self, path: PathBuf) {
        if let Some(id) = self.folder_page.selected.get() {
            self.folder_import_for(id, path);
        }
    }

    /// The picture store prepares the file off the UI thread; it becomes the entry's picture.
    fn folder_import_for(&self, id: FolderId, path: PathBuf) {
        let store = self.pictures.store().clone();
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let answer = store.request(|reply| PictureCommand::Import { path, reply }).await;
            let Some(bridge) = weak.upgrade() else { return };
            match answer {
                Ok(Ok(stored)) => {
                    bridge.update(|s| {
                        let _ = s.export.folders.set_image(id, Some(stored.hash.clone()));
                    });
                    bridge.folders_changed();
                }
                Ok(Err(e)) => tell("The picture was not taken", &e.to_string()),
                Err(e) => tell("The picture was not taken", &e.to_string()),
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; the picture is not taken");
        }
    }

    fn folder_search_icons(&self, text: &str) {
        text.clone_into(&mut self.folder_page.icon_search.borrow_mut());
        self.folders_changed();
    }

    fn folder_pick_color(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        let settings = self.writer.current();
        let current = settings
            .export
            .folders
            .entry(id)
            .and_then(|e| e.color().map(str::to_owned));
        let Some(initial) = color_picker::color_of(current.as_deref().unwrap_or(START_COLOR)) else {
            return;
        };
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let request = PickerRequest {
            title: "Folder Color - RustCut".to_owned(),
            initial,
            default: None,
            show_alpha: false,
            tooltips: (
                settings.appearance.tooltips,
                i64::from(settings.appearance.tooltip_delay_ms),
            ),
        };
        let weak = Weak::clone(&self.me);
        let dimmed = window.as_weak();
        window.global::<SettingsUi<'_>>().set_picker_open(true);
        color_picker::open(
            window,
            &request,
            move |color| {
                if let Some(bridge) = weak.upgrade() {
                    let hex = color_picker::hex_of(color);
                    bridge.update(|s| {
                        let _ = s.export.folders.set_color(id, Some(&hex));
                    });
                    bridge.folders_changed();
                }
            },
            move || {
                if let Some(window) = dimmed.upgrade() {
                    window.global::<SettingsUi<'_>>().set_picker_open(false);
                }
            },
        );
    }

    fn folder_clear_color(&self) {
        let Some(id) = self.folder_page.selected.get() else {
            return;
        };
        self.update(|s| {
            let _ = s.export.folders.set_color(id, None);
        });
        self.folders_changed();
    }

    // --- Dragging -----------------------------------------------------------------------------------------------

    fn folder_drag_start(&self, id: FolderId) {
        if id.is_source() {
            return;
        }
        let Some(window) = self.window.borrow().as_ref().map(slint::ComponentHandle::clone_strong) else {
            return;
        };
        let name = self
            .writer
            .current()
            .export
            .folders
            .find(id)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        *self.folder_page.drag.borrow_mut() = Some(Drag { id, target: None });
        // The details show the dragged entry, so its places on the small wheel are the drop targets beside the tree.
        self.folder_select(Some(id));
        let ui = window.global::<FolderTreeUi<'_>>();
        ui.set_drag_name(name.into());
        ui.set_drop_place(-1);
        ui.set_dragging(true);
        let me = Weak::clone(&self.me);
        // The Settings page scrolls near its edges (the details may stand below the tree in a narrow window).
        crate::ui::drag::begin(&window, "settings", move || {
            if let Some(bridge) = me.upgrade() {
                bridge.folder_drag_end(false);
            }
        });
    }

    /// The pointer moved (or the page scrolled under it): `inside` = over the list, at content y `y`.
    fn folder_drag_move(&self, y: f32, inside: bool) {
        let target = {
            let mut slot = self.folder_page.drag.borrow_mut();
            let Some(drag) = slot.as_mut() else { return };
            let tree = self.writer.current().export.folders;
            drag.target = inside
                .then(|| drop_target(&tree, &self.folder_page.shown.borrow(), drag.id, y, ROW_HEIGHT))
                .flatten();
            drag.target
        };
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<FolderTreeUi<'_>>();
        ui.set_drop_shown(target.is_some_and(|t| t.line.is_some()));
        if let Some((y, depth)) = target.and_then(|t| t.line) {
            ui.set_drop_y(y);
            ui.set_drop_depth(i32::try_from(depth).unwrap_or(0));
        }
        ui.set_drop_into(
            target
                .and_then(|t| t.into)
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(-1),
        );
    }

    /// The button was released (`drop`) or Escape pressed.
    fn folder_drag_end(&self, drop: bool) {
        let Some(drag) = self.folder_page.drag.borrow_mut().take() else {
            return;
        };
        let mut place = -1;
        if let Some(window) = self.window.borrow().as_ref() {
            crate::ui::drag::end(window);
            let ui = window.global::<FolderTreeUi<'_>>();
            place = ui.get_drop_place();
            ui.set_dragging(false);
            ui.set_drop_shown(false);
            ui.set_drop_into(-1);
            ui.set_drop_place(-1);
        }
        // Dropped on a place of the small wheel: the entry becomes the favorite there (Q-FP2).
        if let (true, Ok(place)) = (drop, u8::try_from(place)) {
            self.update(|s| {
                let _ = s.export.folders.put_on_place(drag.id, place);
            });
            tracing::info!(
                id = drag.id.0,
                place,
                "[Settings] Target folder entry put on a wheel place"
            );
            self.folders_changed();
            return;
        }
        let Some(target) = drag.target.filter(|_| drop) else {
            return;
        };
        let mut moved = Ok(false);
        self.update(|s| moved = s.export.folders.move_to(drag.id, target.spot));
        if moved == Ok(true) {
            tracing::info!(id = drag.id.0, "[Settings] Target folder entry moved");
            if let Spot::Into(group) = target.spot {
                self.folder_page.collapsed.borrow_mut().remove(&group);
            }
        }
        self.folder_page.selected.set(Some(drag.id));
        self.folders_changed();
    }

    fn folder_file_over(&self, on: bool) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.global::<FolderTreeUi<'_>>().set_file_over(on);
        }
    }

    /// Files dropped on the Settings window while the page shows: the folders among them join the tree (checked on
    /// the runtime); while "Own" is chosen a file is the entry's picture.
    fn folders_dropped(&self, paths: Vec<PathBuf>) {
        let task = self.runtime.spawn_blocking(move || {
            let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| p.is_dir());
            (folders, files)
        });
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let (Ok((folders, files)), Some(bridge)) = (task.await, weak.upgrade()) else {
                return;
            };
            if !folders.is_empty() {
                bridge.folder_add_paths(folders);
            } else if let (Some(file), true) = (files.first(), bridge.folder_takes_picture()) {
                bridge.folder_import(file.clone());
            } else {
                tell(
                    "Only folders can be dropped here",
                    "Files are not target folders. An image can be dropped while \"Own\" is chosen.",
                );
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Settings] The event loop is gone; the dropped folders are not added");
        }
    }

    fn folder_set_path(&self, text: &str) -> bool {
        self.folder_page
            .selected
            .get()
            .is_none_or(|id| self.set_folder_path(id, text))
    }

    fn folder_path_typed_now(&self, text: &str) {
        if let Some(id) = self.folder_page.selected.get() {
            self.folder_path_typed(id, text);
        }
    }

    fn folder_pick_path_now(&self) {
        if let Some(id) = self.folder_page.selected.get() {
            self.pick_folder_path(id);
        }
    }
}

/// Collects the files of one drop (winit sends one event per file) and hands them over when the drop is complete.
#[derive(Default)]
struct DropBatch {
    paths: Vec<PathBuf>,
}

fn with<R: Default>(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge) -> R) -> R {
    this.upgrade().map(|b| f(&b)).unwrap_or_default()
}

pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    let ui = window.global::<FolderTreeUi<'_>>();
    let t = this.clone();
    ui.on_select(move |id| with(&t, |b| b.folder_select(id_of(id))));
    let t = this.clone();
    ui.on_toggle(move |id| with(&t, |b| id_of(id).map(|id| b.folder_toggle(id)).unwrap_or_default()));
    let t = this.clone();
    ui.on_rename_start(move |id| {
        with(&t, |b| {
            if let Some(id) = id_of(id) {
                b.folder_rename_start(id);
            }
        });
    });
    let t = this.clone();
    ui.on_rename(move |id, name| with(&t, |b| id_of(id).is_some_and(|id| b.folder_rename(id, &name))));
    let t = this.clone();
    ui.on_rename_end(move || with(&t, SettingsBridge::folder_rename_end));
    let t = this.clone();
    ui.on_set_short_name(move |text| with(&t, |b| b.folder_set_short_name(&text)));
    let t = this.clone();
    ui.on_star(move |id| with(&t, |b| id_of(id).map(|id| b.folder_star(id)).unwrap_or_default()));
    let t = this.clone();
    ui.on_add_folder(move || with(&t, SettingsBridge::folder_add));
    let t = this.clone();
    ui.on_add_group(move || with(&t, SettingsBridge::folder_add_group));
    let t = this.clone();
    ui.on_delete(move || with(&t, SettingsBridge::folder_delete));
    let t = this.clone();
    ui.on_key(move |text| with(&t, |b| b.folder_key(&text)));
    let t = this.clone();
    ui.on_put_on_place(move |place| with(&t, |b| b.folder_put_on_place(place)));
    let t = this.clone();
    ui.on_clear_place(move |place| with(&t, |b| b.folder_clear_place(place)));
    let t = this.clone();
    ui.on_set_path(move |text| with(&t, |b| b.folder_set_path(&text)));
    let t = this.clone();
    ui.on_path_typed(move |text| with(&t, |b| b.folder_path_typed_now(&text)));
    let t = this.clone();
    ui.on_path_canceled(move || with(&t, SettingsBridge::folder_path_canceled));
    let t = this.clone();
    ui.on_pick_path(move || with(&t, SettingsBridge::folder_pick_path_now));
    let t = this.clone();
    ui.on_picture_kind(move |kind| with(&t, |b| b.folder_picture_kind(kind)));
    let t = this.clone();
    ui.on_pick_icon(move |name| with(&t, |b| b.folder_pick_icon(&name)));
    let t = this.clone();
    ui.on_pick_file(move || with(&t, SettingsBridge::folder_pick_file));
    let t = this.clone();
    ui.on_delete_image(move || with(&t, SettingsBridge::folder_delete_image));
    let t = this.clone();
    ui.on_search_icons(move |text| with(&t, |b| b.folder_search_icons(&text)));
    let t = this.clone();
    ui.on_pick_color(move || with(&t, SettingsBridge::folder_pick_color));
    let t = this.clone();
    ui.on_clear_color(move || with(&t, SettingsBridge::folder_clear_color));
    let t = this.clone();
    ui.on_drag_start(move |id| with(&t, |b| id_of(id).map(|id| b.folder_drag_start(id)).unwrap_or_default()));
    let t = this.clone();
    ui.on_drag_move(move |y, inside| with(&t, |b| b.folder_drag_move(y, inside)));
    let t = this.clone();
    ui.on_drag_end(move |drop| with(&t, |b| b.folder_drag_end(drop)));

    let t = this.clone();
    let batch = std::cell::RefCell::new(DropBatch::default());
    window.window().on_winit_window_event(move |_, event| {
        match event {
            WindowEvent::DroppedFile(path) => {
                with(&t, |b| b.folder_file_over(false));
                batch.borrow_mut().paths.push(path.clone());
            }
            WindowEvent::HoveredFile(_) => with(&t, |b| b.folder_file_over(true)),
            WindowEvent::HoveredFileCancelled => {
                with(&t, |b| b.folder_file_over(false));
                batch.borrow_mut().paths.clear();
            }
            // The drop's files arrive one after the other; the next pointer or redraw event ends the batch.
            _ => {
                let paths = std::mem::take(&mut batch.borrow_mut().paths);
                if !paths.is_empty() {
                    with(&t, |b| {
                        if b.page.get() == crate::ui::settings::page::Page::Folders {
                            b.folders_dropped(paths);
                        }
                    });
                }
            }
        }
        EventResult::Propagate
    });
}
