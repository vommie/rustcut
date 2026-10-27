//! Where a program of the settings is and whether it runs (the Settings rows of ffmpeg, ffprobe and rustview, 0.0.67):
//! a bare name is looked up in PATH like the shell does (`which`), a path is taken as it is, and the program prints its
//! version with a time limit.

use std::path::{Path, PathBuf};

use super::program_version;

/// What a look at a program found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The file that runs (`None` = not found).
    pub path: Option<PathBuf>,
    /// Its version line, or why it could not be run.
    pub version: Result<String, String>,
}

/// Finds `program` (a bare name in PATH, else the path) and asks it for its version with `flag`.
pub async fn locate(program: &Path, flag: &str) -> Located {
    let bare = program.components().count() == 1 && !program.is_absolute();
    let path = if bare {
        let name = program.to_path_buf();
        tokio::task::spawn_blocking(move || which::which(name).ok())
            .await
            .ok()
            .flatten()
    } else {
        program.is_file().then(|| program.to_path_buf())
    };
    let Some(path) = path else {
        let reason = if bare {
            format!("\"{}\" was not found in PATH", program.display())
        } else {
            format!("{} does not exist", program.display())
        };
        return Located {
            path: None,
            version: Err(reason),
        };
    };
    let version = program_version(&path, flag).await;
    Located {
        path: Some(path),
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bare_names_are_found_in_path_and_paths_are_taken_as_they_are() {
        let found = locate(Path::new("sh"), "-c").await;
        assert!(found.path.as_ref().is_some_and(|p| p.is_absolute()), "{found:?}");
        let missing = locate(Path::new("rustcut-no-such-program"), "--version").await;
        assert_eq!(missing.path, None);
        assert!(missing.version.unwrap_err().contains("not found in PATH"));
        let gone = locate(Path::new("/nonexistent/ffmpeg"), "-version").await;
        assert_eq!(gone.path, None);
        assert!(gone.version.unwrap_err().contains("does not exist"));
    }
}
