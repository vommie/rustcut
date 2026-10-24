//! Connects the dock model to the Slint `Dock` global: every callback becomes one model operation, then the rows are
//! published. Rows are updated in place (stable models per sidebar and per group slot), so tabs keep their elements and
//! their displacement animation while a tab is dragged.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::model::{DockLayout, GroupRef, PanelId, Side};
use super::plan::{DockFrame, Hit, Rect, Resize, hit_test, metrics};
use super::view::{self, DraggedTab};
use crate::util::model::sync;
use crate::{Dock, DockCardView, DockGroupView, DockMetrics, DockRailItem, DockTabView, MainWindow};

/// The center keeps at least this width when a sidebar is widened (logical pixels, matches `min-width` in main.slint).
const CENTER_MIN_WIDTH: f32 = 240.0;
/// The two sidebar resizer bars of main.slint.
const SIDEBAR_RESIZERS: f32 = 10.0;

/// The dock of the main window.
pub struct DockBridge {
    window: slint::Weak<MainWindow>,
    state: RefCell<State>,
    left: SideModels,
    right: SideModels,
    export_cards: Rc<VecModel<DockCardView>>,
    render_cards: Rc<VecModel<DockCardView>>,
}

struct State {
    layout: DockLayout,
    drag: Option<Drag>,
    resize: Option<Resize>,
    sidebar_resize: Option<(Side, f32)>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    panel: PanelId,
    grab_x: f32,
    width: f32,
    hit: Option<Hit>,
}

/// The models of one sidebar; tab models are kept per group slot.
struct SideModels {
    groups: Rc<VecModel<DockGroupView>>,
    tabs: RefCell<Vec<Rc<VecModel<DockTabView>>>>,
    rail: Rc<VecModel<DockRailItem>>,
}

impl SideModels {
    fn new() -> Self {
        Self {
            groups: Rc::new(VecModel::default()),
            tabs: RefCell::new(Vec::new()),
            rail: Rc::new(VecModel::default()),
        }
    }
}

impl DockBridge {
    /// Shows `layout` in `window` and handles the dock's callbacks from now on.
    pub fn install(window: &MainWindow, layout: DockLayout) -> Rc<DockBridge> {
        set_metrics(window);
        let bridge = Rc::new(DockBridge {
            window: window.as_weak(),
            state: RefCell::new(State {
                layout,
                drag: None,
                resize: None,
                sidebar_resize: None,
            }),
            left: SideModels::new(),
            right: SideModels::new(),
            export_cards: Rc::new(VecModel::default()),
            render_cards: Rc::new(VecModel::default()),
        });
        let dock = window.global::<Dock<'_>>();
        dock.set_left_groups(ModelRc::from(Rc::clone(&bridge.left.groups)));
        dock.set_right_groups(ModelRc::from(Rc::clone(&bridge.right.groups)));
        dock.set_left_rail(ModelRc::from(Rc::clone(&bridge.left.rail)));
        dock.set_right_rail(ModelRc::from(Rc::clone(&bridge.right.rail)));
        dock.set_export_cards(ModelRc::from(Rc::clone(&bridge.export_cards)));
        dock.set_render_cards(ModelRc::from(Rc::clone(&bridge.render_cards)));
        connect(&dock, &bridge);
        bridge.publish();
        bridge
    }

    /// The current layout (saved at quit).
    pub fn layout(&self) -> DockLayout {
        self.state.borrow().layout.clone()
    }

    /// Applies `change` to the layout and shows the result.
    /// Shows `panel`: its sidebar expands and its tab becomes the active one.
    pub fn reveal(&self, panel: PanelId) {
        self.update(|l| l.reveal(panel));
    }

    /// Shows `panel` with its card `card` unfolded (a jump mark to a row of that card, Q-N6).
    pub fn show_card(&self, panel: PanelId, card: &str) {
        self.update(|l| {
            l.reveal(panel);
            l.set_card_collapsed(card, false);
        });
    }

    fn update(&self, change: impl FnOnce(&mut DockLayout)) {
        change(&mut self.state.borrow_mut().layout);
        self.publish();
        // A folded card or a moved panel may have taken a bubble's field away: the bubbles look again at once.
        crate::ui::notices::layout_changed();
    }

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = self.state.borrow();
        let layout = &state.layout;
        let dragged = state.drag.map(|d| DraggedTab {
            panel: d.panel,
            width: d.width,
            target: d.hit.map(|h| h.target),
        });
        for (side, models) in [(Side::Left, &self.left), (Side::Right, &self.right)] {
            publish_groups(models, view::group_rows(layout, side, dragged.as_ref()));
            sync(&models.rail, view::rail_rows(layout, side));
        }
        sync(&self.export_cards, view::export_cards(layout));
        sync(&self.render_cards, view::render_cards(layout));

        let dock = window.global::<Dock<'_>>();
        let left = super::plan::plan_sidebar(&layout.left);
        let right = super::plan::plan_sidebar(&layout.right);
        dock.set_left_total_fixed(left.total_fixed);
        dock.set_right_total_fixed(right.total_fixed);
        dock.set_left_width(layout.left.width);
        dock.set_right_width(layout.right.width);
        dock.set_left_shown(layout.left.is_shown());
        dock.set_right_shown(layout.right.is_shown());
        dock.set_left_collapsed(layout.left.collapsed);
        dock.set_right_collapsed(layout.right.collapsed);

        dock.set_dragging(state.drag.is_some());
        let highlight = state.drag.and_then(|d| d.hit).map(|h| h.highlight);
        dock.set_drop_visible(highlight.is_some());
        if let Some(r) = highlight {
            dock.set_drop_x(r.x);
            dock.set_drop_y(r.y);
            dock.set_drop_width(r.w);
            dock.set_drop_height(r.h);
        }
    }

    fn frame(&self) -> Option<DockFrame> {
        let w = self.window.upgrade()?;
        Some(DockFrame {
            left: Rect {
                x: w.get_left_frame_x(),
                y: w.get_left_frame_y(),
                w: w.get_left_frame_width(),
                h: w.get_left_frame_height(),
            },
            right: Rect {
                x: w.get_right_frame_x(),
                y: w.get_right_frame_y(),
                w: w.get_right_frame_width(),
                h: w.get_right_frame_height(),
            },
            center: Rect {
                x: w.get_center_frame_x(),
                y: w.get_center_frame_y(),
                w: w.get_center_frame_width(),
                h: w.get_center_frame_height(),
            },
        })
    }

    fn drag_started(&self, key: &str, x: f32, y: f32, grab_x: f32, width: f32) {
        let Some(panel) = PanelId::from_key(key) else {
            return;
        };
        tracing::debug!(panel = key, "[Dock] Tab drag started");
        if let Some(window) = self.window.upgrade() {
            let dock = window.global::<Dock<'_>>();
            dock.set_ghost_title(panel.title().into());
            dock.set_ghost_icon(panel.icon().to_slint());
            dock.set_ghost_width(width);
        }
        self.state.borrow_mut().drag = Some(Drag {
            panel,
            grab_x,
            width,
            hit: None,
        });
        self.drag_moved(x, y);
    }

    fn drag_moved(&self, x: f32, y: f32) {
        let Some(drag) = self.state.borrow().drag else {
            return;
        };
        if let Some(window) = self.window.upgrade() {
            let dock = window.global::<Dock<'_>>();
            dock.set_ghost_x(x - drag.grab_x);
            dock.set_ghost_y(y - metrics::TAB_HEIGHT / 2.0);
        }
        let hit = self
            .frame()
            .and_then(|frame| hit_test(&self.state.borrow().layout, &frame, drag.panel, x, y));
        if hit != drag.hit {
            if let Some(d) = self.state.borrow_mut().drag.as_mut() {
                d.hit = hit;
            }
            self.publish();
        }
    }

    fn drag_ended(&self) {
        let Some(drag) = self.state.borrow_mut().drag.take() else {
            return;
        };
        if let Some(hit) = drag.hit {
            tracing::debug!(panel = drag.panel.key(), target = ?hit.target, "[Dock] Tab dropped");
            self.state.borrow_mut().layout.move_panel(drag.panel, hit.target);
        } else {
            tracing::debug!(panel = drag.panel.key(), "[Dock] Tab drag ended without a target");
        }
        self.publish();
    }

    fn sidebar_resized(&self, side: Side, delta: f32) {
        let Some((resizing, start)) = self.state.borrow().sidebar_resize else {
            return;
        };
        if resizing != side {
            return;
        }
        let wanted = match side {
            Side::Left => start + delta,
            Side::Right => start - delta,
        };
        let available = self.window.upgrade().map_or(f32::MAX, |w| {
            let layout = &self.state.borrow().layout;
            let other = layout.sidebar(match side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
            });
            let other_width = match (other.is_shown(), other.collapsed) {
                (false, _) => 0.0,
                (true, true) => metrics::RAIL_WIDTH,
                (true, false) => other.width,
            };
            w.get_window_width() - other_width - CENTER_MIN_WIDTH - SIDEBAR_RESIZERS
        });
        self.update(|layout| layout.set_sidebar_width(side, wanted.min(available)));
    }

    fn group_resized(&self, side: Side, upper: usize, delta: f32) {
        let Some(resize) = self.state.borrow().resize else {
            return;
        };
        if resize.side == side && resize.upper == upper {
            self.update(|layout| layout.set_weights(side, upper, resize.weights(delta)));
        }
    }
}

/// Wires every callback of the `Dock` global to the bridge (weakly: the window owns the closures).
fn connect(dock: &Dock<'_>, bridge: &Rc<DockBridge>) {
    macro_rules! on {
        ($setter:ident, |$b:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = Rc::downgrade(bridge);
            dock.$setter(move |$($arg),*| {
                if let Some($b) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_tab_clicked, |b, key| with_panel(&key, |p| b
        .update(|l| l.click_tab(p))));
    on!(on_tab_double_clicked, |b, key| with_panel(&key, |p| {
        b.update(|l| {
            if let Some((at, _)) = l.locate(p) {
                l.toggle_maximized(at);
            }
        });
    }));
    on!(on_bar_double_clicked, |b, side, index| with_group(side, index, |at| b
        .update(|l| l.toggle_maximized(at))));
    on!(on_group_collapse_clicked, |b, side, index| with_group(
        side,
        index,
        |at| b.update(|l| l.toggle_collapsed(at))
    ));
    on!(on_group_maximize_clicked, |b, side, index| with_group(
        side,
        index,
        |at| b.update(|l| l.toggle_maximized(at))
    ));
    on!(on_sidebar_collapse_clicked, |b, side| b
        .update(|l| l.set_sidebar_collapsed(to_side(side), true)));
    on!(on_toggle_sidebar, |b, side| b.update(|l| {
        let side = to_side(side);
        let collapsed = l.sidebar(side).collapsed;
        l.set_sidebar_collapsed(side, !collapsed);
    }));
    on!(on_rail_clicked, |b, side, key| {
        match PanelId::from_key(&key) {
            Some(panel) => b.update(|l| l.reveal(panel)),
            None => b.update(|l| l.set_sidebar_collapsed(to_side(side), false)),
        }
    });
    on!(on_tab_drag_started, |b, key, x, y, grab, width| b
        .drag_started(&key, x, y, grab, width));
    on!(on_tab_drag_moved, |b, x, y| b.drag_moved(x, y));
    on!(on_tab_drag_ended, |b| b.drag_ended());
    on!(on_sidebar_resize_started, |b, side| {
        let side = to_side(side);
        let mut state = b.state.borrow_mut();
        let width = state.layout.sidebar(side).width;
        state.sidebar_resize = Some((side, width));
    });
    on!(on_sidebar_resized, |b, side, delta| b
        .sidebar_resized(to_side(side), delta));
    on!(on_group_resize_started, |b, side, upper, height| {
        let mut state = b.state.borrow_mut();
        let resize = usize::try_from(upper)
            .ok()
            .and_then(|upper| Resize::start(&state.layout, to_side(side), upper, height));
        state.resize = resize;
    });
    on!(on_group_resized, |b, side, upper, delta| {
        if let Ok(upper) = usize::try_from(upper) {
            b.group_resized(to_side(side), upper, delta);
        }
    });
    on!(on_card_toggled, |b, id| b.update(|l| {
        let collapsed = l.collapsed_cards.contains(id.as_str());
        l.set_card_collapsed(&id, !collapsed);
    }));
    on!(on_reset_layout, |b| {
        tracing::info!("[Dock] Layout reset to the default");
        b.update(|l| *l = DockLayout::default());
    });
}

fn to_side(side: i32) -> Side {
    if side == 1 { Side::Right } else { Side::Left }
}

fn with_panel(key: &str, f: impl FnOnce(PanelId)) {
    if let Some(panel) = PanelId::from_key(key) {
        f(panel);
    } else {
        tracing::warn!(key, "[Dock] Unknown panel");
    }
}

fn with_group(side: i32, index: i32, f: impl FnOnce(GroupRef)) {
    if let Ok(index) = usize::try_from(index) {
        f(GroupRef {
            side: to_side(side),
            index,
        });
    }
}

fn set_metrics(window: &MainWindow) {
    let m = window.global::<DockMetrics<'_>>();
    m.set_sidebar_padding(metrics::SIDEBAR_PADDING);
    m.set_tab_bar_height(metrics::TAB_BAR_HEIGHT);
    m.set_tab_height(metrics::TAB_HEIGHT);
    m.set_bar_button(metrics::BAR_BUTTON);
    m.set_resizer(metrics::RESIZER);
    m.set_rail_width(metrics::RAIL_WIDTH);
}

/// Puts the group rows into the sidebar's models; each group slot keeps its own tab model.
fn publish_groups(models: &SideModels, rows: Vec<view::GroupRows>) {
    let mut tab_models = models.tabs.borrow_mut();
    while tab_models.len() < rows.len() {
        tab_models.push(Rc::new(VecModel::default()));
    }
    let groups: Vec<DockGroupView> = rows
        .into_iter()
        .zip(tab_models.iter())
        .map(|(row, tabs)| {
            sync(tabs, row.tabs);
            DockGroupView {
                tabs: ModelRc::from(Rc::clone(tabs)),
                ..row.group
            }
        })
        .collect();
    sync(&models.groups, groups);
}
