//! The target folders on the Export & Naming page (pure): add, label, type or pick a path, reorder, remove. Until P11.3
//! gives the tree its own page, this list edits the tree's top level (the export card offers every folder of the tree
//! after "Next to the source"). A typed path is checked against what Linux allows (P9.7, the user's wish); a folder that does not exist yet
//! is made when its path is kept (the bridge does that off the UI thread).

use std::path::{Component, Path, PathBuf};

use path_clean::PathClean as _;

use rustcut_core::folders::{FolderId, FolderNode, FolderTree, Spot};

/// The id of the top-level entry shown in row `index` (until P11.3 the page lists the tree's top level).
fn row_id(tree: &FolderTree, index: usize) -> Option<FolderId> {
    tree.entries.get(index).map(|n| n.id)
}

/// Adds `path` at the end, named after itself; `false` when the tree has it already.
pub fn add(tree: &mut FolderTree, path: &Path) -> bool {
    tree.add_folder(path, Spot::End).is_ok()
}

/// A new name; an empty one is refused.
pub fn rename(tree: &mut FolderTree, index: usize, label: &str) -> Result<bool, &'static str> {
    let Some(id) = row_id(tree, index) else {
        return Ok(false);
    };
    tree.rename(id, label).map_err(|_empty| "A folder needs a label.")
}

/// The longest path Linux takes (`PATH_MAX` without the terminating NUL) and the longest name of one folder (`NAME_MAX`).
const PATH_MAX: usize = 4095;
const NAME_MAX: usize = 255;

/// A typed folder path: `~` stands for the home folder (`home`), the path must start at `/`, and every part must fit
/// Linux's limits; `.` and `..` are resolved and a trailing `/` goes. The folder need not exist.
pub fn check_path(text: &str, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Err("A folder needs a path.");
    }
    if text.contains('\0') {
        return Err("A path cannot contain a NUL character.");
    }
    let path = match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        (Some(_), None) => return Err("The home folder is not known: type the whole path."),
        _ => PathBuf::from(text),
    };
    if !path.is_absolute() {
        return Err("Type the whole path, starting at /.");
    }
    let path = path.clean();
    if path.as_os_str().len() > PATH_MAX {
        return Err("The path is longer than Linux allows (4096 bytes).");
    }
    let too_long = path.components().any(|c| match c {
        Component::Normal(name) => name.len() > NAME_MAX,
        _ => false,
    });
    if too_long {
        return Err("A folder name in the path is longer than Linux allows (255 bytes).");
    }
    Ok(path)
}

/// The path of the folder at `index` becomes `path`; another folder of the tree with that path refuses it.
pub fn set_path(tree: &mut FolderTree, index: usize, path: PathBuf) -> Result<bool, &'static str> {
    let Some(id) = row_id(tree, index) else {
        return Ok(false);
    };
    tree.set_path(id, path).map_err(|e| match e {
        rustcut_core::folders::EditError::DuplicatePath => "This folder is in the list already.",
        _ => "A group has no path.",
    })
}

/// Moves the folder at `index` by `by` places (within the list).
pub fn shift<T>(folders: &mut [T], index: usize, by: i32) -> bool {
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

/// The folder at `index` was dropped before the row `before` of the current order (the row count = the end; P9.7's
/// drag & drop).
pub fn drop_before<T>(folders: &mut [T], index: usize, before: usize) -> bool {
    let target = if before > index { before - 1 } else { before };
    let by = if target >= index {
        i32::try_from(target - index)
    } else {
        i32::try_from(index - target).map(|n| -n)
    };
    by.is_ok_and(|by| shift(folders, index, by))
}

pub fn remove(tree: &mut FolderTree, index: usize) -> Option<FolderNode> {
    tree.remove(row_id(tree, index)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tree: &FolderTree) -> Vec<&str> {
        tree.entries
            .iter()
            .map(|f| f.path.as_deref().unwrap().to_str().unwrap())
            .collect()
    }

    #[test]
    fn typed_paths_follow_linux_rules() {
        let home = Some(Path::new("/home/u"));
        assert_eq!(check_path(" /v/clips/ ", home), Ok(PathBuf::from("/v/clips")));
        assert_eq!(check_path("~/Videos", home), Ok(PathBuf::from("/home/u/Videos")));
        assert_eq!(check_path("~", home), Ok(PathBuf::from("/home/u")));
        assert_eq!(check_path("/v/./a/../b", home), Ok(PathBuf::from("/v/b")));
        assert_eq!(
            check_path("/Ärger über Größe", home),
            Ok(PathBuf::from("/Ärger über Größe"))
        );
        assert!(check_path("", home).is_err());
        assert!(check_path("videos/clips", home).is_err(), "relative");
        assert!(
            check_path("~other/x", home).is_err(),
            "another user's home is not expanded"
        );
        assert!(check_path("~/x", None).is_err());
        assert!(check_path("/a\0b", home).is_err());
        assert!(
            check_path(&format!("/{}", "x".repeat(256)), home).is_err(),
            "a name over 255 bytes"
        );
        assert!(check_path(&format!("/{}", "x".repeat(255)), home).is_ok());
        assert!(
            check_path(&"/abcdefgh".repeat(512), home).is_err(),
            "a path over 4095 bytes"
        );
        let mut folders = FolderTree::default();
        assert!(add(&mut folders, Path::new("/v/clips")));
        assert!(add(&mut folders, Path::new("/v/best")));
        assert_eq!(
            set_path(&mut folders, 1, PathBuf::from("/v/clips")),
            Err("This folder is in the list already.")
        );
        assert_eq!(set_path(&mut folders, 1, PathBuf::from("/v/top")), Ok(true));
        assert_eq!(set_path(&mut folders, 1, PathBuf::from("/v/top")), Ok(false));
        assert_eq!(paths(&folders), ["/v/clips", "/v/top"]);
        assert_eq!(
            folders.entries.get(1).map(|f| f.name.as_str()),
            Some("top"),
            "the folder's name follows"
        );
    }

    #[test]
    fn folders_are_added_once_labeled_moved_and_removed() {
        let mut folders = FolderTree::default();
        assert!(add(&mut folders, Path::new("/v/clips")));
        assert!(add(&mut folders, Path::new("/v/best")));
        assert!(add(&mut folders, Path::new("/")));
        assert!(!add(&mut folders, Path::new("/v/clips")));
        assert_eq!(folders.entries.first().unwrap().name, "clips");
        assert_eq!(folders.entries.get(2).unwrap().name, "/");
        assert_eq!(rename(&mut folders, 1, "  Best of  "), Ok(true));
        assert_eq!(folders.entries.get(1).unwrap().name, "Best of");
        assert!(rename(&mut folders, 1, " ").is_err());
        assert!(shift(&mut folders.entries, 2, -2));
        assert_eq!(paths(&folders), ["/", "/v/clips", "/v/best"]);
        assert!(shift(&mut folders.entries, 0, 1));
        assert_eq!(paths(&folders), ["/v/clips", "/", "/v/best"]);
        assert!(
            !shift(&mut folders.entries, 0, -1)
                && !shift(&mut folders.entries, 2, 1)
                && !shift(&mut folders.entries, 9, 1)
        );
        assert!(drop_before(&mut folders.entries, 0, 3), "the first one to the end");
        assert_eq!(paths(&folders), ["/", "/v/best", "/v/clips"]);
        assert!(drop_before(&mut folders.entries, 2, 0));
        assert_eq!(paths(&folders), ["/v/clips", "/", "/v/best"]);
        assert!(
            !drop_before(&mut folders.entries, 1, 1) && !drop_before(&mut folders.entries, 1, 2),
            "its own place"
        );
        assert_eq!(remove(&mut folders, 1).unwrap().path, Some(PathBuf::from("/")));
        assert_eq!(remove(&mut folders, 7), None);
        assert_eq!(paths(&folders), ["/v/clips", "/v/best"]);
    }
}
