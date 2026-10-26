//! The filters' options menus (P9.13 step 3, Q-P9.13) and the rows' context menus. One options menu is open at a time
//! (`menu_kind`); its header resets its options to the defaults of Settings > Filters (held), saves them as those
//! defaults (`SettingsWriter`, the Settings window and the live listener follow), pins it (kept in `ui-state.toml`)
//! and jumps to Settings > Filters. A right click on a row offers what the row can do, also Move Up and Move Down
//! (the card's arrows went with drag & drop, the review's note 12#1).

use std::rc::{Rc, Weak};

use rustcut_core::edit::{FilterKind, RatioPreset};
use slint::ComponentHandle as _;

use super::view::{self, RowView};
use super::{FiltersBridge, kind_from_id};
use crate::icons::Icon;
use crate::ui::jump::{self, JumpTarget};
use crate::ui::menu::{MenuAction, MenuItem};
use crate::ui::notices::{Kind, Notice};
use crate::ui::settings::page::Page;
use crate::{Filters, MainWindow};

/// The size of a row's adjustments button (`IconButton`, `FilterMetrics.options-width`).
const BUTTON_SIZE: (f32, f32) = (26.0, 24.0);

/// The first option of each filter's card in Settings > Filters (the jump's target).
const fn settings_key(kind: FilterKind) -> &'static str {
    match kind {
        FilterKind::Deinterlace => "filters.deinterlacer",
        FilterKind::Crop => "filters.crop_search.limit",
        FilterKind::Deshake => "filters.deshake.shakiness",
        FilterKind::Resize | FilterKind::Orientation => "filters.resize_algorithm",
    }
}

/// The entries of a row's context menu.
pub fn row_items(row: &RowView) -> Vec<MenuItem> {
    let mut items = vec![MenuAction::new("toggle", if row.enabled { "Switch Off" } else { "Switch On" }, None).into()];
    if row.has_menu() {
        items.push(MenuAction::new("options", "Options...", Some(Icon::FilterOptions)).into());
    }
    items.push(MenuItem::Separator);
    items.push(
        MenuAction::new("up", "Move Up", Some(Icon::MoveUp))
            .enabled(row.can_up)
            .tooltip("Earlier in the chain (the filters run from top to bottom)")
            .into(),
    );
    items.push(
        MenuAction::new("down", "Move Down", Some(Icon::MoveDown))
            .enabled(row.can_down)
            .tooltip("Later in the chain (the filters run from top to bottom)")
            .into(),
    );
    if row.has_menu() {
        items.push(MenuItem::Separator);
        items.push(
            MenuAction::new("reset", "Reset Options", Some(Icon::FilterReset))
                .hold()
                .tooltip("The options of this filter back to the defaults of Settings > Filters")
                .into(),
        );
    }
    items
}

/// The resize ratio menu: a heading and one entry per preset (its id = the preset's index).
pub fn resize_ratio_items(presets: &[RatioPreset]) -> Vec<MenuItem> {
    std::iter::once(MenuItem::Heading("Height from the width".into()))
        .chain(
            presets
                .iter()
                .enumerate()
                .map(|(i, p)| MenuAction::new(i.to_string(), p.label.clone(), None).into()),
        )
        .collect()
}

impl FiltersBridge {
    /// The adjustments button: opens `kind`'s menu, or closes it when it is the open one.
    fn open_menu(&self, kind: FilterKind) {
        let open = (self.menu_kind.get() != Some(kind)).then_some(kind);
        tracing::debug!(menu = ?open, "[Filters] Options menu");
        self.menu_kind.set(open);
        self.publish_card();
    }

    pub fn close_menu(&self) {
        if self.menu_kind.take().is_some() {
            tracing::debug!("[Filters] Options menu closed");
            self.publish_card();
        }
    }

    fn set_menu_pinned(&self, pinned: bool) {
        self.menu_pinned.set(pinned);
        self.publish_card();
    }

    /// The menu's held reset: its options back to the defaults.
    fn menu_reset(&self, kind: FilterKind) {
        self.update(|s| s.reset_options(kind));
        Self::tell(Notice::new(
            Kind::FiltersReset,
            format!("{} options reset", kind.label()),
        ));
    }

    /// "Save as default": the menu's options become the defaults of Settings > Filters (every new video starts with
    /// them). The state is not borrowed meanwhile: the settings' listener hands the new defaults to this bridge.
    fn menu_save(&self, kind: FilterKind) {
        let defaults = self.state.borrow().options_as_defaults(kind);
        let saved = self.writer.update(|s| s.filters = defaults);
        if saved {
            tracing::info!(filter = kind.label(), "[Filters] Options saved as the defaults");
            Self::tell(Notice::new(
                Kind::FilterDefaultsSaved,
                format!("{} options saved as the defaults", kind.label()),
            ));
        } else {
            Self::tell(
                Notice::new(Kind::SettingsReadOnly, "Not saved")
                    .detail("config.toml is from a newer RustCut: the defaults hold for this run only."),
            );
        }
        self.publish_card();
    }

    /// The interlace check runs once more (the Deinterlace menu's "Analyze Again").
    fn analyze_again(&self) {
        let Some(source) = self.state.borrow().source().copied() else {
            return;
        };
        if let Some(task) = self.idet_task.borrow_mut().take() {
            task.abort();
        }
        tracing::info!("[Filters] Interlace check again");
        self.check_interlace(source);
        self.publish_card();
    }

    /// A right click on a row: its context menu at the window position; `button` = the top left corner of the row's
    /// adjustments button, where "Options..." hangs the options menu.
    fn row_menu(&self, kind: FilterKind, x: f32, y: f32, button: (f32, f32)) {
        let Some(menu) = self.menu.upgrade() else { return };
        let Some(row) = view::card(&self.state.borrow())
            .rows
            .into_iter()
            .find(|r| r.kind == kind)
        else {
            return;
        };
        let this = Weak::clone(&self.me);
        menu.show(&row_items(&row), x, y, move |id| {
            if let Some(bridge) = this.upgrade() {
                bridge.row_action(kind, id, button);
            }
        });
    }

    fn row_action(&self, kind: FilterKind, id: &str, button: (f32, f32)) {
        match id {
            "toggle" => self.update(|s| s.toggle(kind)),
            "options" => {
                if let Some(window) = self.window.upgrade() {
                    let f = window.global::<Filters<'_>>();
                    f.set_menu_anchor_x(button.0);
                    f.set_menu_anchor_y(button.1);
                    f.set_menu_anchor_width(BUTTON_SIZE.0);
                    f.set_menu_anchor_height(BUTTON_SIZE.1);
                }
                self.menu_kind.set(Some(kind));
                self.publish_card();
            }
            "up" | "down" => self.update(|s| {
                s.move_filter(kind, id == "up");
            }),
            "reset" => self.menu_reset(kind),
            _ => tracing::debug!(id, "[Filters] Unknown row menu entry"),
        }
    }

    /// The resize's ratio button: the height for one of the crop's ratios (Settings > Filters, 13#4) from the width.
    fn resize_ratio_menu(&self, anchor: [f32; 4]) {
        let Some(menu) = self.menu.upgrade() else { return };
        let presets = self.state.borrow().ratios().presets.clone();
        let items = resize_ratio_items(&presets);
        let this = Weak::clone(&self.me);
        menu.show_at(&items, anchor, move |id| {
            let ratio = id
                .parse::<usize>()
                .ok()
                .and_then(|i| presets.get(i))
                .map(|p| p.ratio.value());
            if let (Some(bridge), Some(ratio)) = (this.upgrade(), ratio) {
                bridge.update(|s| s.resize_to_ratio(ratio));
            }
        });
    }
}

/// Runs `f` with the bridge and the open menu's filter while both exist.
fn with_menu(this: &Weak<FiltersBridge>, f: impl Fn(&FiltersBridge, FilterKind) + 'static) -> impl Fn() + 'static {
    let this = Weak::clone(this);
    move || {
        if let Some(bridge) = this.upgrade()
            && let Some(kind) = bridge.menu_kind.get()
        {
            f(&bridge, kind);
        }
    }
}

pub(super) fn connect(bridge: &Rc<FiltersBridge>, window: &MainWindow) {
    let this = Rc::downgrade(bridge);
    let f = window.global::<Filters<'_>>();
    let weak = Weak::clone(&this);
    f.on_open_menu(move |id| {
        if let (Some(bridge), Some(kind)) = (weak.upgrade(), kind_from_id(&id)) {
            bridge.open_menu(kind);
        }
    });
    let weak = Weak::clone(&this);
    f.on_close_menu(move || {
        if let Some(bridge) = weak.upgrade() {
            bridge.close_menu();
        }
    });
    let weak = Weak::clone(&this);
    f.on_set_menu_pinned(move |pinned| {
        if let Some(bridge) = weak.upgrade() {
            bridge.set_menu_pinned(pinned);
        }
    });
    f.on_menu_reset(with_menu(&this, FiltersBridge::menu_reset));
    f.on_menu_save(with_menu(&this, FiltersBridge::menu_save));
    f.on_menu_jump(with_menu(&this, |_, kind| {
        jump::go(JumpTarget::Settings {
            page: Page::Filters,
            key: settings_key(kind),
        });
    }));
    let weak = Weak::clone(&this);
    f.on_analyze_again(move || {
        if let Some(bridge) = weak.upgrade() {
            bridge.analyze_again();
        }
    });
    let weak = Weak::clone(&this);
    f.on_row_menu(move |id, x, y, button_x, button_y| {
        if let (Some(bridge), Some(kind)) = (weak.upgrade(), kind_from_id(&id)) {
            bridge.row_menu(kind, x, y, (button_x, button_y));
        }
    });
    let weak = Weak::clone(&this);
    f.on_resize_ratio_menu(move |x, y, w, h| {
        if let Some(bridge) = weak.upgrade() {
            bridge.resize_ratio_menu([x, y, w, h]);
        }
    });
}
