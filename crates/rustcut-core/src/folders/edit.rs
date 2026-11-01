//! Every change of the folder tree is one method (the Settings page, the export card and the pickers call these): add
//! a folder or a group, rename, change a path, move (before, after or into an entry), star with a wheel place, set a
//! picture or a color, and delete (a group with everything in it, Q-FP2). The source entry cannot be moved, renamed or
//! deleted (D-FPb).

use std::path::{Path, PathBuf};

use super::tree::{FolderId, FolderNode, FolderTree, Look, LookKind, MAX_PLACES, default_name};

/// Why an edit was refused (the texts are for the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("This folder is in the list already.")]
    DuplicatePath,
    #[error("An entry needs a name.")]
    EmptyName,
    #[error("The entry is gone.")]
    Unknown,
    #[error("\"Next to the source\" cannot be changed this way.")]
    Source,
    #[error("A group has no path.")]
    Group,
    #[error("Only a group can hold entries.")]
    NotGroup,
    #[error("A group cannot go into itself.")]
    IntoItself,
    #[error("Every place on the wheel is taken.")]
    NoFreePlace,
}

/// Where a moved or new entry goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// At the end of the top level.
    End,
    Before(FolderId),
    After(FolderId),
    /// The last entry of this group.
    Into(FolderId),
}

/// The list holding `id` and its index there.
fn locate(nodes: &mut Vec<FolderNode>, id: FolderId) -> Option<(&mut Vec<FolderNode>, usize)> {
    if let Some(i) = nodes.iter().position(|n| n.id == id) {
        return Some((nodes, i));
    }
    nodes.iter_mut().find_map(|n| locate(&mut n.children, id))
}

impl FolderTree {
    /// Adds a folder named after itself at `spot`; a path the tree has is refused.
    pub fn add_folder(&mut self, path: &Path, spot: Spot) -> Result<FolderId, EditError> {
        if self.find_path(path).is_some() {
            return Err(EditError::DuplicatePath);
        }
        let id = self.next_id();
        self.insert(FolderNode::folder(id, path), spot)?;
        Ok(id)
    }

    pub fn add_group(&mut self, name: &str, spot: Spot) -> Result<FolderId, EditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::EmptyName);
        }
        let id = self.next_id();
        self.insert(FolderNode::group(id, name), spot)?;
        Ok(id)
    }

    fn insert(&mut self, node: FolderNode, spot: Spot) -> Result<(), EditError> {
        match spot {
            Spot::End => self.entries.push(node),
            Spot::Before(at) | Spot::After(at) => {
                let (list, i) = locate(&mut self.entries, at).ok_or(EditError::Unknown)?;
                let i = if matches!(spot, Spot::After(_)) { i + 1 } else { i };
                list.insert(i.min(list.len()), node);
            }
            Spot::Into(group) => {
                let group = self.find_mut(group).ok_or(EditError::Unknown)?;
                if !group.is_group() {
                    return Err(EditError::NotGroup);
                }
                group.children.push(node);
            }
        }
        Ok(())
    }

    /// A new name; an empty one is refused. `Ok(false)` = nothing changed.
    pub fn rename(&mut self, id: FolderId, name: &str) -> Result<bool, EditError> {
        if id.is_source() {
            return Err(EditError::Source);
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(EditError::EmptyName);
        }
        let node = self.find_mut(id).ok_or(EditError::Unknown)?;
        if node.name == name {
            return Ok(false);
        }
        name.clone_into(&mut node.name);
        Ok(true)
    }

    /// The folder's path becomes `path` (another folder with it refuses); a name that was the old folder's own name
    /// follows the new one.
    pub fn set_path(&mut self, id: FolderId, path: PathBuf) -> Result<bool, EditError> {
        if id.is_source() {
            return Err(EditError::Source);
        }
        if self.find_path(&path).is_some_and(|n| n.id != id) {
            return Err(EditError::DuplicatePath);
        }
        let node = self.find_mut(id).ok_or(EditError::Unknown)?;
        let old = node.path.as_deref().ok_or(EditError::Group)?;
        if old == path {
            return Ok(false);
        }
        if node.name == default_name(old) {
            node.name = default_name(&path);
        }
        node.path = Some(path);
        Ok(true)
    }

    /// Moves an entry (with its contents); into itself or one of its own groups is refused. `Ok(false)` = it stays.
    pub fn move_to(&mut self, id: FolderId, spot: Spot) -> Result<bool, EditError> {
        if id.is_source() {
            return Err(EditError::Source);
        }
        let target = match spot {
            Spot::End => None,
            Spot::Before(t) | Spot::After(t) | Spot::Into(t) => Some(t),
        };
        if let Some(t) = target {
            if t == id && matches!(spot, Spot::Before(_) | Spot::After(_)) {
                return Ok(false);
            }
            if t == id || self.groups_of(t).contains(&id) {
                return Err(EditError::IntoItself);
            }
            let node = self.find(t).ok_or(EditError::Unknown)?;
            if matches!(spot, Spot::Into(_)) && !node.is_group() {
                return Err(EditError::NotGroup);
            }
        }
        let before = self.entries.clone();
        let node = self.take(id).ok_or(EditError::Unknown)?;
        if let Err(e) = self.insert(node, spot) {
            self.entries = before;
            return Err(e);
        }
        Ok(self.entries != before)
    }

    /// Removes the entry (a group with everything in it).
    fn take(&mut self, id: FolderId) -> Option<FolderNode> {
        let (list, i) = locate(&mut self.entries, id)?;
        Some(list.remove(i))
    }

    /// Deletes an entry; a group goes with its contents (Q-FP2). The source cannot be deleted.
    pub fn remove(&mut self, id: FolderId) -> Result<FolderNode, EditError> {
        if id.is_source() {
            return Err(EditError::Source);
        }
        self.take(id).ok_or(EditError::Unknown)
    }

    fn look_mut(&mut self, id: FolderId) -> Result<&mut Look, EditError> {
        if id.is_source() {
            Ok(&mut self.source.look)
        } else {
            Ok(&mut self.find_mut(id).ok_or(EditError::Unknown)?.look)
        }
    }

    /// What the entry shows; the icon and the own picture stay kept whatever the choice is.
    pub fn set_picture_kind(&mut self, id: FolderId, kind: LookKind) -> Result<bool, EditError> {
        let look = self.look_mut(id)?;
        let changed = look.kind != kind;
        look.kind = kind;
        Ok(changed)
    }

    /// A built-in icon: it is kept and shown.
    pub fn set_icon(&mut self, id: FolderId, name: &str) -> Result<bool, EditError> {
        let look = self.look_mut(id)?;
        let changed = look.kind != LookKind::Icon || look.icon.as_deref() != Some(name);
        look.kind = LookKind::Icon;
        look.icon = Some(name.to_owned());
        Ok(changed)
    }

    /// An own picture by its hash: it is kept and shown. `None` deletes it; the choice stays (the letter shows).
    pub fn set_image(&mut self, id: FolderId, hash: Option<String>) -> Result<bool, EditError> {
        let look = self.look_mut(id)?;
        let changed = look.image != hash || (hash.is_some() && look.kind != LookKind::Own);
        if hash.is_some() {
            look.kind = LookKind::Own;
        }
        look.image = hash;
        Ok(changed)
    }

    /// The wheel's short name; an empty text removes it (the wheel shows the name again).
    pub fn set_short_name(&mut self, id: FolderId, short: &str) -> Result<bool, EditError> {
        let short = Some(short.trim()).filter(|s| !s.is_empty()).map(str::to_owned);
        let slot = if id.is_source() {
            &mut self.source.short_name
        } else {
            &mut self.find_mut(id).ok_or(EditError::Unknown)?.short_name
        };
        if *slot == short {
            return Ok(false);
        }
        *slot = short;
        Ok(true)
    }

    /// `#RRGGBB` or none; another text is refused as unknown.
    pub fn set_color(&mut self, id: FolderId, color: Option<&str>) -> Result<bool, EditError> {
        let color = color.filter(|c| is_color(c)).map(str::to_ascii_lowercase);
        let slot = if id.is_source() {
            &mut self.source.color
        } else {
            &mut self.find_mut(id).ok_or(EditError::Unknown)?.color
        };
        if *slot == color {
            return Ok(false);
        }
        *slot = color;
        Ok(true)
    }

    fn place_slot(&mut self, id: FolderId) -> Option<&mut Option<u8>> {
        if id.is_source() {
            Some(&mut self.source.place)
        } else {
            self.find_mut(id).map(|n| &mut n.place)
        }
    }

    /// The star: on = the first free place of `places` (Q-FP2: every favorite has its own place), off = none.
    pub fn set_favorite(&mut self, id: FolderId, on: bool, places: u8) -> Result<bool, EditError> {
        let taken: Vec<u8> = self.favorites().into_iter().map(|(_, p)| p).collect();
        let slot = self.place_slot(id).ok_or(EditError::Unknown)?;
        match (on, *slot) {
            (true, Some(_)) | (false, None) => Ok(false),
            (false, Some(_)) => {
                *slot = None;
                Ok(true)
            }
            (true, None) => {
                let free = (0..places.min(MAX_PLACES))
                    .find(|p| !taken.contains(p))
                    .ok_or(EditError::NoFreePlace)?;
                *slot = Some(free);
                Ok(true)
            }
        }
    }

    /// Puts a favorite (or makes the entry one) on `place`; the favorite that had it takes the entry's old place (or
    /// loses its star when the entry had none).
    pub fn put_on_place(&mut self, id: FolderId, place: u8) -> Result<bool, EditError> {
        if place >= MAX_PLACES {
            return Err(EditError::NoFreePlace);
        }
        let holder = self.favorites().into_iter().find(|(_, p)| *p == place).map(|(f, _)| f);
        if holder == Some(id) {
            return Ok(false);
        }
        let old = *self.place_slot(id).ok_or(EditError::Unknown)?;
        if let Some(other) = holder.and_then(|h| self.place_slot(h)) {
            *other = old;
        }
        if let Some(slot) = self.place_slot(id) {
            *slot = Some(place);
        }
        Ok(true)
    }
}

/// `#RRGGBB`.
pub fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text.chars().skip(1).all(|c| c.is_ascii_hexdigit())
}
