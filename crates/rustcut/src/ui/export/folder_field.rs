//! The Job Finalization card's target folder field and its list (P11.4, user decision Q-FP2): the field shows the
//! target's picture and name; the list (`folder_list.rs` is its pure state) lets the user pick another one. Choosing
//! only sets the folder; Save stays separate.

use std::path::Path;

use rustcut_core::folders::{FolderId, FolderTree, ListRow, SOURCE_NAME, Section};
use rustcut_core::settings::{ColorMode, PictureShape, Settings};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::ExportBridge;
use super::folder_list::{JOB_TARGET, ListKey, Outcome};
use super::target::JOB_TARGET as JOB_TARGET_NAME;
use super::target::Target;
use crate::ui::folders::view;
use crate::{FolderList, FolderListRowView, FolderPictureView};

/// A heading's and an entry's height in the list (`folder_list.slint`).
const HEADING_HEIGHT: f32 = 22.0;
const ENTRY_HEIGHT: f32 = 28.0;

/// How the pickers look, from the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Look {
    shape: i32,
    tint: bool,
    stripe: bool,
}

impl Look {
    pub(super) const fn of(settings: &Settings) -> Self {
        let mode = settings.folder_wheel.color_mode;
        Self {
            shape: match settings.folder_wheel.list_shape {
                PictureShape::Round => 0,
                PictureShape::Rounded => 1,
                PictureShape::Square => 2,
            },
            tint: matches!(mode, ColorMode::BackgroundAndIcons),
            stripe: !matches!(mode, ColorMode::Pointed),
        }
    }
}

/// The text of a key event as a list key.
fn list_key(text: &str) -> Option<ListKey> {
    use slint::platform::Key;
    let is = |k: Key| text == slint::SharedString::from(k).as_str();
    [
        (Key::UpArrow, ListKey::Up),
        (Key::DownArrow, ListKey::Down),
        (Key::LeftArrow, ListKey::Left),
        (Key::RightArrow, ListKey::Right),
        (Key::Return, ListKey::Enter),
        (Key::Escape, ListKey::Escape),
    ]
    .into_iter()
    .find(|(k, _)| is(*k))
    .map(|(_, l)| l)
}

/// The list's width bounds: wide enough for most names, never tied to the field (the user's wish 2026-10-28).
const MIN_WIDTH: f32 = 240.0;
const MAX_WIDTH: f32 = 460.0;

/// The width the widest row wants: its indentation, fold mark, stripe, picture, name, groups and warning mark, plus
/// the panel's padding and the scroll bar.
fn panel_width(rows: &[FolderListRowView]) -> f32 {
    // The estimate runs a little short of the real font (umlauts, bold): this much room more.
    let width = |text: &str, font: f32| crate::ui::folders::wheel::label::width(text, font) * 1.15;
    let widest = rows
        .iter()
        .map(|r| {
            if r.heading {
                return width(&r.title, 10.0) * 1.1 + 16.0;
            }
            let depth = f32::from(u16::try_from(r.depth).unwrap_or(u16::MAX));
            let name = width(&r.name, 12.0) * if r.current || r.group { 1.08 } else { 1.0 };
            let context = if r.context.is_empty() {
                0.0
            } else {
                6.0 + width(&r.context, 11.0)
            };
            let missing = if r.missing { 22.0 } else { 0.0 };
            // Padding 8 + 8, fold mark 12 + 6, stripe 3 + 3, picture 18 + 6.
            64.0 + depth * 16.0 + name + context + missing
        })
        .fold(0.0_f32, f32::max);
    // The panel's padding (8 + 8) and the scroll bar.
    (widest + 16.0 + 14.0).clamp(MIN_WIDTH, MAX_WIDTH)
}

/// A color stripe only where the picture does not show the color: an own image, or a one-color picture drawn in the
/// text color. A letter stands on the color and a tinted icon has it (the user's note 2026-10-28).
const fn needs_stripe(picture: &FolderPictureView, tint: bool) -> bool {
    picture.has_color && picture.has_image && !(picture.mono && tint)
}

/// The top of row `index` in the list (for scrolling the highlight into view).
fn row_top(rows: &[ListRow], index: usize) -> f32 {
    rows.iter()
        .take(index)
        .map(|r| {
            if matches!(r, ListRow::Heading(_)) {
                HEADING_HEIGHT
            } else {
                ENTRY_HEIGHT
            }
        })
        .sum()
}

impl ExportBridge {
    /// The field: the target's picture, name and path.
    pub(super) fn publish_field(&self, folder: Option<&Path>) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<FolderList<'_>>();
        let st = self.state.borrow();
        let look = self.look.get();
        let tree = &st.target.tree;
        let picture = match &st.target.target {
            Target::Entry(id) => tree
                .entry(*id)
                .map(|e| view::picture(e, &self.pictures))
                .unwrap_or_default(),
            Target::Loose(path) => FolderPictureView {
                initial: view::initial(&rustcut_core::folders::default_name(path)).into(),
                ..FolderPictureView::default()
            },
        };
        ui.set_picture(picture);
        ui.set_label(st.target.label(&st.target.target).into());
        ui.set_tooltip(
            folder
                .map_or_else(
                    || "The folder of the source video".to_owned(),
                    |f| f.display().to_string(),
                )
                .into(),
        );
        ui.set_shape(look.shape);
        ui.set_tint(look.tint);
        ui.set_stripe(look.stripe);
    }

    /// The list's rows (while it is open).
    pub(super) fn publish_list(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<FolderList<'_>>();
        let list = self.list.borrow();
        ui.set_open(list.open);
        if !list.open {
            return;
        }
        let st = self.state.borrow();
        let tree = &st.target.tree;
        let current = match st.target.target {
            Target::Entry(id) => Some(id),
            Target::Loose(_) => Some(JOB_TARGET),
        };
        let mut in_tree = false;
        let rows: Vec<FolderListRowView> = list
            .rows
            .iter()
            .map(|r| {
                in_tree |= matches!(r, ListRow::Heading(Section::All));
                let mut view = self.row_view(tree, r, current);
                view.foldable = in_tree && view.group;
                view
            })
            .collect();
        ui.set_rows_height(row_top(&list.rows, list.rows.len()));
        ui.set_panel_width(panel_width(&rows));
        ui.set_rows(ModelRc::new(VecModel::from(rows)));
        let highlight = list.highlight.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1);
        ui.set_highlight(highlight);
        ui.set_highlight_top(list.highlight.map_or(0.0, |i| row_top(&list.rows, i)));
        ui.set_query(list.query.as_str().into());
    }

    fn row_view(&self, tree: &FolderTree, row: &ListRow, current: Option<FolderId>) -> FolderListRowView {
        match row {
            ListRow::Heading(section) => FolderListRowView {
                heading: true,
                title: match section {
                    Section::Favorites => "FAVORITES",
                    Section::Recent => "RECENT",
                    Section::All => "ALL FOLDERS",
                }
                .into(),
                ..FolderListRowView::default()
            },
            ListRow::Entry { id, context, .. } if *id == JOB_TARGET => FolderListRowView {
                name: JOB_TARGET_NAME.into(),
                context: context.as_str().into(),
                picture: FolderPictureView {
                    initial: "J".into(),
                    ..FolderPictureView::default()
                },
                current: current == Some(JOB_TARGET),
                ..FolderListRowView::default()
            },
            ListRow::Entry {
                id,
                depth,
                group,
                expanded,
                context,
            } => {
                let entry = tree.entry(*id);
                let path = tree.find(*id).and_then(|n| n.path.as_deref());
                let picture = entry.map(|e| view::picture(e, &self.pictures)).unwrap_or_default();
                FolderListRowView {
                    heading: false,
                    title: "".into(),
                    depth: i32::try_from(*depth).unwrap_or(0),
                    group: *group,
                    foldable: false,
                    expanded: *expanded,
                    name: entry.map_or(SOURCE_NAME, rustcut_core::folders::EntryRef::name).into(),
                    context: context.as_str().into(),
                    missing: path.is_some_and(|p| self.presence.is_missing(p)),
                    stripe: needs_stripe(&picture, self.look.get().tint),
                    picture,
                    current: current == Some(*id),
                }
            }
        }
    }

    /// The field was clicked: the list opens with the target highlighted; the folders are checked.
    pub(super) fn open_list(&self) {
        {
            let st = self.state.borrow();
            let current = if st.target.is_loose() {
                JOB_TARGET
            } else {
                st.target.saved_id()
            };
            let mut list = self.list.borrow_mut();
            list.job_target = st.target.loose().map(|p| p.display().to_string());
            list.open(&st.target.tree, &st.recent, current);
            drop(list);
            let paths = st
                .target
                .tree
                .folders()
                .into_iter()
                .filter_map(|f| f.path.clone())
                .collect();
            let weak = std::rc::Weak::clone(&self.me);
            self.presence.refresh(&self.runtime, paths, move || {
                if let Some(bridge) = weak.upgrade() {
                    bridge.publish_list();
                }
            });
        }
        self.publish_list();
    }

    pub(super) fn close_list(&self) {
        self.list.borrow_mut().close();
        self.publish_list();
    }

    fn picked(&self, id: FolderId) {
        self.close_list();
        // "Job target" is the loaded job's folder: it stays.
        if id == JOB_TARGET {
            return;
        }
        tracing::info!(id = id.0, "[Export] Target folder chosen");
        self.choose_target(id);
    }

    /// Another target (the list, the folder wheel); the job is not saved.
    pub fn choose_target(&self, id: FolderId) {
        self.state.borrow_mut().target.select_id(id);
        self.rescan();
        self.publish();
    }

    pub(super) fn list_click(&self, index: usize) {
        let outcome = {
            let st = self.state.borrow();
            self.list.borrow_mut().click(&st.target.tree, &st.recent, index)
        };
        self.list_outcome(outcome);
    }

    pub(super) fn list_search(&self, text: &str) {
        {
            let st = self.state.borrow();
            self.list.borrow_mut().search(&st.target.tree, &st.recent, text);
        }
        self.publish_list();
    }

    /// `true` = the key was the list's.
    pub(super) fn list_key(&self, text: &str) -> bool {
        let Some(key) = list_key(text) else {
            return false;
        };
        let outcome = {
            let st = self.state.borrow();
            self.list.borrow_mut().key(&st.target.tree, &st.recent, key)
        };
        self.list_outcome(outcome);
        true
    }

    fn list_outcome(&self, outcome: Outcome) {
        match outcome {
            Outcome::Nothing => {}
            Outcome::Changed => self.publish_list(),
            Outcome::Pick(id) => self.picked(id),
            Outcome::Close => self.close_list(),
        }
    }

    /// The tree or the recent targets changed while the list is open.
    pub(super) fn refresh_list(&self) {
        if !self.list.borrow().open {
            return;
        }
        {
            let st = self.state.borrow();
            let mut list = self.list.borrow_mut();
            list.job_target = st.target.loose().map(|p| p.display().to_string());
            list.refresh(&st.target.tree, &st.recent);
        }
        self.publish_list();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_stripes() {
        let row = |name: &str, context: &str| FolderListRowView {
            name: name.into(),
            context: context.into(),
            ..FolderListRowView::default()
        };
        assert!(
            (panel_width(&[row("a", "")]) - MIN_WIDTH).abs() < f32::EPSILON,
            "never narrower"
        );
        let long = panel_width(&[row("einzusammenhängenderordnername", "Kunden › Archiv")]);
        assert!(long > MIN_WIDTH && long <= MAX_WIDTH);
        assert!(
            (panel_width(&[row(&"x".repeat(200), "")]) - MAX_WIDTH).abs() < f32::EPSILON,
            "never wider"
        );
        let colored = |has_image, mono| FolderPictureView {
            has_color: true,
            has_image,
            mono,
            ..FolderPictureView::default()
        };
        assert!(
            !needs_stripe(&colored(false, false), true),
            "a letter stands on its color"
        );
        assert!(!needs_stripe(&colored(true, true), true), "a tinted icon has its color");
        assert!(needs_stripe(&colored(true, true), false), "an icon in the text color");
        assert!(needs_stripe(&colored(true, false), true), "an own image");
        assert!(!needs_stripe(&FolderPictureView::default(), true), "no color");
    }

    #[test]
    fn keys_and_row_tops() {
        let text = |k: slint::platform::Key| slint::SharedString::from(k).to_string();
        assert_eq!(list_key(&text(slint::platform::Key::UpArrow)), Some(ListKey::Up));
        assert_eq!(list_key(&text(slint::platform::Key::Return)), Some(ListKey::Enter));
        assert_eq!(list_key("a"), None);
        let rows = [
            ListRow::Entry {
                id: FolderId::SOURCE,
                depth: 0,
                group: false,
                expanded: false,
                context: String::new(),
            },
            ListRow::Heading(Section::All),
            ListRow::Heading(Section::Recent),
        ];
        assert!((row_top(&rows, 2) - 50.0).abs() < f32::EPSILON);
        assert!(row_top(&rows, 0).abs() < f32::EPSILON);
    }
}
