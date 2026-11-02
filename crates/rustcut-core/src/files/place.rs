//! Whether a folder can take moved files, and how much room its drive has (the move dialog's checks, P12 block 7).

use std::path::Path;

use rustix::fs::{Access, AtFlags, CWD, accessat, statvfs};

/// What a target folder is like. Blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Why it cannot take files (it is a file, it cannot be created, no write permission); `None` = it can.
    pub unusable: Option<String>,
    /// Free bytes on its drive (of the nearest folder that exists).
    pub free: Option<u64>,
}

/// Looks at `folder`: an existing folder must be writable, a missing one must be creatable in its nearest existing
/// ancestor.
pub fn place(folder: &Path) -> Place {
    let Some(existing) = folder.ancestors().find(|p| p.symlink_metadata().is_ok()) else {
        return Place {
            unusable: Some("no part of the path exists".into()),
            free: None,
        };
    };
    let free = statvfs(existing).ok().map(|s| s.f_bavail.saturating_mul(s.f_frsize));
    let writable = |p: &Path| accessat(CWD, p, Access::WRITE_OK | Access::EXEC_OK, AtFlags::empty()).is_ok();
    let unusable = if !existing.is_dir() {
        Some(format!("{} is a file, not a folder", existing.display()))
    } else if !writable(existing) {
        Some(if existing == folder {
            "no permission to write into it".to_owned()
        } else {
            format!(
                "it cannot be created: no permission to write into {}",
                existing.display()
            )
        })
    } else {
        None
    };
    Place { unusable, free }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_files_and_missing_parts() {
        let dir = tempfile::tempdir().unwrap();
        let fine = place(dir.path());
        assert_eq!(fine.unusable, None);
        assert!(fine.free.is_some_and(|f| f > 0));
        assert_eq!(
            place(&dir.path().join("new/deeper")).unusable,
            None,
            "made when a file moves in"
        );
        let file = dir.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        assert!(place(&file.join("sub")).unusable.unwrap().contains("is a file"));
    }
}
