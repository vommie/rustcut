//! The dock layout (ADR-12): two sidebars, each a vertical stack of panel groups; a group is a tab bar over its active panel.
//!
//! Pure data and operations, no Slint: every user action is one method, and after every method the layout is valid
//! ([`DockLayout::is_valid`]): each panel exists exactly once, no group is empty, a group's active panel is one of its tabs,
//! weights are finite and not negative, widths are in range and a maximized group exists. [`DockLayout::sanitize`]
//! restores these rules after loading a file written by another version or edited by hand.

use std::collections::BTreeSet;

use crate::icons::Icon;

/// Narrowest and widest sidebar in logical pixels.
pub const MIN_SIDEBAR_WIDTH: f32 = 220.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 720.0;
const DEFAULT_LEFT_WIDTH: f32 = 330.0;
const DEFAULT_RIGHT_WIDTH: f32 = 330.0;

/// A dock panel. Each one exists exactly once in the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PanelId {
    Export,
    RenderSettings,
    Queue,
    TagRate,
    MediaInfo,
    Logs,
}

impl PanelId {
    pub const ALL: [PanelId; 6] = [
        PanelId::Export,
        PanelId::RenderSettings,
        PanelId::Queue,
        PanelId::TagRate,
        PanelId::MediaInfo,
        PanelId::Logs,
    ];

    /// Stable name in `ui-state.toml` and in the Slint models.
    pub fn key(self) -> &'static str {
        match self {
            PanelId::Export => "export",
            PanelId::RenderSettings => "render-settings",
            PanelId::Queue => "queue",
            PanelId::TagRate => "tag-rate",
            PanelId::MediaInfo => "media-info",
            PanelId::Logs => "logs",
        }
    }

    pub fn from_key(key: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|panel| panel.key() == key)
    }

    /// Title on the tab.
    pub fn title(self) -> &'static str {
        match self {
            PanelId::Export => "Export",
            PanelId::RenderSettings => "Render Settings",
            PanelId::Queue => "Queue",
            PanelId::TagRate => "Tag & Rate",
            PanelId::MediaInfo => "Media Info",
            PanelId::Logs => "Logs",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            PanelId::Export => Icon::PanelExport,
            PanelId::RenderSettings => Icon::PanelRenderSettings,
            PanelId::Queue => Icon::PanelQueue,
            PanelId::TagRate => Icon::PanelTagRate,
            PanelId::MediaInfo => Icon::PanelMediaInfo,
            PanelId::Logs => Icon::PanelLogs,
        }
    }

    /// The sidebar the panel lives in by default (where a panel missing from a file is put back).
    pub fn default_side(self) -> Side {
        match self {
            PanelId::Export | PanelId::RenderSettings | PanelId::Queue => Side::Left,
            PanelId::TagRate | PanelId::MediaInfo | PanelId::Logs => Side::Right,
        }
    }
}

/// One of the two dock areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const BOTH: [Side; 2] = [Side::Left, Side::Right];
}

/// Address of a group: its sidebar and its position from the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupRef {
    pub side: Side,
    pub index: usize,
}

/// A tab bar with its panels; shows the active one.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub tabs: Vec<PanelId>,
    pub active: PanelId,
    /// Share of the sidebar's free height relative to the other expanded groups.
    pub weight: f32,
    /// Only the tab bar is shown.
    pub collapsed: bool,
}

impl Group {
    pub fn new(tabs: Vec<PanelId>, weight: f32) -> Option<Group> {
        let active = *tabs.first()?;
        Some(Group {
            tabs,
            active,
            weight,
            collapsed: false,
        })
    }
}

/// A dock area.
#[derive(Debug, Clone, PartialEq)]
pub struct Sidebar {
    pub groups: Vec<Group>,
    /// Width in logical pixels while expanded.
    pub width: f32,
    /// Shown as a slim rail of panel icons.
    pub collapsed: bool,
    /// The group that fills the sidebar while the others show only their tab bars.
    pub maximized: Option<usize>,
}

impl Sidebar {
    fn new(groups: Vec<Group>, width: f32) -> Sidebar {
        Sidebar {
            groups,
            width,
            collapsed: false,
            maximized: None,
        }
    }

    /// Whether the sidebar takes space (a sidebar without groups is hidden, also as a rail).
    pub fn is_shown(&self) -> bool {
        !self.groups.is_empty()
    }
}

/// Where a dragged tab is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// Into a group's tab bar before tab `index` (`index` >= the tab count appends).
    Tabs { group: GroupRef, index: usize },
    /// A new group of its own above (`below == false`) or below the group.
    Split { group: GroupRef, below: bool },
    /// A new group at the end of a sidebar (the only way into a sidebar without groups).
    Sidebar(Side),
}

/// The complete dock layout.
#[derive(Debug, Clone, PartialEq)]
pub struct DockLayout {
    pub left: Sidebar,
    pub right: Sidebar,
    /// Cards inside panels (Export: Job Finalization, Filters, Sections) that show only their header.
    pub collapsed_cards: BTreeSet<String>,
}

impl Default for DockLayout {
    /// The arrangement of the user's PyCutRv screenshot (ADR-12).
    fn default() -> Self {
        let group = |tabs: Vec<PanelId>, weight: f32| Group {
            active: tabs.first().copied().unwrap_or(PanelId::Export),
            tabs,
            weight,
            collapsed: false,
        };
        DockLayout {
            left: Sidebar::new(
                vec![
                    group(vec![PanelId::Export, PanelId::RenderSettings], 3.0),
                    group(vec![PanelId::Queue], 2.0),
                ],
                DEFAULT_LEFT_WIDTH,
            ),
            right: Sidebar::new(
                vec![group(vec![PanelId::TagRate, PanelId::MediaInfo, PanelId::Logs], 1.0)],
                DEFAULT_RIGHT_WIDTH,
            ),
            collapsed_cards: BTreeSet::new(),
        }
    }
}

impl DockLayout {
    pub fn sidebar(&self, side: Side) -> &Sidebar {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    pub fn sidebar_mut(&mut self, side: Side) -> &mut Sidebar {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub fn group(&self, group: GroupRef) -> Option<&Group> {
        self.sidebar(group.side).groups.get(group.index)
    }

    fn group_mut(&mut self, group: GroupRef) -> Option<&mut Group> {
        self.sidebar_mut(group.side).groups.get_mut(group.index)
    }

    /// The group that holds `panel` and the panel's tab position.
    pub fn locate(&self, panel: PanelId) -> Option<(GroupRef, usize)> {
        Side::BOTH.into_iter().find_map(|side| {
            self.sidebar(side).groups.iter().enumerate().find_map(|(index, group)| {
                let tab = group.tabs.iter().position(|p| *p == panel)?;
                Some((GroupRef { side, index }, tab))
            })
        })
    }

    /// A click on a tab: an inactive tab becomes active (and its group expands); the active tab collapses or expands
    /// its group, or restores the sidebar when its group is maximized.
    pub fn click_tab(&mut self, panel: PanelId) {
        let Some((at, _)) = self.locate(panel) else {
            return;
        };
        let sidebar = self.sidebar_mut(at.side);
        let Some(group) = sidebar.groups.get_mut(at.index) else {
            return;
        };
        if group.active != panel {
            group.active = panel;
            group.collapsed = false;
            if sidebar.maximized.is_some_and(|m| m != at.index) {
                sidebar.maximized = None;
            }
        } else if sidebar.maximized == Some(at.index) {
            sidebar.maximized = None;
        } else {
            group.collapsed = !group.collapsed;
        }
    }

    /// Shows `panel`: active in its group, the group expanded and visible, the sidebar expanded (rail icon, menu).
    pub fn reveal(&mut self, panel: PanelId) {
        let Some((at, _)) = self.locate(panel) else {
            return;
        };
        let sidebar = self.sidebar_mut(at.side);
        sidebar.collapsed = false;
        if sidebar.maximized.is_some_and(|m| m != at.index) {
            sidebar.maximized = None;
        }
        if let Some(group) = sidebar.groups.get_mut(at.index) {
            group.active = panel;
            group.collapsed = false;
        }
    }

    /// The chevron of a group: collapse or expand it (a maximized group is restored instead).
    pub fn toggle_collapsed(&mut self, at: GroupRef) {
        let sidebar = self.sidebar_mut(at.side);
        if sidebar.maximized == Some(at.index) {
            sidebar.maximized = None;
        } else if let Some(group) = sidebar.groups.get_mut(at.index) {
            group.collapsed = !group.collapsed;
        }
    }

    /// Double click on a tab bar: the group fills its sidebar, or the sidebar is restored. Needs a second group.
    pub fn toggle_maximized(&mut self, at: GroupRef) {
        let sidebar = self.sidebar_mut(at.side);
        if sidebar.groups.len() < 2 || at.index >= sidebar.groups.len() {
            return;
        }
        if sidebar.maximized == Some(at.index) {
            sidebar.maximized = None;
        } else {
            sidebar.maximized = Some(at.index);
            if let Some(group) = sidebar.groups.get_mut(at.index) {
                group.collapsed = false;
            }
        }
    }

    pub fn set_sidebar_collapsed(&mut self, side: Side, collapsed: bool) {
        self.sidebar_mut(side).collapsed = collapsed;
    }

    /// Sets a sidebar's width, kept between [`MIN_SIDEBAR_WIDTH`] and [`MAX_SIDEBAR_WIDTH`].
    pub fn set_sidebar_width(&mut self, side: Side, width: f32) {
        if width.is_finite() {
            self.sidebar_mut(side).width = width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        }
    }

    /// Sets the weights of two neighboring groups (a resizer drag, computed by `plan::resize_weights`).
    pub fn set_weights(&mut self, side: Side, upper: usize, weights: (f32, f32)) {
        let valid = |w: f32| w.is_finite() && w >= 0.0;
        if !valid(weights.0) || !valid(weights.1) {
            return;
        }
        let groups = &mut self.sidebar_mut(side).groups;
        let Some([above, below]) = groups.get_mut(upper..upper.saturating_add(2)) else {
            return;
        };
        above.weight = weights.0;
        below.weight = weights.1;
    }

    pub fn set_card_collapsed(&mut self, card: &str, collapsed: bool) {
        if collapsed {
            self.collapsed_cards.insert(card.to_owned());
        } else {
            self.collapsed_cards.remove(card);
        }
    }

    /// Moves `panel` to `target`. Returns `false` when nothing changed (the target does not exist, or the panel would
    /// be split off its own single-tab group).
    pub fn move_panel(&mut self, panel: PanelId, target: DropTarget) -> bool {
        let Some((from, from_tab)) = self.locate(panel) else {
            return false;
        };
        let target_exists = match target {
            DropTarget::Tabs { group, .. } | DropTarget::Split { group, .. } => self.group(group).is_some(),
            DropTarget::Sidebar(_) => true,
        };
        if !target_exists {
            return false;
        }
        let single = self.group(from).is_some_and(|g| g.tabs.len() == 1);
        match target {
            DropTarget::Split { group, .. } if group == from && single => return false,
            DropTarget::Sidebar(side)
                if side == from.side && single && from.index + 1 == self.sidebar(side).groups.len() =>
            {
                return false;
            }
            _ => {}
        }

        // Take the tab out; an emptied group stays until the end so every GroupRef keeps pointing at the same group.
        if let Some(source) = self.group_mut(from) {
            source.tabs.retain(|p| *p != panel);
            if source.active == panel
                && let Some(next) = source.tabs.get(from_tab.min(source.tabs.len().saturating_sub(1)))
            {
                source.active = *next;
            }
        }

        match target {
            DropTarget::Tabs { group, index } => {
                let index = if group == from && index > from_tab {
                    index - 1
                } else {
                    index
                };
                if let Some(g) = self.group_mut(group) {
                    g.tabs.insert(index.min(g.tabs.len()), panel);
                    g.active = panel;
                    g.collapsed = false;
                }
            }
            DropTarget::Split { group, below } => {
                let sidebar = self.sidebar_mut(group.side);
                let mut weight = 1.0;
                if let Some(g) = sidebar.groups.get_mut(group.index) {
                    g.weight /= 2.0;
                    weight = g.weight;
                }
                let at = if below { group.index + 1 } else { group.index };
                if let Some(new) = Group::new(vec![panel], weight) {
                    sidebar.groups.insert(at.min(sidebar.groups.len()), new);
                }
            }
            DropTarget::Sidebar(side) => {
                let sidebar = self.sidebar_mut(side);
                let weight = mean_weight(&sidebar.groups);
                if let Some(new) = Group::new(vec![panel], weight) {
                    sidebar.groups.push(new);
                }
            }
        }

        let to_side = match target {
            DropTarget::Tabs { group, .. } | DropTarget::Split { group, .. } => group.side,
            DropTarget::Sidebar(side) => side,
        };
        for side in [from.side, to_side] {
            let sidebar = self.sidebar_mut(side);
            sidebar.groups.retain(|g| !g.tabs.is_empty());
            sidebar.maximized = None;
        }
        self.sidebar_mut(to_side).collapsed = false;
        true
    }

    /// Whether every rule of the module docs holds.
    pub fn is_valid(&self) -> bool {
        let mut seen = BTreeSet::new();
        for side in Side::BOTH {
            let sidebar = self.sidebar(side);
            if !(MIN_SIDEBAR_WIDTH..=MAX_SIDEBAR_WIDTH).contains(&sidebar.width) {
                return false;
            }
            if sidebar.maximized.is_some_and(|m| m >= sidebar.groups.len()) {
                return false;
            }
            for group in &sidebar.groups {
                if group.tabs.is_empty() || !group.tabs.contains(&group.active) {
                    return false;
                }
                if !group.weight.is_finite() || group.weight < 0.0 {
                    return false;
                }
                if !group.tabs.iter().all(|p| seen.insert(*p)) {
                    return false;
                }
            }
        }
        seen.len() == PanelId::ALL.len()
    }

    /// Repairs a layout read from a file: duplicate panels are dropped, missing ones return to their default sidebar,
    /// empty groups disappear, actives, weights, widths and the maximized group are made valid.
    pub fn sanitize(&mut self) {
        let mut seen = BTreeSet::new();
        for side in Side::BOTH {
            let sidebar = self.sidebar_mut(side);
            for group in &mut sidebar.groups {
                group.tabs.retain(|p| seen.insert(*p));
                if !group.tabs.contains(&group.active)
                    && let Some(first) = group.tabs.first()
                {
                    group.active = *first;
                }
                if !group.weight.is_finite() || group.weight < 0.0 {
                    group.weight = 1.0;
                }
            }
            let maximized = sidebar.maximized.and_then(|m| sidebar.groups.get(m)).cloned();
            sidebar.groups.retain(|g| !g.tabs.is_empty());
            sidebar.maximized = maximized.and_then(|m| sidebar.groups.iter().position(|g| *g == m));
            if sidebar.groups.len() < 2 {
                sidebar.maximized = None;
            }
            let width = if sidebar.width.is_finite() {
                sidebar.width
            } else {
                match side {
                    Side::Left => DEFAULT_LEFT_WIDTH,
                    Side::Right => DEFAULT_RIGHT_WIDTH,
                }
            };
            sidebar.width = width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        }
        for panel in PanelId::ALL {
            if seen.contains(&panel) {
                continue;
            }
            let sidebar = self.sidebar_mut(panel.default_side());
            match sidebar.groups.last_mut() {
                Some(group) => group.tabs.push(panel),
                None => sidebar.groups.extend(Group::new(vec![panel], 1.0)),
            }
        }
    }
}

/// Average weight of the groups (1 for none), the weight of a group added at the end.
fn mean_weight(groups: &[Group]) -> f32 {
    if groups.is_empty() {
        return 1.0;
    }
    let sum: f32 = groups.iter().map(|g| g.weight).sum();
    let mean = sum / groups.len() as f32;
    if mean.is_finite() && mean > 0.0 { mean } else { 1.0 }
}
