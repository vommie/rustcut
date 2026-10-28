//! Settings > Target Folders (P11.3, user decision Q-FP2): the folder tree with its details. Every change goes through
//! `SettingsWriter::update` at once (Q-W1); the page keeps only what it shows: the selected entry, the folded groups
//! (both in `ui-state.toml`), an entry being renamed, a drag, and the icon search. `rows.rs` and `details.rs` are the
//! pure parts; `actions.rs` the edits; `path.rs` the typed path.

mod actions;
pub(super) mod details;
mod path;
pub(super) mod rows;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};

use rustcut_core::folders::{FolderId, FolderTree, Picture};
use rustcut_core::settings::{ColorMode, PictureShape, Settings, SettingsWindowState};
use slint::{ComponentHandle as _, Model as _, ModelRc, VecModel};

use self::details::PictureChoice;
use self::rows::{DropTarget, TreeRow};
use super::SettingsBridge;
use crate::ui::folders::{pictures, view};
use crate::util::model::sync;
use crate::{
    FolderDetailsView, FolderIconRowView, FolderIconView, FolderPlaceView, FolderTreeRowView, FolderTreeUi,
    SettingsWindow,
};

/// Icons per row of the details' grid.
const ICONS_PER_ROW: usize = 9;
/// A row's height (`FolderTreeUi.row-height`).
const ROW_HEIGHT: f32 = 28.0;

/// A drag of an entry: what it is and where it would land.
struct Drag {
    id: FolderId,
    target: Option<DropTarget>,
}

/// The page's own state (UI thread).
pub(super) struct FolderPage {
    selected: Cell<Option<FolderId>>,
    collapsed: RefCell<BTreeSet<FolderId>>,
    renaming: Cell<Option<FolderId>>,
    drag: RefCell<Option<Drag>>,
    /// The rows as last shown (keys and drags work on them).
    shown: RefCell<Vec<TreeRow>>,
    icon_search: RefCell<String>,
    icons_open: Cell<bool>,
    rows: Rc<VecModel<FolderTreeRowView>>,
    places: Rc<VecModel<FolderPlaceView>>,
    icon_rows: Rc<VecModel<FolderIconRowView>>,
}

impl FolderPage {
    pub(super) fn new(state: &SettingsWindowState) -> Self {
        Self {
            selected: Cell::new(state.folder_selected),
            collapsed: RefCell::new(state.folder_collapsed.iter().copied().collect()),
            renaming: Cell::new(None),
            drag: RefCell::new(None),
            shown: RefCell::default(),
            icon_search: RefCell::default(),
            icons_open: Cell::new(false),
            rows: Rc::default(),
            places: Rc::default(),
            icon_rows: Rc::default(),
        }
    }

    pub(super) fn save_into(&self, state: &mut SettingsWindowState) {
        state.folder_selected = self.selected.get();
        state.folder_collapsed = self.collapsed.borrow().iter().copied().collect();
    }

    /// The models the window shows (set once when the window is made).
    pub(super) fn attach(&self, window: &SettingsWindow) {
        let ui = window.global::<FolderTreeUi<'_>>();
        ui.set_rows(ModelRc::from(Rc::clone(&self.rows)));
        ui.set_places(ModelRc::from(Rc::clone(&self.places)));
        ui.set_icon_rows(ModelRc::from(Rc::clone(&self.icon_rows)));
        ui.set_icons_per_row(i32::try_from(ICONS_PER_ROW).unwrap_or(9));
    }
}

/// An id as Slint keeps it.
fn slint_id(id: FolderId) -> i32 {
    i32::try_from(id.0).unwrap_or(-1)
}

/// An id from Slint (`None` for -1 or anything that is no id).
fn id_of(value: i32) -> Option<FolderId> {
    u32::try_from(value).ok().map(FolderId)
}

const fn shape_index(shape: PictureShape) -> i32 {
    match shape {
        PictureShape::Round => 0,
        PictureShape::Rounded => 1,
        PictureShape::Square => 2,
    }
}

impl SettingsBridge {
    /// Shows the tree, the selected entry's details, the wheel's places and (while open) the icon grid.
    pub(super) fn publish_folder_tree(&self, settings: &Settings) {
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<FolderTreeUi<'_>>();
        let page = &self.folder_page;
        let tree = &settings.export.folders;
        if page.selected.get().is_some_and(|id| tree.entry(id).is_none()) {
            page.selected.set(None);
        }
        let selected = page.selected.get();
        let shown = rows::rows(tree, &page.collapsed.borrow());
        let views = shown
            .iter()
            .map(|r| FolderTreeRowView {
                id: slint_id(r.id),
                depth: i32::try_from(r.depth).unwrap_or(0),
                group: r.group,
                expanded: r.expanded,
                source: r.is_source(),
                name: r.name.as_str().into(),
                path: r
                    .path
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
                    .into(),
                missing: r.path.as_deref().is_some_and(|p| self.presence.is_missing(p)),
                place: r.place.map_or(-1, i32::from),
                picture: tree
                    .entry(r.id)
                    .map(|e| view::picture(e, &self.pictures))
                    .unwrap_or_default(),
                selected: Some(r.id) == selected,
            })
            .collect();
        sync(&page.rows, views);
        *page.shown.borrow_mut() = shown;
        ui.set_details(self.details_view(tree, selected));
        let places = details::places(tree, selected, settings.folder_wheel.places)
            .into_iter()
            .map(|p| {
                let entry = p.occupant.and_then(|id| tree.entry(id));
                FolderPlaceView {
                    taken: entry.is_some(),
                    mine: p.mine,
                    name: entry
                        .map(rustcut_core::folders::EntryRef::name)
                        .unwrap_or_default()
                        .into(),
                    picture: entry.map(|e| view::picture(e, &self.pictures)).unwrap_or_default(),
                }
            })
            .collect();
        sync(&page.places, places);
        // The grid shows when it was asked for ("Icon" or its button), for the selected entry only.
        let icons = selected.is_some() && page.icons_open.get();
        self.publish_icons(tree, selected, icons);
        ui.set_icons_open(icons);
        ui.set_shape(shape_index(settings.folder_wheel.list_shape));
        ui.set_tint(settings.folder_wheel.color_mode == ColorMode::BackgroundAndIcons);
        ui.set_delete_tip(details::delete_tip(tree, selected).into());
        ui.set_can_delete(selected.is_some_and(|id| !id.is_source()));
        ui.set_renaming(page.renaming.get().map_or(-1, slint_id));
        let paths = tree.folders().into_iter().filter_map(|f| f.path.clone()).collect();
        let weak = Weak::clone(&self.me);
        self.presence.refresh(&self.runtime, paths, move || {
            if let Some(bridge) = weak.upgrade() {
                bridge.publish_folder_tree(&bridge.writer.current());
            }
        });
    }

    fn details_view(&self, tree: &FolderTree, selected: Option<FolderId>) -> FolderDetailsView {
        let Some((id, entry)) = selected.and_then(|id| tree.entry(id).map(|e| (id, e))) else {
            return FolderDetailsView::default();
        };
        let path = tree.find(id).and_then(|n| n.path.clone());
        let tint = view::color(entry.color());
        FolderDetailsView {
            shown: true,
            id: slint_id(id),
            source: id.is_source(),
            group: entry.is_group(),
            name: details::title(tree, id).into(),
            path: path
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
                .into(),
            missing: path.as_deref().is_some_and(|p| self.presence.is_missing(p)),
            picture_kind: PictureChoice::of(entry.picture()).index(),
            picture: view::picture(entry, &self.pictures),
            icon_name: match entry.picture() {
                Some(Picture::Icon(name)) => name.as_str().into(),
                _ => "".into(),
            },
            has_color: tint.is_some(),
            color: tint.unwrap_or_default(),
            place: entry.place().map_or(-1, i32::from),
            contents: i32::try_from(details::contents(tree, id)).unwrap_or(i32::MAX),
        }
    }

    /// The icon grid while it is open (the pictures come as they are drawn).
    fn publish_icons(&self, tree: &FolderTree, selected: Option<FolderId>, shown: bool) {
        let page = &self.folder_page;
        if !shown {
            if page.icon_rows.row_count() > 0 {
                page.icon_rows.set_vec(Vec::new());
            }
            return;
        }
        let chosen = selected
            .and_then(|id| tree.entry(id))
            .and_then(|e| e.picture().cloned());
        let rows = details::icon_rows(pictures::icon_names(), &page.icon_search.borrow(), ICONS_PER_ROW)
            .into_iter()
            .map(|names| {
                let icons: Vec<FolderIconView> = names
                    .into_iter()
                    .map(|name| {
                        let picture = Picture::Icon(name.to_owned());
                        let shown = self.pictures.image(&picture);
                        FolderIconView {
                            name: name.into(),
                            chosen: chosen.as_ref() == Some(&picture),
                            picture: crate::FolderPictureView {
                                has_image: shown.is_some(),
                                mono: shown.as_ref().is_some_and(|s| s.mono),
                                image: shown.map(|s| s.image).unwrap_or_default(),
                                initial: view::initial(name).into(),
                                ..Default::default()
                            },
                        }
                    })
                    .collect();
                FolderIconRowView {
                    icons: ModelRc::new(VecModel::from(icons)),
                }
            })
            .collect::<Vec<_>>();
        page.icon_rows.set_vec(rows);
    }
}

/// Connects the page's callbacks and the folders dropped from the file manager.
pub(super) fn connect(this: &Weak<SettingsBridge>, window: &SettingsWindow) {
    actions::connect(this, window);
}
