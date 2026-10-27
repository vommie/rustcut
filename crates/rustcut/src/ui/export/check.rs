//! The card's live check of the target in words (a free name, a hint, a conflict that Save will ask about, or why the
//! job cannot be saved) and where its bubble stands (M6.5c, Q-N10: at the name field or the folder choice; a conflict
//! and similar names lead to the files). The words follow the user's review (P9.2): short titles, no list of names.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCheck {
    pub level: Level,
    pub text: String,
    pub detail: String,
    /// Where its bubble stands (`None` = no bubble: a free name, no video, the folder is being read).
    pub at: Option<At>,
    /// The bubble's link that shows the target folder's files (the conflict dialog to look): "Show the file" when the
    /// target exists, else "Show the files"; `None` = no link.
    pub browse: Option<&'static str>,
}

impl CardCheck {
    fn new(level: Level, text: impl Into<String>, detail: impl Into<String>, at: Option<At>) -> Self {
        Self {
            level,
            text: text.into(),
            detail: detail.into(),
            at,
            browse: None,
        }
    }

    const fn browsing(mut self, link: &'static str) -> Self {
        self.browse = Some(link);
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
    /// Names that are no hint: while the counter auto-iterates, the files of the name's series with a lower number are
    /// the jobs saved before ([`earlier_in_series`]; user note 2026-10-27).
    pub quiet: &'a [String],
}

/// The files of `names` that are no hint while the counter auto-iterates: the name's numbered series (`stem_for(n)`)
/// below `current`, and the name itself without a number (`base` with any extension), the files before the series
/// (user review 2026-10-28: "hd_h264_1080p.mkv" next to "hd_h264_1080p - 07" kept the bubble).
pub fn earlier_in_series<'a>(
    names: impl IntoIterator<Item = &'a String> + Clone,
    current: u32,
    base: &str,
    stem_for: impl Fn(u32) -> Option<String>,
) -> Vec<String> {
    let base = base.trim();
    let mut quiet: Vec<String> = rustcut_core::naming::series(names.clone(), stem_for)
        .into_iter()
        .filter(|f| f.counter < current)
        .map(|f| f.name)
        .collect();
    quiet.extend(
        names
            .into_iter()
            .filter(|n| {
                !base.is_empty()
                    && n.rsplit_once('.')
                        .is_some_and(|(stem, _)| stem.eq_ignore_ascii_case(base))
            })
            .cloned(),
    );
    quiet
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
        source: Some(source),
        scan,
        exists,
        queue: input.queue,
        warnings: input.warnings,
    });
    // The user's words (review 44#3), polished.
    let asks = "If you do not fix it here, Save opens a dialog that helps you resolve it.";
    if found.exists.is_some() {
        return CardCheck::new(Level::Conflict, "Target filename already exists", asks, Some(At::Name))
            .browsing("Show the file");
    }
    match found.queued.as_slice() {
        [] => {}
        [one] => {
            return CardCheck::new(
                Level::Conflict,
                format!("Job {} already uses the target filename", one.id),
                asks,
                Some(At::Name),
            )
            .browsing("Show the files");
        }
        many => {
            return CardCheck::new(
                Level::Conflict,
                format!("{} jobs already use the target filename", many.len()),
                asks,
                Some(At::Name),
            )
            .browsing("Show the files");
        }
    }
    let similar = found.similar.iter().filter(|n| !input.quiet.contains(n)).count();
    let title = match similar {
        0 => return CardCheck::new(Level::Free, "Free", folder.display().to_string(), None),
        1 => "1 similar filename in target folder".to_owned(),
        n => format!("{n} similar filenames in target folder"),
    };
    CardCheck::new(Level::Hint, title, "", Some(At::Name)).browsing("Show the files")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::job::{JobId, JobStateKind};

    use super::*;

    fn scan(names: &[&str]) -> FolderScan {
        FolderScan {
            readable: true,
            missing: false,
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
            quiet: &[],
        };
        let line = card_check(&input("Clip - 01.mkv"));
        assert_eq!(
            (line.level, line.text.as_str(), line.at, line.browse),
            (
                Level::Conflict,
                "Target filename already exists",
                Some(At::Name),
                Some("Show the file")
            )
        );
        let queued = card_check(&input("Clip - 02.mkv"));
        assert_eq!(
            (queued.text.as_str(), queued.browse),
            ("Job #9 already uses the target filename", Some("Show the files"))
        );
        let hint = card_check(&input("Clip - 03.mkv"));
        assert_eq!(
            (hint.level, hint.text.as_str(), hint.detail.as_str()),
            (Level::Hint, "2 similar filenames in target folder", "")
        );
        assert!(hint.can_save() && hint.browse == Some("Show the files"));
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

    #[test]
    fn earlier_numbers_of_the_series_are_no_hint_while_the_counter_iterates() {
        let warnings = WarningSettings::default();
        let folder_scan = scan(&["Clip - 01.mkv", "Clip - 02.mp4", "Clip - 05.mkv", "Clip extra.mkv"]);
        let stem = |n: u32| Some(format!("Clip - {n:02}"));
        let quiet = earlier_in_series(&folder_scan.names, 3, "Clip", stem);
        assert_eq!(quiet, vec!["Clip - 01.mkv".to_owned(), "Clip - 02.mp4".to_owned()]);
        let line = |quiet: &[String]| {
            card_check(&CardInput {
                source: Some(Path::new("/in/Clip.mkv")),
                folder: Some(Path::new("/out")),
                file: Ok(("", "Clip - 03.mkv")),
                base: "Clip",
                scan: Some(&folder_scan),
                queue: &[],
                warnings: &warnings,
                quiet,
            })
        };
        assert_eq!(line(&[]).text, "4 similar filenames in target folder");
        assert_eq!(
            line(&quiet).text,
            "2 similar filenames in target folder",
            "a higher number and another name still count"
        );
        let mut all = earlier_in_series(&folder_scan.names, 9, "Clip", stem);
        all.push("Clip extra.mkv".to_owned());
        assert_eq!(line(&all).level, Level::Free, "nothing else is similar: free");
    }

    #[test]
    fn the_name_without_a_number_is_no_hint_either() {
        let names: Vec<String> = [
            "hd - 01.mkv",
            "hd - 06.mkv",
            "hd.mkv",
            "HD.mp4",
            "hd extra.mkv",
            "hd - 07.mkv",
        ]
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
        let quiet = earlier_in_series(&names, 7, "hd", |n| Some(format!("hd - {n:02}")));
        assert_eq!(quiet, ["hd - 01.mkv", "hd - 06.mkv", "hd.mkv", "HD.mp4"]);
    }
}
