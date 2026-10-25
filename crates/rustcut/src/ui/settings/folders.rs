//! The named target folders on the Export & Naming page (pure): add, label, type or pick a path, reorder, remove. The
//! export card offers them after "Source folder" in this order (user decision Q-X3); a folder's label is what the card
//! shows. A typed path is checked against what Linux allows (P9.7, the user's wish); a folder that does not exist yet
//! is made when its path is kept (the bridge does that off the UI thread).

use std::path::{Component, Path, PathBuf};

use path_clean::PathClean as _;

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

/// The path of the folder at `index` becomes `path`; another folder of the list with that path refuses it.
pub fn set_path(folders: &mut [TargetFolder], index: usize, path: PathBuf) -> Result<bool, &'static str> {
    if folders.iter().enumerate().any(|(i, f)| i != index && f.path == path) {
        return Err("This folder is in the list already.");
    }
    let Some(folder) = folders.get_mut(index) else {
        return Ok(false);
    };
    if folder.path == path {
        return Ok(false);
    }
    // A label that was the old folder's name follows the new one.
    if folder.label == default_label(&folder.path) {
        folder.label = default_label(&path);
    }
    folder.path = path;
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

/// The folder at `index` was dropped before the row `before` of the current order (the row count = the end; P9.7's
/// drag & drop).
pub fn drop_before(folders: &mut [TargetFolder], index: usize, before: usize) -> bool {
    let target = if before > index { before - 1 } else { before };
    let by = if target >= index {
        i32::try_from(target - index)
    } else {
        i32::try_from(index - target).map(|n| -n)
    };
    by.is_ok_and(|by| shift(folders, index, by))
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
        let mut folders = Vec::new();
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
            folders.get(1).map(|f| f.label.as_str()),
            Some("top"),
            "the folder's name follows"
        );
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
        assert!(drop_before(&mut folders, 0, 3), "the first one to the end");
        assert_eq!(paths(&folders), ["/", "/v/best", "/v/clips"]);
        assert!(drop_before(&mut folders, 2, 0));
        assert_eq!(paths(&folders), ["/v/clips", "/", "/v/best"]);
        assert!(
            !drop_before(&mut folders, 1, 1) && !drop_before(&mut folders, 1, 2),
            "its own place"
        );
        let before = folders.clone();
        assert_eq!(remove(&mut folders, 1).unwrap().path, PathBuf::from("/"));
        assert_eq!(remove(&mut folders, 7), None);
        assert_eq!(follow_selection(&before, &folders, Some(2)), Some(1));
        assert_eq!(
            follow_selection(&before, &folders, Some(1)),
            None,
            "removed: back to the source folder"
        );
        assert_eq!(follow_selection(&before, &folders, None), None);
    }
}
