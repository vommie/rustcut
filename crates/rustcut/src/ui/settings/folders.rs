//! The named target folders on the Export & Naming page (pure): add, label, reorder, remove. The export card offers them
//! after "Source folder" in this order (user decision Q-X3); a folder's label is what the card shows.

use std::path::{Path, PathBuf};

use rustcut_core::settings::TargetFolder;

/// Adds `path` at the end with its folder name as the label; `false` when the list has it already.
pub fn add(folders: &mut Vec<TargetFolder>, path: &Path) -> bool {
    if folders.iter().any(|f| f.path == path) {
        return false;
    }
    folders.push(TargetFolder {
        label: default_label(path),
        path: path.to_path_buf(),
    });
    true
}

/// The folder's own name, or the whole path for `/`.
pub fn default_label(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// A new label; an empty one is refused.
pub fn rename(folders: &mut [TargetFolder], index: usize, label: &str) -> Result<bool, &'static str> {
    let label = label.trim();
    if label.is_empty() {
        return Err("A folder needs a label.");
    }
    let Some(folder) = folders.get_mut(index) else {
        return Ok(false);
    };
    if folder.label == label {
        return Ok(false);
    }
    label.clone_into(&mut folder.label);
    Ok(true)
}

/// Moves the folder at `index` by `by` places (within the list).
pub fn shift(folders: &mut [TargetFolder], index: usize, by: i32) -> bool {
    let Some(target) = index.checked_add_signed(isize::try_from(by).unwrap_or(0)) else {
        return false;
    };
    if index >= folders.len() || target >= folders.len() || target == index {
        return false;
    }
    if target < index {
        if let Some(part) = folders.get_mut(target..=index) {
            part.rotate_right(1);
        }
    } else if let Some(part) = folders.get_mut(index..=target) {
        part.rotate_left(1);
    }
    true
}

pub fn remove(folders: &mut Vec<TargetFolder>, index: usize) -> Option<TargetFolder> {
    (index < folders.len()).then(|| folders.remove(index))
}

/// Where the export card's selection goes when the list changes: the same folder (by path) when it is still listed,
/// else "Source folder" (`None`). The card's index counts the folders after "Source folder" from 0.
pub fn follow_selection(before: &[TargetFolder], after: &[TargetFolder], selected: Option<usize>) -> Option<usize> {
    let path: &PathBuf = &before.get(selected?)?.path;
    after.iter().position(|f| &f.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(folders: &[TargetFolder]) -> Vec<&str> {
        folders.iter().map(|f| f.path.to_str().unwrap()).collect()
    }

    #[test]
    fn folders_are_added_once_labeled_moved_and_removed() {
        let mut folders = Vec::new();
        assert!(add(&mut folders, Path::new("/v/clips")));
        assert!(add(&mut folders, Path::new("/v/best")));
        assert!(add(&mut folders, Path::new("/")));
        assert!(!add(&mut folders, Path::new("/v/clips")));
        assert_eq!(folders.first().unwrap().label, "clips");
        assert_eq!(folders.get(2).unwrap().label, "/");
        assert_eq!(rename(&mut folders, 1, "  Best of  "), Ok(true));
        assert_eq!(folders.get(1).unwrap().label, "Best of");
        assert!(rename(&mut folders, 1, " ").is_err());
        assert!(shift(&mut folders, 2, -2));
        assert_eq!(paths(&folders), ["/", "/v/clips", "/v/best"]);
        assert!(shift(&mut folders, 0, 1));
        assert_eq!(paths(&folders), ["/v/clips", "/", "/v/best"]);
        assert!(!shift(&mut folders, 0, -1) && !shift(&mut folders, 2, 1) && !shift(&mut folders, 9, 1));
        let before = folders.clone();
        assert_eq!(remove(&mut folders, 1).unwrap().path, PathBuf::from("/"));
        assert_eq!(remove(&mut folders, 7), None);
        assert_eq!(follow_selection(&before, &folders, Some(2)), Some(1));
        assert_eq!(follow_selection(&before, &folders, Some(1)), None, "removed: back to the source folder");
        assert_eq!(follow_selection(&before, &folders, None), None);
    }
}
