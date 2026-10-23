//! The queue list's columns (HANDOFF 8.Q): which ones fit into the panel's width and where they sit. In a narrow
//! sidebar the list shows compact two-line rows; as the panel gets wider (or its group is maximized) it becomes a
//! table, and columns join in the order of their priority. The name stretches; the other columns keep the widths the
//! user dragged them to (`UiState.queue.columns`). Slint draws exactly these numbers.

use std::collections::BTreeMap;

/// What a column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Column {
    Id,
    Name,
    State,
    Eta,
    Length,
    Size,
    Profile,
    Folder,
}

/// Left to right.
pub const ORDER: [Column; 8] = [
    Column::Id,
    Column::Name,
    Column::State,
    Column::Eta,
    Column::Length,
    Column::Size,
    Column::Profile,
    Column::Folder,
];

/// The optional columns, the first one left out first when the panel gets narrower. Without room for the ETA and
/// the length, the compact rows say more than a table could.
const DROP_ORDER: [Column; 4] = [Column::Folder, Column::Profile, Column::Size, Column::Id];

/// The name column is never narrower than this; below it (with every optional column gone) the rows turn compact.
pub const NAME_MIN: u32 = 160;
/// Widths a dragged column can have.
pub const WIDTH_RANGE: std::ops::RangeInclusive<u32> = 32..=600;
/// Space between two columns.
pub const GAP: u32 = 8;

impl Column {
    /// The id in `ui-state.toml` and in the Slint models.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::State => "state",
            Self::Eta => "eta",
            Self::Length => "length",
            Self::Size => "size",
            Self::Profile => "profile",
            Self::Folder => "folder",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        ORDER.into_iter().find(|c| c.id() == id)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Id => "#",
            Self::Name => "Name",
            Self::State => "State",
            Self::Eta => "ETA",
            Self::Length => "Length",
            Self::Size => "Size",
            Self::Profile => "Profile",
            Self::Folder => "Folder",
        }
    }

    pub const fn tooltip(self) -> &'static str {
        match self {
            Self::Id => "The job's number (never used twice)",
            Self::Name => "The file the job writes",
            Self::State => "Where the job stands, with the render's progress",
            Self::Eta => "Time until the render is done",
            Self::Length => "Length of the output (and the number of sections when there are several)",
            Self::Size => "Size of the output: estimated while rendering, measured when finished",
            Self::Profile => "The render profile the job was saved with",
            Self::Folder => "The target folder",
        }
    }

    const fn default_width(self) -> u32 {
        match self {
            Self::Id => 40,
            Self::Name => NAME_MIN,
            Self::State => 116,
            Self::Eta | Self::Length => 64,
            Self::Size => 72,
            Self::Profile | Self::Folder => 120,
        }
    }

    /// Numbers are aligned right.
    pub const fn right_aligned(self) -> bool {
        matches!(self, Self::Id | Self::Eta | Self::Length | Self::Size)
    }
}

/// One shown column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub column: Column,
    pub x: u32,
    pub width: u32,
}

/// The list's layout for one width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Two-line rows without a header.
    pub compact: bool,
    pub slots: Vec<Slot>,
}

/// The width of `column`: the dragged one, else its default.
pub fn width_of(column: Column, widths: &BTreeMap<String, u32>) -> u32 {
    widths.get(column.id()).copied().map_or_else(
        || column.default_width(),
        |w| w.clamp(*WIDTH_RANGE.start(), *WIDTH_RANGE.end()),
    )
}

/// The columns that fit into `width` logical pixels (the row's inner width).
pub fn layout(width: u32, widths: &BTreeMap<String, u32>) -> Layout {
    let mut shown: Vec<Column> = ORDER.to_vec();
    let fixed = |shown: &[Column]| -> u32 {
        shown
            .iter()
            .filter(|c| **c != Column::Name)
            .map(|c| width_of(*c, widths) + GAP)
            .sum()
    };
    let mut drops = DROP_ORDER.iter();
    while width.saturating_sub(fixed(&shown)) < NAME_MIN {
        let Some(next) = drops.next() else {
            return Layout {
                compact: true,
                slots: Vec::new(),
            };
        };
        shown.retain(|c| c != next);
    }
    let name = width.saturating_sub(fixed(&shown));
    let mut x = 0;
    let slots = shown
        .into_iter()
        .map(|column| {
            let w = if column == Column::Name {
                name
            } else {
                width_of(column, widths)
            };
            let slot = Slot { column, x, width: w };
            x += w + GAP;
            slot
        })
        .collect();
    Layout { compact: false, slots }
}

/// The width of `column` after its right edge was dragged by `delta` from `start`.
pub fn dragged(start: u32, delta: i32) -> u32 {
    let w = i64::from(start).saturating_add(i64::from(delta));
    u32::try_from(w.clamp(i64::from(*WIDTH_RANGE.start()), i64::from(*WIDTH_RANGE.end())))
        .unwrap_or(*WIDTH_RANGE.start())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(l: &Layout) -> Vec<&'static str> {
        l.slots.iter().map(|s| s.column.id()).collect()
    }

    #[test]
    fn columns_join_as_the_panel_widens() {
        let widths = BTreeMap::new();
        assert!(layout(200, &widths).compact, "a narrow sidebar shows two-line rows");
        assert!(layout(420, &widths).compact, "a table needs the ETA and the length");
        let l = layout(440, &widths);
        assert_eq!(ids(&l), ["name", "state", "eta", "length"]);
        let l = layout(480, &widths);
        assert_eq!(ids(&l), ["id", "name", "state", "eta", "length"]);
        let l = layout(2000, &widths);
        assert_eq!(ids(&l).len(), 8);
        // Every slot follows the previous one, and the row is filled exactly.
        let last = l.slots.last().unwrap();
        assert_eq!(last.x + last.width, 2000);
        for pair in l.slots.windows(2) {
            assert_eq!(pair[0].x + pair[0].width + GAP, pair[1].x);
        }
        assert!(l.slots.iter().all(|s| s.column != Column::Name || s.width >= NAME_MIN));
    }

    #[test]
    fn dragged_widths_count_and_are_limited() {
        let mut widths = BTreeMap::new();
        widths.insert("eta".to_owned(), 200);
        widths.insert("state".to_owned(), 5);
        assert_eq!(width_of(Column::State, &widths), *WIDTH_RANGE.start());
        assert!(layout(400, &widths).compact, "a wide ETA leaves no room for a table");
        let l = layout(520, &widths);
        assert_eq!(ids(&l), ["name", "state", "eta", "length"]);
        assert_eq!(dragged(100, -500), 32);
        assert_eq!(dragged(100, 30), 130);
        assert_eq!(Column::from_id("folder"), Some(Column::Folder));
        assert_eq!(Column::from_id("nope"), None);
    }
}
