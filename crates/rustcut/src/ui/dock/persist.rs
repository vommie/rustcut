//! The dock layout in `ui-state.toml` (`UiState.dock`, a table the core stores without knowing it).
//!
//! The stored form is tolerant: panels are names (unknown ones, e.g. from a newer version, are dropped), every field has a
//! default, and the result is sanitized. The table carries its own `version` for future reshaping (none so far).

use serde::{Deserialize, Serialize};

use super::model::{DockLayout, Group, PanelId, Side, Sidebar};

/// Version of the stored dock table.
const DOCK_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredDock {
    version: u32,
    left: Option<StoredSidebar>,
    right: Option<StoredSidebar>,
    collapsed_cards: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredSidebar {
    width: Option<f32>,
    collapsed: bool,
    maximized: Option<usize>,
    groups: Vec<StoredGroup>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredGroup {
    tabs: Vec<String>,
    active: Option<String>,
    weight: Option<f32>,
    collapsed: bool,
}

/// The layout as a table for `UiState.dock`.
pub fn to_table(layout: &DockLayout) -> Option<toml::Table> {
    let stored = StoredDock {
        version: DOCK_VERSION,
        left: Some(store_sidebar(&layout.left)),
        right: Some(store_sidebar(&layout.right)),
        collapsed_cards: layout.collapsed_cards.iter().cloned().collect(),
    };
    match toml::Table::try_from(stored) {
        Ok(table) => Some(table),
        Err(e) => {
            tracing::warn!(error = %e, "[Dock] The layout could not be stored");
            None
        }
    }
}

/// The layout from `UiState.dock`; the default layout without a table or when the table cannot be read.
pub fn from_table(table: Option<&toml::Table>) -> DockLayout {
    let Some(table) = table else {
        return DockLayout::default();
    };
    let stored: StoredDock = match table.clone().try_into() {
        Ok(stored) => stored,
        Err(e) => {
            tracing::warn!(error = %e, "[Dock] The stored layout is unreadable; the default layout is used");
            return DockLayout::default();
        }
    };
    if stored.version > DOCK_VERSION {
        tracing::warn!(
            version = stored.version,
            "[Dock] The layout was stored by a newer version; reading what is known"
        );
    }
    let defaults = DockLayout::default();
    let mut layout = DockLayout {
        left: stored
            .left
            .map_or_else(|| defaults.left.clone(), |s| load_sidebar(s, Side::Left)),
        right: stored
            .right
            .map_or_else(|| defaults.right.clone(), |s| load_sidebar(s, Side::Right)),
        collapsed_cards: stored.collapsed_cards.into_iter().collect(),
    };
    layout.sanitize();
    layout
}

fn store_sidebar(sidebar: &Sidebar) -> StoredSidebar {
    StoredSidebar {
        width: Some(sidebar.width),
        collapsed: sidebar.collapsed,
        maximized: sidebar.maximized,
        groups: sidebar
            .groups
            .iter()
            .map(|g| StoredGroup {
                tabs: g.tabs.iter().map(|p| p.key().to_owned()).collect(),
                active: Some(g.active.key().to_owned()),
                weight: Some(g.weight),
                collapsed: g.collapsed,
            })
            .collect(),
    }
}

fn load_sidebar(stored: StoredSidebar, side: Side) -> Sidebar {
    let default_width = DockLayout::default().sidebar(side).width;
    let mut groups = Vec::new();
    let mut maximized = None;
    for (index, g) in stored.groups.into_iter().enumerate() {
        let tabs: Vec<PanelId> = g.tabs.iter().filter_map(|key| PanelId::from_key(key)).collect();
        let Some(mut group) = Group::new(tabs, g.weight.unwrap_or(1.0)) else {
            continue;
        };
        if stored.maximized == Some(index) {
            maximized = Some(groups.len());
        }
        if let Some(active) = g.active.as_deref().and_then(PanelId::from_key)
            && group.tabs.contains(&active)
        {
            group.active = active;
        }
        group.collapsed = g.collapsed;
        groups.push(group);
    }
    Sidebar {
        groups,
        width: stored.width.unwrap_or(default_width),
        collapsed: stored.collapsed,
        // Follows its group when groups before it were dropped (no known panel left).
        maximized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dock::model::{DropTarget, GroupRef};

    #[test]
    fn no_table_gives_the_default_layout() {
        assert_eq!(from_table(None), DockLayout::default());
    }

    #[test]
    fn a_changed_layout_round_trips() {
        let mut layout = DockLayout::default();
        layout.move_panel(
            PanelId::Logs,
            DropTarget::Split {
                group: GroupRef {
                    side: Side::Left,
                    index: 1,
                },
                below: true,
            },
        );
        layout.set_sidebar_width(Side::Right, 400.0);
        layout.left.collapsed = true;
        layout.toggle_maximized(GroupRef {
            side: Side::Left,
            index: 0,
        });
        layout.set_card_collapsed("filters", true);
        let table = to_table(&layout).unwrap();
        let text = toml::to_string(&table).unwrap();
        let read: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(from_table(Some(&read)), layout);
    }

    #[test]
    fn unknown_panels_and_fields_are_ignored_and_missing_ones_return() {
        let text = r#"
            version = 1
            future_field = "x"
            [left]
            width = 250.0
            [[left.groups]]
            tabs = ["queue", "timeline-v9", "queue"]
            active = "timeline-v9"
            weight = -3.0
        "#;
        let table: toml::Table = toml::from_str(text).unwrap();
        let layout = from_table(Some(&table));
        assert!(layout.is_valid(), "{layout:?}");
        assert_eq!(layout.left.width, 250.0);
        let first = &layout.left.groups[0];
        assert_eq!(first.tabs[0], PanelId::Queue);
        assert_eq!(first.active, PanelId::Queue);
        assert_eq!(first.weight, 1.0);
        // The right sidebar was not in the file: the default one is used.
        assert_eq!(layout.right, DockLayout::default().right);
    }

    #[test]
    fn a_layout_of_0_0_12_gets_the_job_panel_next_to_the_codecs_panel() {
        // 0.0.12 had no Job panel; the user had moved the Codecs panel (render-settings) to the right.
        let text = r#"
            version = 1
            [[left.groups]]
            tabs = ["export"]
            [[left.groups]]
            tabs = ["queue"]
            [[right.groups]]
            tabs = ["tag-rate", "render-settings", "media-info", "logs"]
            active = "media-info"
        "#;
        let table: toml::Table = toml::from_str(text).unwrap();
        let layout = from_table(Some(&table));
        assert!(layout.is_valid(), "{layout:?}");
        assert_eq!(
            layout.right.groups[0].tabs,
            [
                PanelId::TagRate,
                PanelId::RenderSettings,
                PanelId::Job,
                PanelId::MediaInfo,
                PanelId::Logs
            ]
        );
        assert_eq!(
            layout.right.groups[0].active,
            PanelId::MediaInfo,
            "the active tab stays"
        );
    }

    #[test]
    fn a_table_of_the_wrong_shape_gives_the_default_layout() {
        let table: toml::Table = toml::from_str("left = 5").unwrap();
        assert_eq!(from_table(Some(&table)), DockLayout::default());
    }

    #[test]
    fn widths_out_of_range_are_clamped() {
        let table: toml::Table = toml::from_str("[left]\nwidth = 5.0\n[right]\nwidth = 99999.0").unwrap();
        let layout = from_table(Some(&table));
        assert!(layout.is_valid());
        assert_eq!(layout.left.width, crate::ui::dock::model::MIN_SIDEBAR_WIDTH);
        assert_eq!(layout.right.width, crate::ui::dock::model::MAX_SIDEBAR_WIDTH);
    }
}
