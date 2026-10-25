//! RustCut's context menu (M4.1): one menu at a time, drawn over the window in RustView's style (`ui/context_menu.slint`)
//! with one level of submenus. Entries that delete or discard something are held instead of clicked (user decision
//! Q-Q4; the row fills up while it is held). A caller builds the entries ([`MenuItem`]) and gets the chosen entry's id
//! back on the UI thread. A menu of a button ([`ContextMenuBridge::show_at`]) hangs from it with an arrow (P9.6).

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::icons::Icon;
use crate::{ContextMenu, MainWindow, MenuItemView, MenuLevelView};

/// One entry of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Action(MenuAction),
    Separator,
    /// A title over the entries (not clickable).
    Heading(String),
    /// Opens a second level beside the row (its items are actions and separators).
    Submenu {
        title: String,
        icon: Icon,
        enabled: bool,
        items: Vec<MenuItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuAction {
    pub id: String,
    pub title: String,
    pub icon: Option<Icon>,
    pub enabled: bool,
    /// Deletes or discards something: the entry has to be held (Q-Q4).
    pub hold: bool,
    pub tooltip: String,
    pub shortcut: String,
    /// The icon's color.
    pub tint: MenuTint,
    /// The chosen entry: a check mark on the right.
    pub checked: bool,
}

/// How far an entry goes, as its icon's color (`MenuTints` in `context_menu.slint`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MenuTint {
    /// The text's color.
    #[default]
    Plain,
    Green,
    Yellow,
    Orange,
    Red,
}

impl MenuTint {
    const fn number(self) -> i32 {
        match self {
            Self::Plain => 0,
            Self::Green => 1,
            Self::Yellow => 2,
            Self::Orange => 3,
            Self::Red => 4,
        }
    }
}

impl MenuAction {
    pub fn new(id: impl Into<String>, title: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon,
            enabled: true,
            hold: false,
            tooltip: String::new(),
            shortcut: String::new(),
            tint: MenuTint::Plain,
            checked: false,
        }
    }

    #[must_use]
    pub const fn tint(mut self, tint: MenuTint) -> Self {
        self.tint = tint;
        self
    }

    #[must_use]
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    #[must_use]
    pub const fn hold(mut self) -> Self {
        self.hold = true;
        self
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }

    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = shortcut.into();
        self
    }
}

impl From<MenuAction> for MenuItem {
    fn from(action: MenuAction) -> Self {
        Self::Action(action)
    }
}

/// Kinds of a row as `context_menu.slint` knows them.
const KIND_ACTION: i32 = 0;
const KIND_SEPARATOR: i32 = 1;
const KIND_SUBMENU: i32 = 2;
const KIND_HEADING: i32 = 3;

/// The rows of the top level and the levels of the submenus (a submenu row points at its level by index).
pub fn views(items: &[MenuItem]) -> (Vec<MenuItemView>, Vec<Vec<MenuItemView>>) {
    let mut levels = Vec::new();
    let top = items.iter().map(|item| row(item, &mut levels)).collect();
    (top, levels)
}

fn row(item: &MenuItem, levels: &mut Vec<Vec<MenuItemView>>) -> MenuItemView {
    match item {
        MenuItem::Action(a) => MenuItemView {
            id: a.id.as_str().into(),
            title: a.title.as_str().into(),
            icon: a.icon.map(Icon::to_slint).unwrap_or_default(),
            kind: KIND_ACTION,
            enabled: a.enabled,
            hold: a.hold,
            tooltip: a.tooltip.as_str().into(),
            shortcut: a.shortcut.as_str().into(),
            level: -1,
            tint: a.tint.number(),
            checked: a.checked,
        },
        MenuItem::Heading(title) => MenuItemView {
            title: title.as_str().into(),
            kind: KIND_HEADING,
            level: -1,
            ..MenuItemView::default()
        },
        MenuItem::Separator => MenuItemView {
            kind: KIND_SEPARATOR,
            level: -1,
            ..MenuItemView::default()
        },
        MenuItem::Submenu {
            title,
            icon,
            enabled,
            items,
        } => {
            let sub: Vec<MenuItemView> = items
                .iter()
                .filter(|i| !matches!(i, MenuItem::Submenu { .. }))
                .map(|i| row(i, levels))
                .collect();
            let level = i32::try_from(levels.len()).unwrap_or(i32::MAX);
            levels.push(sub);
            MenuItemView {
                title: title.as_str().into(),
                icon: icon.to_slint(),
                kind: KIND_SUBMENU,
                enabled: *enabled && !items.is_empty(),
                level,
                ..MenuItemView::default()
            }
        }
    }
}

type Handler = Box<dyn Fn(&str)>;

/// Shows menus and hands the chosen entry to the caller that opened it.
pub struct ContextMenuBridge {
    window: slint::Weak<MainWindow>,
    handler: RefCell<Option<Handler>>,
}

impl ContextMenuBridge {
    /// The app owns the only strong reference.
    pub fn install(window: &MainWindow) -> Rc<Self> {
        let bridge = Rc::new(Self {
            window: window.as_weak(),
            handler: RefCell::new(None),
        });
        let menu = window.global::<ContextMenu<'_>>();
        let weak = Rc::downgrade(&bridge);
        menu.on_activated(move |id| {
            if let Some(b) = weak.upgrade() {
                b.close();
                // Taken out first: the handler may open the next menu.
                let handler = b.handler.borrow_mut().take();
                if let Some(handler) = handler {
                    handler(id.as_str());
                }
            }
        });
        let weak = Rc::downgrade(&bridge);
        menu.on_dismissed(move || {
            if let Some(b) = weak.upgrade() {
                b.close();
                b.handler.borrow_mut().take();
            }
        });
        bridge
    }

    /// Opens `items` at the window position (`x`, `y`); `on_pick` gets the id of the chosen entry.
    pub fn show(&self, items: &[MenuItem], x: f32, y: f32, on_pick: impl Fn(&str) + 'static) {
        self.open(items, None, x, y, on_pick);
    }

    /// Opens `items` hanging from the button at `anchor` (window x, y, width, height) with an arrow to it.
    pub fn show_at(&self, items: &[MenuItem], anchor: [f32; 4], on_pick: impl Fn(&str) + 'static) {
        let [x, y, _, height] = anchor;
        self.open(items, Some(anchor), x, y + height, on_pick);
    }

    fn open(&self, items: &[MenuItem], anchor: Option<[f32; 4]>, x: f32, y: f32, on_pick: impl Fn(&str) + 'static) {
        let Some(window) = self.window.upgrade() else { return };
        let (top, levels) = views(items);
        let menu = window.global::<ContextMenu<'_>>();
        menu.set_items(ModelRc::new(VecModel::from(top)));
        menu.set_levels(ModelRc::new(VecModel::from(
            levels
                .into_iter()
                .map(|items| MenuLevelView {
                    items: ModelRc::new(VecModel::from(items)),
                })
                .collect::<Vec<_>>(),
        )));
        menu.set_x(x);
        menu.set_y(y);
        menu.set_anchored(anchor.is_some());
        let [ax, ay, aw, ah] = anchor.unwrap_or_default();
        menu.set_anchor_x(ax);
        menu.set_anchor_y(ay);
        menu.set_anchor_width(aw);
        menu.set_anchor_height(ah);
        *self.handler.borrow_mut() = Some(Box::new(on_pick));
        menu.set_open(true);
    }

    fn close(&self) {
        if let Some(window) = self.window.upgrade() {
            window.global::<ContextMenu<'_>>().set_open(false);
            window.invoke_focus_keys();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submenus_become_levels() {
        let items = vec![
            MenuAction::new("play", "Play Output", Some(Icon::PlayFile)).into(),
            MenuItem::Separator,
            MenuItem::Submenu {
                title: "Render Again With".into(),
                icon: Icon::JobRetryWith,
                enabled: true,
                items: vec![
                    MenuAction::new("p:a", "A", None).into(),
                    MenuAction::new("p:b", "B", None).enabled(false).into(),
                ],
            },
            MenuItem::Submenu {
                title: "Empty".into(),
                icon: Icon::JobRetryWith,
                enabled: true,
                items: Vec::new(),
            },
            MenuAction::new("remove", "Remove 3 Jobs", Some(Icon::JobRemove))
                .hold()
                .into(),
        ];
        let (top, levels) = views(&items);
        assert_eq!(top.len(), 5);
        assert_eq!((top[2].kind, top[2].level), (KIND_SUBMENU, 0));
        assert!(!top[3].enabled, "an empty submenu cannot open");
        assert!(top[4].hold);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0][1].id.as_str(), "p:b");
        assert!(!levels[0][1].enabled);
    }
}
