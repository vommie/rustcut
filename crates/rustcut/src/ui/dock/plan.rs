//! Geometry of the dock: one plan that Slint applies and Rust hit-tests, so a drop lands where the highlight showed it.
//!
//! A sidebar is a column of groups with resizer bars between them. Each group gets a fixed part (its tab bar, plus a
//! minimum body when it is expanded) and a share of the free height ("extra") proportional to its weight:
//! `y = fixed_before + frac_before * extra`, `height = fixed + frac * extra`, `extra = max(0, sidebar height - total_fixed)`.
//! Slint evaluates exactly these formulas with the sidebar's real height; Rust uses the same numbers for drops and resizes.
//! Tab positions are computed here too (estimated text widths), and Slint places tabs at these positions.

use super::model::{DockLayout, DropTarget, GroupRef, PanelId, Side, Sidebar};

/// Sizes shared with the Slint side (set into the `DockMetrics` global at startup), in logical pixels.
pub mod metrics {
    pub const SIDEBAR_PADDING: f32 = 6.0;
    /// Above the first group: none, so the sidebars start on the line of the video's top edge (P9.12, the user's wish
    /// with the new menu bar).
    pub const SIDEBAR_TOP_PADDING: f32 = 0.0;
    pub const TAB_BAR_HEIGHT: f32 = 30.0;
    pub const TAB_HEIGHT: f32 = 26.0;
    pub const TAB_SPACING: f32 = 4.0;
    /// Narrowest tab (icon only) and widest tab.
    pub const TAB_MIN_WIDTH: f32 = 30.0;
    pub const TAB_MAX_WIDTH: f32 = 140.0;
    /// Square buttons at the end of a tab bar (collapse, maximize; the first group also collapses the sidebar).
    pub const BAR_BUTTON: f32 = 22.0;
    pub const BAR_BUTTON_SPACING: f32 = 2.0;
    /// Space between the group's edge and the first tab, and between the last button and the edge.
    pub const TAB_BAR_INSET: f32 = 4.0;
    pub const RESIZER: f32 = 6.0;
    /// Smallest body of an expanded group.
    pub const MIN_BODY: f32 = 80.0;
    pub const RAIL_WIDTH: f32 = 36.0;
    /// Drop strip at the window edge for a sidebar without groups.
    pub const EDGE_STRIP: f32 = 56.0;
    /// Bands at the top and bottom of a group body that split a new group off (at most this, at most 30 % of the body).
    pub const SPLIT_BAND: f32 = 56.0;
}

use metrics::{
    BAR_BUTTON, BAR_BUTTON_SPACING, EDGE_STRIP, MIN_BODY, RESIZER, SIDEBAR_PADDING, SIDEBAR_TOP_PADDING, SPLIT_BAND,
    TAB_BAR_HEIGHT, TAB_BAR_INSET, TAB_MAX_WIDTH, TAB_MIN_WIDTH, TAB_SPACING,
};

/// Vertical placement of one group inside its sidebar (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub fixed_before: f32,
    pub frac_before: f32,
    pub fixed: f32,
    pub frac: f32,
    /// The body is shown (expanded and not hidden by another maximized group).
    pub shows_body: bool,
}

impl Placement {
    pub fn y(&self, extra: f32) -> f32 {
        self.fixed_before + self.frac_before * extra
    }

    pub fn height(&self, extra: f32) -> f32 {
        self.fixed + self.frac * extra
    }
}

/// The plan of one sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarPlan {
    pub groups: Vec<Placement>,
    /// Everything that does not stretch: paddings, tab bars, minimum bodies, resizers.
    pub total_fixed: f32,
}

impl SidebarPlan {
    pub fn extra(&self, height: f32) -> f32 {
        (height - self.total_fixed).max(0.0)
    }
}

/// Places the groups of `sidebar`.
pub fn plan_sidebar(sidebar: &Sidebar) -> SidebarPlan {
    let shows_body: Vec<bool> = sidebar
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| match sidebar.maximized {
            Some(m) => m == i,
            None => !g.collapsed,
        })
        .collect();
    let weights: Vec<f32> = sidebar
        .groups
        .iter()
        .zip(&shows_body)
        .map(|(g, shows)| if *shows { g.weight.max(0.0) } else { 0.0 })
        .collect();
    let total_weight: f32 = weights.iter().sum();
    let expanded = shows_body.iter().filter(|s| **s).count();

    let mut groups = Vec::with_capacity(sidebar.groups.len());
    let mut fixed_before = SIDEBAR_TOP_PADDING;
    let mut frac_before = 0.0;
    for (shows, weight) in shows_body.iter().zip(&weights) {
        let fixed = if *shows {
            TAB_BAR_HEIGHT + MIN_BODY
        } else {
            TAB_BAR_HEIGHT
        };
        let frac = match (*shows, total_weight > 0.0) {
            (false, _) => 0.0,
            (true, true) => weight / total_weight,
            (true, false) => 1.0 / expanded as f32,
        };
        groups.push(Placement {
            fixed_before,
            frac_before,
            fixed,
            frac,
            shows_body: *shows,
        });
        fixed_before += fixed + RESIZER;
        frac_before += frac;
    }
    let total_fixed = if groups.is_empty() {
        SIDEBAR_TOP_PADDING + SIDEBAR_PADDING
    } else {
        fixed_before - RESIZER + SIDEBAR_PADDING
    };
    SidebarPlan { groups, total_fixed }
}

/// Width a tab would like: icon, paddings and its title in the bold 11 px tab font (the active tab is bold; every tab is
/// sized for bold so a tab does not grow when it becomes active). Character widths are estimated by class.
pub fn preferred_tab_width(title: &str) -> f32 {
    let text: f32 = title
        .chars()
        .map(|c| match c {
            ' ' => 3.2,
            'i' | 'j' | 'l' | 't' | 'f' | 'r' | '.' | ',' | '\'' => 4.0,
            'm' | 'w' | '&' | '%' => 8.6,
            c if c.is_uppercase() => 8.6,
            _ => 7.0,
        })
        .sum();
    // Paddings 2 x 8, icon 16, spacing 6, and a little room so the text never elides at its preferred width.
    (16.0 + 16.0 + 6.0 + 4.0 + text).clamp(60.0, TAB_MAX_WIDTH)
}

/// The buttons at the end of a group's tab bar: collapse, maximize (with a second group), collapse the sidebar (first group).
pub fn bar_buttons(sidebar: &Sidebar, index: usize) -> usize {
    1 + usize::from(sidebar.groups.len() > 1) + usize::from(index == 0)
}

/// Width for the tabs of a group's bar: the group's width minus the insets and the buttons at its end.
pub fn tabs_area_width(sidebar: &Sidebar, index: usize) -> f32 {
    let buttons = bar_buttons(sidebar, index) as f32;
    let group_width = sidebar.width - 2.0 * SIDEBAR_PADDING;
    (group_width - 2.0 * TAB_BAR_INSET - buttons * (BAR_BUTTON + BAR_BUTTON_SPACING)).max(0.0)
}

/// Horizontal position and width of one tab, relative to the start of its tab bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSlot {
    pub x: f32,
    pub width: f32,
}

/// Tabs side by side at their preferred widths. When they do not fit `area`, the widest ones shrink first (all tabs above
/// a common cap are cut to it), down to icon-only tabs.
pub fn tab_slots(panels: &[PanelId], area: f32) -> Vec<TabSlot> {
    let preferred: Vec<f32> = panels.iter().map(|p| preferred_tab_width(p.title())).collect();
    let spacing = TAB_SPACING * panels.len().saturating_sub(1) as f32;
    let cap = width_cap(&preferred, (area - spacing).max(0.0));
    let mut x = 0.0;
    preferred
        .into_iter()
        .map(|w| {
            let width = w.min(cap).max(TAB_MIN_WIDTH);
            let slot = TabSlot { x, width };
            x += width + TAB_SPACING;
            slot
        })
        .collect()
}

/// The largest cap with `sum(min(width, cap)) <= available` (no cap when everything fits).
fn width_cap(widths: &[f32], available: f32) -> f32 {
    if widths.iter().sum::<f32>() <= available {
        return f32::MAX;
    }
    let mut sorted = widths.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mut used = 0.0;
    for (i, w) in sorted.iter().enumerate() {
        let rest = (sorted.len() - i) as f32;
        let cap = (available - used) / rest;
        if cap <= *w {
            return cap.max(0.0);
        }
        used += w;
    }
    f32::MAX
}

/// Where a tab dropped at `x` (relative to the tab bar) is inserted: before the first tab whose center is right of `x`.
pub fn insertion_index(slots: &[TabSlot], x: f32) -> usize {
    slots.iter().take_while(|s| s.x + s.width / 2.0 < x).count()
}

/// A rectangle in window coordinates (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Where the sidebars and the center are in the window (read from Slint while dragging).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DockFrame {
    pub left: Rect,
    pub right: Rect,
    pub center: Rect,
}

impl DockFrame {
    fn sidebar(&self, side: Side) -> Rect {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }
}

/// The drop under the cursor and what to highlight for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub target: DropTarget,
    /// Area that lights up.
    pub highlight: Rect,
}

/// The rectangle of a group in window coordinates.
pub fn group_rect(sidebar: &Sidebar, frame: Rect, index: usize) -> Option<Rect> {
    let plan = plan_sidebar(sidebar);
    let extra = plan.extra(frame.h);
    let p = plan.groups.get(index)?;
    Some(Rect {
        x: frame.x + SIDEBAR_PADDING,
        y: frame.y + p.y(extra),
        w: (frame.w - 2.0 * SIDEBAR_PADDING).max(0.0),
        h: p.height(extra),
    })
}

/// The drop target for `panel` at window point (`x`, `y`). Targets that would not change the layout are not offered.
pub fn hit_test(layout: &DockLayout, frame: &DockFrame, panel: PanelId, x: f32, y: f32) -> Option<Hit> {
    let hit = Side::BOTH
        .into_iter()
        .find_map(|side| hit_sidebar(layout, frame, side, x, y))?;
    let mut probe = layout.clone();
    (probe.move_panel(panel, hit.target) && probe != *layout).then_some(hit)
}

fn hit_sidebar(layout: &DockLayout, frame: &DockFrame, side: Side, x: f32, y: f32) -> Option<Hit> {
    let sidebar = layout.sidebar(side);
    let rect = frame.sidebar(side);
    if !sidebar.is_shown() {
        let strip = match side {
            Side::Left => Rect {
                w: EDGE_STRIP,
                ..frame.center
            },
            Side::Right => Rect {
                x: frame.center.x + frame.center.w - EDGE_STRIP,
                w: EDGE_STRIP,
                ..frame.center
            },
        };
        return strip.contains(x, y).then_some(Hit {
            target: DropTarget::Sidebar(side),
            highlight: strip,
        });
    }
    if !rect.contains(x, y) {
        return None;
    }
    if sidebar.collapsed {
        return Some(Hit {
            target: DropTarget::Sidebar(side),
            highlight: rect,
        });
    }
    let plan = plan_sidebar(sidebar);
    let extra = plan.extra(rect.h);
    for (index, (placement, group)) in plan.groups.iter().zip(&sidebar.groups).enumerate() {
        let Some(g) = group_rect(sidebar, rect, index) else {
            continue;
        };
        let half_gap = RESIZER / 2.0;
        if y < g.y - half_gap || y >= g.bottom() + half_gap {
            continue;
        }
        let at = GroupRef { side, index };
        let bar = Rect { h: TAB_BAR_HEIGHT, ..g };
        if y < bar.bottom() || !placement.shows_body {
            let slots = tab_slots(&group.tabs, tabs_area_width(sidebar, index));
            let tab_index = insertion_index(&slots, x - bar.x - TAB_BAR_INSET);
            return Some(Hit {
                target: DropTarget::Tabs {
                    group: at,
                    index: tab_index,
                },
                highlight: bar,
            });
        }
        let body = Rect {
            y: bar.bottom(),
            h: placement.height(extra) - TAB_BAR_HEIGHT,
            ..g
        };
        let band = SPLIT_BAND.min(body.h * 0.3);
        let half = g.h / 2.0;
        if y < body.y + band {
            return Some(Hit {
                target: DropTarget::Split {
                    group: at,
                    below: false,
                },
                highlight: Rect { h: half, ..g },
            });
        }
        if y >= body.bottom() - band {
            return Some(Hit {
                target: DropTarget::Split { group: at, below: true },
                highlight: Rect {
                    y: g.y + half,
                    h: half,
                    ..g
                },
            });
        }
        return Some(Hit {
            target: DropTarget::Tabs {
                group: at,
                index: group.tabs.len(),
            },
            highlight: g,
        });
    }
    // Below the last group: a new group at the end, if there is room to show where it goes.
    let last_bottom = plan
        .groups
        .last()
        .map_or(rect.y + SIDEBAR_TOP_PADDING, |p| rect.y + p.y(extra) + p.height(extra));
    let free = Rect {
        x: rect.x + SIDEBAR_PADDING,
        y: last_bottom + RESIZER,
        w: (rect.w - 2.0 * SIDEBAR_PADDING).max(0.0),
        h: rect.bottom() - SIDEBAR_PADDING - last_bottom - RESIZER,
    };
    (free.h >= TAB_BAR_HEIGHT && free.contains(x, y)).then_some(Hit {
        target: DropTarget::Sidebar(side),
        highlight: free,
    })
}

/// A resizer drag between groups `upper` and `upper + 1`, measured when the drag started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resize {
    pub side: Side,
    pub upper: usize,
    above: f32,
    below: f32,
    min_above: f32,
    min_below: f32,
    weight_sum: f32,
}

impl Resize {
    /// Starts a resize; `None` unless both groups show their bodies (a collapsed group has no height to give).
    pub fn start(layout: &DockLayout, side: Side, upper: usize, height: f32) -> Option<Resize> {
        let sidebar = layout.sidebar(side);
        let plan = plan_sidebar(sidebar);
        let extra = plan.extra(height);
        let a = plan.groups.get(upper)?;
        let b = plan.groups.get(upper + 1)?;
        if !a.shows_body || !b.shows_body {
            return None;
        }
        let weight_sum = sidebar.groups.get(upper)?.weight + sidebar.groups.get(upper + 1)?.weight;
        Some(Resize {
            side,
            upper,
            above: a.height(extra),
            below: b.height(extra),
            min_above: a.fixed,
            min_below: b.fixed,
            weight_sum: if weight_sum > 0.0 { weight_sum } else { 1.0 },
        })
    }

    /// The two weights after moving the bar by `delta` pixels (positive = down), each group kept at its minimum.
    pub fn weights(&self, delta: f32) -> (f32, f32) {
        let total = self.above + self.below;
        let above = (self.above + delta).clamp(self.min_above, (total - self.min_below).max(self.min_above));
        let extra_above = above - self.min_above;
        let extra_below = (total - above - self.min_below).max(0.0);
        let free = extra_above + extra_below;
        if free <= 0.0 {
            return (self.weight_sum / 2.0, self.weight_sum / 2.0);
        }
        let upper = self.weight_sum * extra_above / free;
        (upper, self.weight_sum - upper)
    }
}
