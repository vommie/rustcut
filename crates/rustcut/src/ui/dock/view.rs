//! The rows the Slint dock shows, built from the layout and the tab being dragged.

use slint::{ModelRc, SharedString};

use super::model::{DockLayout, DropTarget, GroupRef, PanelId, Side};
use super::plan::{self, metrics::TAB_SPACING};
use crate::{DockCardView, DockGroupView, DockRailItem, DockTabView};

/// The cards of the Export panel: id, title, and what the card will hold.
const EXPORT_CARDS: [(&str, &str, &str); 3] = [
    (
        "finalize",
        "Job Finalization",
        "Target folder, file name, counter and saving the job.",
    ),
    (
        "filters",
        "Filters",
        "Crop, deinterlace, resize, rotate, mirror and deshake.",
    ),
    ("sections", "Sections", "The current section and the section list."),
];

/// The cards of the Render Settings panel (M2.5): id and title.
const RENDER_CARDS: [(&str, &str); 5] = [
    ("render-profile", "Profile"),
    ("render-video", "Encoding"),
    ("render-audio", "Audio"),
    ("render-expert", "Expert"),
    ("render-probe", "Probe Render"),
];

/// The tab being dragged, as far as the rows need it.
#[derive(Debug, Clone, Copy)]
pub struct DraggedTab {
    pub panel: PanelId,
    pub width: f32,
    pub target: Option<DropTarget>,
}

/// One group row and the rows of its tab bar (the bridge puts the tabs into the group's own model).
pub struct GroupRows {
    pub group: DockGroupView,
    pub tabs: Vec<DockTabView>,
}

pub fn group_rows(layout: &DockLayout, side: Side, drag: Option<&DraggedTab>) -> Vec<GroupRows> {
    let sidebar = layout.sidebar(side);
    let plan = plan::plan_sidebar(sidebar);
    let placements = &plan.groups;
    sidebar
        .groups
        .iter()
        .zip(placements)
        .enumerate()
        .map(|(index, (group, placement))| {
            let at = GroupRef { side, index };
            let slots = plan::tab_slots(&group.tabs, plan::tabs_area_width(sidebar, index));
            let shifts = shifts(&group.tabs, &slots, at, drag);
            let tabs = group
                .tabs
                .iter()
                .zip(&slots)
                .zip(shifts)
                .map(|((panel, slot), shift)| DockTabView {
                    key: panel.key().into(),
                    title: panel.title().into(),
                    icon: panel.icon().to_slint(),
                    active: *panel == group.active,
                    x: slot.x,
                    width: slot.width,
                    shift,
                    placeholder: drag.is_some_and(|d| d.panel == *panel),
                })
                .collect();
            let next_shows_body = placements.get(index + 1).is_some_and(|p| p.shows_body);
            GroupRows {
                group: DockGroupView {
                    tabs: ModelRc::default(),
                    active_key: group.active.key().into(),
                    shows_body: placement.shows_body,
                    collapsed: group.collapsed,
                    maximized: sidebar.maximized == Some(index),
                    can_maximize: sidebar.groups.len() > 1,
                    first: index == 0,
                    fixed_before: placement.fixed_before,
                    frac_before: placement.frac_before,
                    fixed: placement.fixed,
                    frac: placement.frac,
                    resizable_below: placement.shows_body && next_shows_body,
                },
                tabs,
            }
        })
        .collect()
}

/// How far each tab of the group at `at` moves aside while a tab is dragged over its bar.
fn shifts(tabs: &[PanelId], slots: &[plan::TabSlot], at: GroupRef, drag: Option<&DraggedTab>) -> Vec<f32> {
    let mut shifts = vec![0.0; tabs.len()];
    let Some(drag) = drag else {
        return shifts;
    };
    let Some(DropTarget::Tabs { group, index }) = drag.target else {
        return shifts;
    };
    if group != at {
        return shifts;
    }
    match tabs.iter().position(|p| *p == drag.panel) {
        // A reorder inside the bar: the tabs between the old and the new place close the gap.
        Some(source) => {
            let gap = slots.get(source).map_or(drag.width, |s| s.width) + TAB_SPACING;
            let target = if index > source { index - 1 } else { index };
            for (i, shift) in shifts.iter_mut().enumerate() {
                if source < i && i <= target {
                    *shift = -gap;
                } else if target <= i && i < source {
                    *shift = gap;
                }
            }
        }
        // A tab from elsewhere: the tabs from the insertion point on make room for it.
        None => {
            for shift in shifts.iter_mut().skip(index) {
                *shift = drag.width + TAB_SPACING;
            }
        }
    }
    shifts
}

/// The icons of a collapsed sidebar, group by group.
pub fn rail_rows(layout: &DockLayout, side: Side) -> Vec<DockRailItem> {
    let mut rows = Vec::new();
    for (index, group) in layout.sidebar(side).groups.iter().enumerate() {
        for (tab, panel) in group.tabs.iter().enumerate() {
            rows.push(DockRailItem {
                key: panel.key().into(),
                title: panel.title().into(),
                icon: panel.icon().to_slint(),
                active: *panel == group.active,
                group_start: index > 0 && tab == 0,
            });
        }
    }
    rows
}

pub fn export_cards(layout: &DockLayout) -> Vec<DockCardView> {
    EXPORT_CARDS
        .iter()
        .map(|(id, title, note)| DockCardView {
            id: SharedString::from(*id),
            title: SharedString::from(*title),
            collapsed: layout.collapsed_cards.contains(*id),
            note: SharedString::from(*note),
        })
        .collect()
}

pub fn render_cards(layout: &DockLayout) -> Vec<DockCardView> {
    RENDER_CARDS
        .iter()
        .map(|(id, title)| DockCardView {
            id: SharedString::from(*id),
            title: SharedString::from(*title),
            collapsed: layout.collapsed_cards.contains(*id),
            note: SharedString::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(panel: PanelId, group: GroupRef, index: usize) -> DraggedTab {
        DraggedTab {
            panel,
            width: 100.0,
            target: Some(DropTarget::Tabs { group, index }),
        }
    }

    const RIGHT: GroupRef = GroupRef {
        side: Side::Right,
        index: 0,
    };

    #[test]
    fn a_foreign_tab_pushes_the_tabs_after_the_insertion_point() {
        let layout = DockLayout::default();
        let rows = group_rows(&layout, Side::Right, Some(&drag(PanelId::Queue, RIGHT, 1)));
        let shifts: Vec<f32> = rows[0].tabs.iter().map(|t| t.shift).collect();
        assert_eq!(shifts, [0.0, 104.0, 104.0]);
        assert!(rows[0].tabs.iter().all(|t| !t.placeholder));
    }

    #[test]
    fn a_reorder_closes_the_gap_behind_the_dragged_tab() {
        let layout = DockLayout::default();
        // Tag & Rate (first) dragged behind Logs (insertion index 3).
        let rows = group_rows(&layout, Side::Right, Some(&drag(PanelId::TagRate, RIGHT, 3)));
        let tabs = &rows[0].tabs;
        assert!(tabs[0].placeholder);
        assert_eq!(tabs[0].shift, 0.0);
        assert!(tabs[1].shift < 0.0 && tabs[2].shift < 0.0);
    }

    #[test]
    fn rows_follow_the_plan() {
        let mut layout = DockLayout::default();
        layout.toggle_collapsed(GroupRef {
            side: Side::Left,
            index: 1,
        });
        let rows = group_rows(&layout, Side::Left, None);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].group.first && rows[0].group.can_maximize);
        assert!(!rows[1].group.shows_body);
        assert!(!rows[0].group.resizable_below);
        assert_eq!(rows[0].group.active_key, "export");
    }

    #[test]
    fn the_rail_marks_group_starts() {
        let layout = DockLayout::default();
        let rail = rail_rows(&layout, Side::Left);
        let starts: Vec<bool> = rail.iter().map(|r| r.group_start).collect();
        assert_eq!(starts, [false, false, true]);
        assert!(rail[0].active && !rail[1].active && rail[2].active);
    }

    #[test]
    fn cards_carry_their_collapsed_state() {
        let mut layout = DockLayout::default();
        layout.set_card_collapsed("filters", true);
        let cards = export_cards(&layout);
        assert_eq!(cards.len(), 3);
        assert!(cards[1].collapsed && !cards[0].collapsed);
    }
}
