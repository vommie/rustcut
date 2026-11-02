//! The folder select (P11.4, user decision Q-FP2; a component of its own since P12 block 7, the user's wish in Q-P12f): a
//! field that shows a folder's picture and name, and the list that opens from it with "Next to the source", the
//! favorites, the recent targets and the tree. One list per window (`FolderList` global) serves every field: a field
//! names its owner, the owner registers what a click on its field does (`on_field`) and opens the list with its own tree,
//! recent targets and current entry (`open`); a pick goes back to that owner only. `list.rs` is the pure state.

pub mod list;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use list::{FolderList as ListState, JOB_TARGET, ListKey, Outcome};
use rustcut_core::folders::{FolderId, FolderTree, ListRow, SOURCE_NAME, Section};
use rustcut_core::settings::{ColorMode, PictureShape, Settings};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::pictures::Pictures;
use super::presence::FolderPresence;
use super::view;
use crate::{FolderFieldView, FolderList, FolderListRowView, FolderPictureView, MainWindow};

/// The name of the temporary first row (a loaded job's own folder).
pub const JOB_TARGET_NAME: &str = "Job target";

/// A heading's and an entry's height in the list (`folder_list.slint`).
const HEADING_HEIGHT: f32 = 22.0;
const ENTRY_HEIGHT: f32 = 28.0;

/// The list's width bounds: wide enough for most names, never tied to the field (the user's wish 2026-10-28).
const MIN_WIDTH: f32 = 240.0;
const MAX_WIDTH: f32 = 460.0;

/// How the pickers look, from the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    shape: i32,
    tint: bool,
    stripe: bool,
}

impl Look {
    pub const fn of(settings: &Settings) -> Self {
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

/// What an owner shows in the list when it opens it.
pub struct Session {
    pub owner: &'static str,
    pub tree: FolderTree,
    pub recent: Vec<FolderId>,
    /// The highlighted entry ([`JOB_TARGET`] for the loaded job's own folder).
    pub current: FolderId,
    /// The loaded job's own folder, shown as the first row.
    pub job_target: Option<PathBuf>,
    pub on_pick: Rc<dyn Fn(FolderId)>,
}

/// What a click on an owner's field does.
type Opener = Rc<dyn Fn()>;

pub struct FolderSelect {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    pictures: Rc<Pictures>,
    presence: Rc<FolderPresence>,
    look: Cell<Look>,
    list: RefCell<ListState>,
    session: RefCell<Option<Session>>,
    openers: RefCell<HashMap<&'static str, Opener>>,
}

impl FolderSelect {
    /// The window's select; `collapsed` are the groups folded in the list (`ui-state.toml`).
    pub fn install(
        window: &MainWindow,
        runtime: tokio::runtime::Handle,
        pictures: &Rc<Pictures>,
        collapsed: &[FolderId],
        settings: &Settings,
    ) -> Rc<Self> {
        let select = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime,
            pictures: Rc::clone(pictures),
            presence: Rc::default(),
            look: Cell::new(Look::of(settings)),
            list: RefCell::new(ListState::new(collapsed)),
            session: RefCell::new(None),
            openers: RefCell::new(HashMap::new()),
        });
        let weak = Rc::downgrade(&select);
        pictures.listen(move || {
            if let Some(s) = weak.upgrade() {
                s.publish_list();
            }
        });
        let ui = window.global::<FolderList<'_>>();
        let weak = Rc::downgrade(&select);
        ui.on_opened(move |owner| {
            let opener = weak
                .upgrade()
                .and_then(|s| s.openers.borrow().get(owner.as_str()).cloned());
            if let Some(opener) = opener {
                opener();
            }
        });
        let weak = Rc::downgrade(&select);
        ui.on_closed(move || {
            if let Some(s) = weak.upgrade() {
                s.close();
            }
        });
        let weak = Rc::downgrade(&select);
        ui.on_pick(move |index| {
            if let (Some(s), Ok(index)) = (weak.upgrade(), usize::try_from(index)) {
                s.click(index);
            }
        });
        let weak = Rc::downgrade(&select);
        ui.on_search(move |text| {
            if let Some(s) = weak.upgrade() {
                s.search(&text);
            }
        });
        let weak = Rc::downgrade(&select);
        ui.on_key(move |text| weak.upgrade().is_some_and(|s| s.key(&text)));
        select.apply_look();
        select
    }

    /// What a click on `owner`'s field does (usually: [`open`](Self::open) with the owner's data).
    pub fn on_field(&self, owner: &'static str, opener: impl Fn() + 'static) {
        self.openers.borrow_mut().insert(owner, Rc::new(opener));
    }

    pub fn is_open_for(&self, owner: &str) -> bool {
        self.list.borrow().open && self.session.borrow().as_ref().is_some_and(|s| s.owner == owner)
    }

    /// Opens the list for `session`'s owner with its current entry highlighted; the folders are checked.
    pub fn open(&self, session: Session) {
        {
            let mut list = self.list.borrow_mut();
            list.job_target = session.job_target.as_ref().map(|p| p.display().to_string());
            list.open(&session.tree, &session.recent, session.current);
        }
        let paths = session
            .tree
            .folders()
            .into_iter()
            .filter_map(|f| f.path.clone())
            .collect();
        if let Some(window) = self.window.upgrade() {
            window.global::<FolderList<'_>>().set_owner(session.owner.into());
        }
        self.session.replace(Some(session));
        let weak = Weak::clone(&self.me);
        self.presence.refresh(&self.runtime, paths, move || {
            if let Some(s) = weak.upgrade() {
                s.publish_list();
            }
        });
        self.publish_list();
    }

    /// The owner's tree or recent targets changed while its list is open.
    pub fn refresh(&self, owner: &str, tree: &FolderTree, recent: &[FolderId], job_target: Option<&Path>) {
        if !self.is_open_for(owner) {
            return;
        }
        {
            let mut list = self.list.borrow_mut();
            list.job_target = job_target.map(|p| p.display().to_string());
            list.refresh(tree, recent);
        }
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.tree = tree.clone();
            recent.clone_into(&mut session.recent);
            session.job_target = job_target.map(Path::to_path_buf);
        }
        self.publish_list();
    }

    pub fn close(&self) {
        self.list.borrow_mut().close();
        self.session.replace(None);
        self.publish_list();
    }

    /// The groups folded in the list (saved at quit).
    pub fn collapsed(&self) -> Vec<FolderId> {
        self.list.borrow().collapsed.iter().copied().collect()
    }

    /// The settings changed: the pictures' look.
    pub fn settings_changed(&self, settings: &Settings) {
        self.look.set(Look::of(settings));
        self.apply_look();
        self.publish_list();
    }

    fn apply_look(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<FolderList<'_>>();
        let look = self.look.get();
        ui.set_shape(look.shape);
        ui.set_tint(look.tint);
        ui.set_stripe(look.stripe);
    }

    /// What a field shows for an entry of `tree` (`None` = "Next to the source").
    pub fn entry_field(&self, tree: &FolderTree, id: FolderId, label: String, tooltip: String) -> FolderFieldView {
        FolderFieldView {
            picture: tree
                .entry(id)
                .map(|e| view::picture(e, &self.pictures))
                .unwrap_or_default(),
            label: label.into(),
            tooltip: tooltip.into(),
        }
    }

    /// What a field shows for a folder that is no entry of the tree (a typed path, a loaded job's folder).
    pub fn loose_field(path: &Path, label: String) -> FolderFieldView {
        FolderFieldView {
            picture: FolderPictureView {
                initial: view::initial(&rustcut_core::folders::default_name(path)).into(),
                ..FolderPictureView::default()
            },
            label: label.into(),
            tooltip: path.display().to_string().into(),
        }
    }

    /// The list's rows (while it is open).
    pub fn publish_list(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<FolderList<'_>>();
        let list = self.list.borrow();
        ui.set_open(list.open);
        if !list.open {
            return;
        }
        let session = self.session.borrow();
        let Some(session) = session.as_ref() else { return };
        let mut in_tree = false;
        let rows: Vec<FolderListRowView> = list
            .rows
            .iter()
            .map(|r| {
                in_tree |= matches!(r, ListRow::Heading(Section::All));
                let mut view = self.row_view(&session.tree, r, session.current);
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

    fn row_view(&self, tree: &FolderTree, row: &ListRow, current: FolderId) -> FolderListRowView {
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
                current: current == JOB_TARGET,
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
                    current: current == *id,
                }
            }
        }
    }

    fn click(&self, index: usize) {
        let outcome = {
            let session = self.session.borrow();
            let Some(s) = session.as_ref() else { return };
            self.list.borrow_mut().click(&s.tree, &s.recent, index)
        };
        self.outcome(outcome);
    }

    fn search(&self, text: &str) {
        {
            let session = self.session.borrow();
            let Some(s) = session.as_ref() else { return };
            self.list.borrow_mut().search(&s.tree, &s.recent, text);
        }
        self.publish_list();
    }

    /// `true` = the key was the list's.
    fn key(&self, text: &str) -> bool {
        let Some(key) = list_key(text) else {
            return false;
        };
        let outcome = {
            let session = self.session.borrow();
            let Some(s) = session.as_ref() else { return false };
            self.list.borrow_mut().key(&s.tree, &s.recent, key)
        };
        self.outcome(outcome);
        true
    }

    fn outcome(&self, outcome: Outcome) {
        match outcome {
            Outcome::Nothing => {}
            Outcome::Changed => self.publish_list(),
            Outcome::Pick(id) => {
                let pick = self.session.borrow().as_ref().map(|s| Rc::clone(&s.on_pick));
                self.close();
                if let Some(pick) = pick {
                    pick(id);
                }
            }
            Outcome::Close => self.close(),
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
