//! The card's live check of the target in words (a free name, a hint, a conflict that Save will ask about, or why the
//! job cannot be saved) and where its bubble stands (M6.5c, Q-N10: at the name field or the folder choice; a conflict
//! and similar names lead to the files).

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

/// The input a check is about: its bubble stands there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Name,
    Folder,
}

/// How many similar names a bubble lists before "and N more".
const LISTED: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCheck {
    pub level: Level,
    pub text: String,
    pub detail: String,
    /// Where its bubble stands (`None` = no bubble: a free name, no video, the folder is being read).
    pub at: Option<At>,
    /// A click on the bubble shows the target folder's files (the conflict dialog to look).
    pub browse: bool,
}

impl CardCheck {
    fn new(level: Level, text: impl Into<String>, detail: impl Into<String>, at: Option<At>) -> Self {
        Self {
            level,
            text: text.into(),
            detail: detail.into(),
            at,
            browse: false,
        }
    }

    const fn browsing(mut self) -> Self {
        self.browse = true;
        self
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
        return CardCheck::new(Level::Error, "Open a video first", "", None);
    };
    let Some(folder) = input.folder else {
        return CardCheck::new(Level::Error, "No target folder", "", Some(At::Folder));
    };
    let file = match input.file {
        Ok((_, file)) => file,
        Err(e) => return CardCheck::new(Level::Error, "The name cannot be used", e.to_string(), Some(At::Name)),
    };
    let target = folder.join(file);
    if same_path(source, &target) {
        return CardCheck::new(
            Level::Error,
            "This is the source file",
            target.display().to_string(),
            Some(At::Name),
        );
    }
    let Some(scan) = input.scan else {
        return CardCheck::new(
            Level::Hint,
            "Checking the folder...",
            folder.display().to_string(),
            None,
        );
    };
    if !scan.readable {
        return CardCheck::new(
            Level::Error,
            "The folder does not exist or cannot be read",
            folder.display().to_string(),
            Some(At::Folder),
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
    let asks = "Save asks what to do.";
    if found.exists.is_some() {
        return CardCheck::new(Level::Conflict, "The file exists", asks, Some(At::Name)).browsing();
    }
    match found.queued.as_slice() {
        [] => {}
        [one] => {
            return CardCheck::new(
                Level::Conflict,
                format!("Job {} writes this file too", one.id),
                asks,
                Some(At::Name),
            )
            .browsing();
        }
        many => {
            return CardCheck::new(
                Level::Conflict,
                format!("{} queued jobs write this file", many.len()),
                asks,
                Some(At::Name),
            )
            .browsing();
        }
    }
    let title = match found.similar.len() {
        0 => return CardCheck::new(Level::Free, "Free", folder.display().to_string(), None),
        1 => "1 file in the folder has a similar name".to_owned(),
        n => format!("{n} files in the folder have similar names"),
    };
    let mut names = found
        .similar
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if found.similar.len() > LISTED {
        names = format!("{names} and {} more", found.similar.len() - LISTED);
    }
    CardCheck::new(Level::Hint, title, names, Some(At::Name)).browsing()
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
            (line.level, line.text.as_str(), line.at, line.browse),
            (Level::Conflict, "The file exists", Some(At::Name), true)
        );
        assert_eq!(card_check(&input("Clip - 02.mkv")).text, "Job #9 writes this file too");
        let hint = card_check(&input("Clip - 03.mkv"));
        assert_eq!(
            (hint.level, hint.text.as_str(), hint.detail.as_str()),
            (
                Level::Hint,
                "2 files in the folder have similar names",
                "Clip - 01.mkv, clip.mp4"
            )
        );
        assert!(hint.can_save() && hint.browse);
        let free = card_check(&CardInput {
            base: "Other",
            ..input("Other.mkv")
        });
        assert_eq!((free.level, free.at), (Level::Free, None));
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
        let unreadable = card_check(&CardInput {
            scan: Some(&missing),
            ..input("a.mkv")
        });
        assert!(!unreadable.can_save() && unreadable.at == Some(At::Folder));
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
