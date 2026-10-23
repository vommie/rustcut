//! The card's live check of the target in words: one line under the file name (a free name, a hint, a conflict that
//! Save will ask about, or why the job cannot be saved) and its tooltip.

use std::path::Path;

use rustcut_core::naming::{CheckInput, Conflicts, FolderScan, NameError, QueuedTarget, check, same_path};
use rustcut_core::settings::WarningSettings;

/// How the line looks: 0 free, 1 hint, 2 a conflict Save asks about, 3 the job cannot be saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Free = 0,
    Hint = 1,
    Conflict = 2,
    Error = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCheck {
    pub level: Level,
    pub text: String,
    pub tooltip: String,
}

impl CardCheck {
    fn new(level: Level, text: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
            tooltip: tooltip.into(),
        }
    }

    pub const fn can_save(&self) -> bool {
        !matches!(self.level, Level::Error)
    }
}

/// What the card knows about the job's target.
pub struct CardInput<'a> {
    pub source: Option<&'a Path>,
    pub folder: Option<&'a Path>,
    /// The stem and the file name, or why the template failed.
    pub file: Result<(&'a str, &'a str), &'a NameError>,
    pub base: &'a str,
    /// The last scan of `folder` (`None` while it runs).
    pub scan: Option<&'a FolderScan>,
    pub queue: &'a [QueuedTarget],
    pub warnings: &'a WarningSettings,
}

pub fn card_check(input: &CardInput<'_>) -> CardCheck {
    let Some(source) = input.source else {
        return CardCheck::new(Level::Error, "Open a video first", "");
    };
    let Some(folder) = input.folder else {
        return CardCheck::new(Level::Error, "No target folder", "");
    };
    let file = match input.file {
        Ok((_, file)) => file,
        Err(e) => return CardCheck::new(Level::Error, format!("The name cannot be used: {e}"), ""),
    };
    let target = folder.join(file);
    if same_path(source, &target) {
        return CardCheck::new(Level::Error, "This is the source file", target.display().to_string());
    }
    let Some(scan) = input.scan else {
        return CardCheck::new(Level::Hint, "Checking the folder...", folder.display().to_string());
    };
    if !scan.readable {
        return CardCheck::new(
            Level::Error,
            "The folder does not exist or cannot be read",
            folder.display().to_string(),
        );
    }
    let exists = scan.names.contains(file).then_some(rustcut_core::naming::ExistingFile {
        bytes: 0,
        modified: None,
    });
    let found: Conflicts = check(&CheckInput {
        target: &target,
        base: input.base,
        scan,
        exists,
        queue: input.queue,
        warnings: input.warnings,
    });
    let similar_tooltip = || {
        if found.similar.is_empty() {
            folder.display().to_string()
        } else {
            format!(
                "Files in {} starting with \"{}\":\n{}",
                folder.display(),
                input.base.trim(),
                found.similar.join("\n")
            )
        }
    };
    if found.exists.is_some() {
        return CardCheck::new(
            Level::Conflict,
            "The file exists: Save asks what to do",
            similar_tooltip(),
        );
    }
    match found.queued.as_slice() {
        [] => {}
        [one] => {
            return CardCheck::new(
                Level::Conflict,
                format!("Job {} writes this file too: Save asks", one.id),
                similar_tooltip(),
            );
        }
        many => {
            return CardCheck::new(
                Level::Conflict,
                format!("{} queued jobs write this file: Save asks", many.len()),
                similar_tooltip(),
            );
        }
    }
    match found.similar.len() {
        0 => CardCheck::new(Level::Free, "Free", folder.display().to_string()),
        1 => CardCheck::new(
            Level::Hint,
            "1 file in the folder has a similar name",
            similar_tooltip(),
        ),
        n => CardCheck::new(
            Level::Hint,
            format!("{n} files in the folder have similar names"),
            similar_tooltip(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::job::{JobId, JobStateKind};

    use super::*;

    fn scan(names: &[&str]) -> FolderScan {
        FolderScan {
            readable: true,
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    #[test]
    fn the_line_names_the_first_problem() {
        let warnings = WarningSettings::default();
        let folder_scan = scan(&["Clip - 01.mkv", "clip.mp4"]);
        let queue = vec![QueuedTarget {
            id: JobId(9),
            target: PathBuf::from("/out/Clip - 02.mkv"),
            state: JobStateKind::Waiting,
        }];
        let input = |file: &'static str| CardInput {
            source: Some(Path::new("/in/Clip.mkv")),
            folder: Some(Path::new("/out")),
            file: Ok(("", file)),
            base: "Clip",
            scan: Some(&folder_scan),
            queue: &queue,
            warnings: &warnings,
        };
        let line = card_check(&input("Clip - 01.mkv"));
        assert_eq!(
            (line.level, line.text.as_str()),
            (Level::Conflict, "The file exists: Save asks what to do")
        );
        assert!(line.tooltip.contains("clip.mp4"));
        assert_eq!(
            card_check(&input("Clip - 02.mkv")).text,
            "Job #9 writes this file too: Save asks"
        );
        let hint = card_check(&input("Clip - 03.mkv"));
        assert_eq!(
            (hint.level, hint.text.as_str()),
            (Level::Hint, "2 files in the folder have similar names")
        );
        assert!(hint.can_save());
        let free = card_check(&CardInput {
            base: "Other",
            ..input("Other.mkv")
        });
        assert_eq!(free.level, Level::Free);
        let same = card_check(&CardInput {
            folder: Some(Path::new("/in")),
            ..input("Clip.mkv")
        });
        assert!(!same.can_save() && same.text.contains("source"));
        assert!(
            !card_check(&CardInput {
                source: None,
                ..input("a.mkv")
            })
            .can_save()
        );
        let missing = FolderScan::default();
        assert!(
            !card_check(&CardInput {
                scan: Some(&missing),
                ..input("a.mkv")
            })
            .can_save()
        );
        assert_eq!(
            card_check(&CardInput {
                scan: None,
                ..input("a.mkv")
            })
            .level,
            Level::Hint
        );
        let err = NameError::Empty;
        assert!(
            !card_check(&CardInput {
                file: Err(&err),
                ..input("a.mkv")
            })
            .can_save()
        );
    }
}
