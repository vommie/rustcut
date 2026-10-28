//! The folder wheel's levels (Q-FP2): a level has a fixed number of places (default 8, 4-12) or as many as it has
//! entries ("divided"); when it has more entries than places, the last place becomes "More ...", itself a level with
//! the rest (and so on). The first level shows the favorites on their own places and nothing else: a tree without
//! favorites gives an empty first level (the wheel says how to fill it; the user's answer to D-FPc, 2026-10-28), and
//! "Next to the source" is on the wheel only with a star.

use super::tree::{FolderId, FolderTree};

/// How a level spreads its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Places per level (4-12).
    pub places: u8,
    /// As many places as entries (up to `places`) instead of always `places`.
    pub divided: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            places: 8,
            divided: false,
        }
    }
}

/// Which level the wheel shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LevelKey {
    Start,
    Group(FolderId),
    /// The entries a "More ..." place leads to.
    More(Vec<FolderId>),
}

/// One place of a level.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Slot {
    Empty,
    Entry(FolderId),
    More(Vec<FolderId>),
}

impl Slot {
    /// The level this place opens (a group or "More ..."); `None` for a folder or an empty place.
    pub fn opens(&self, tree: &FolderTree) -> Option<LevelKey> {
        match self {
            Self::Empty => None,
            Self::More(ids) => Some(LevelKey::More(ids.clone())),
            Self::Entry(id) => tree.find(*id).filter(|n| n.is_group()).map(|n| LevelKey::Group(n.id)),
        }
    }
}

/// A level's places, clockwise from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub key: LevelKey,
    pub slots: Vec<Slot>,
}

impl Layout {
    fn places(self) -> usize {
        usize::from(self.places.clamp(2, super::tree::MAX_PLACES))
    }

    /// Entries one after the other; "More ..." takes the last place when they do not fit.
    fn sequence(self, ids: &[FolderId]) -> Vec<Slot> {
        let places = self.places();
        if ids.is_empty() {
            return vec![Slot::Empty; places];
        }
        let mut slots: Vec<Slot> = if ids.len() <= places {
            ids.iter().copied().map(Slot::Entry).collect()
        } else {
            let (shown, rest) = ids.split_at(places - 1);
            shown
                .iter()
                .copied()
                .map(Slot::Entry)
                .chain(std::iter::once(Slot::More(rest.to_vec())))
                .collect()
        };
        if !self.divided {
            slots.resize(places, Slot::Empty);
        }
        slots
    }
}

/// The level `key`; `None` when its group is gone.
pub fn level(tree: &FolderTree, key: &LevelKey, layout: Layout) -> Option<Level> {
    let slots = match key {
        LevelKey::Start => start(tree, layout),
        LevelKey::Group(id) => {
            let group = tree.find(*id).filter(|n| n.is_group())?;
            let ids: Vec<FolderId> = group.children.iter().map(|n| n.id).collect();
            layout.sequence(&ids)
        }
        LevelKey::More(ids) => {
            let ids: Vec<FolderId> = ids.iter().copied().filter(|id| tree.entry(*id).is_some()).collect();
            layout.sequence(&ids)
        }
    };
    Some(Level {
        key: key.clone(),
        slots,
    })
}

fn start(tree: &FolderTree, layout: Layout) -> Vec<Slot> {
    let mut favorites = tree.favorites();
    if favorites.is_empty() {
        return layout.sequence(&[]);
    }
    favorites.sort_by_key(|(_, place)| *place);
    if layout.divided {
        let ids: Vec<FolderId> = favorites.iter().map(|(id, _)| *id).collect();
        return layout.sequence(&ids);
    }
    let places = layout.places();
    let mut slots = vec![Slot::Empty; places];
    let mut left = Vec::new();
    for (id, place) in favorites {
        match slots.get_mut(usize::from(place)) {
            Some(slot @ Slot::Empty) => *slot = Slot::Entry(id),
            _ => left.push(id),
        }
    }
    // Favorites beyond the places (the count was lowered) take the free places in order.
    let mut left = left.into_iter();
    for slot in &mut slots {
        if matches!(slot, Slot::Empty) {
            match left.next() {
                Some(id) => *slot = Slot::Entry(id),
                None => break,
            }
        }
    }
    let rest: Vec<FolderId> = left.collect();
    if !rest.is_empty()
        && let Some(last) = slots.last_mut()
    {
        let mut more: Vec<FolderId> = match last {
            Slot::Entry(id) => vec![*id],
            _ => Vec::new(),
        };
        more.extend(rest);
        *last = Slot::More(more);
    }
    slots
}
