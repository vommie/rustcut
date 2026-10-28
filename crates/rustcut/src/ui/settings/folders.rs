//! Typed target folder paths (pure): `~` is the home folder, the path must start at `/` and fit Linux's limits (P9.7,
//! the user's wish). The Target Folders page (P11.3) checks its path field with this; a folder that does not exist
//! yet is made when a job writes into it.

use std::path::{Component, Path, PathBuf};

use path_clean::PathClean as _;

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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
