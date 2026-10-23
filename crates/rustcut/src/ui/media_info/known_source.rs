//! The known-source banner (parity: PyCutRv's known-source warning, without its modal box and without registering the
//! hash on open, bug 3). The hook is ready: the bridge clears the banner when a file opens and calls
//! [`source_hash_ready`] when the open file's MD5 is known. The lookup itself (`SyncCommand::CheckHash`, i.e.
//! `rustview api check-hash`) needs the RustView client and arrives with M3.1; it then calls [`show`] with the [`view`] of
//! the answer when `warnings.known_source` is on.

use std::path::Path;

use rustcut_core::rustview::HashCheck;
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::{KnownSource, MainWindow};

/// Library files listed in the banner; the detail line gives the full count.
const LISTED_FILES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSourceView {
    pub title: String,
    pub detail: String,
    pub files: Vec<String>,
}

/// The banner for RustView's answer; `None` when the source is not known (or the call did not succeed).
pub fn view(check: &HashCheck) -> Option<KnownSourceView> {
    if !check.success || !check.known {
        return None;
    }
    let count = check.associated_files.len();
    let mut detail = Vec::new();
    if let Some(date) = check.date_time.as_deref().filter(|d| !d.trim().is_empty()) {
        detail.push(format!("RustView first saw it on {date}."));
    }
    detail.push(match count {
        0 => "No file in the library was made from it.".to_owned(),
        1 => "1 file in the library was made from it:".to_owned(),
        n => format!("{n} files in the library were made from it:"),
    });
    let mut files: Vec<String> = check.associated_files.iter().take(LISTED_FILES).cloned().collect();
    if count > LISTED_FILES {
        files.push(format!("and {} more", count - LISTED_FILES));
    }
    Some(KnownSourceView {
        title: "This video was cut before".into(),
        detail: detail.join(" "),
        files,
    })
}

/// Shows `view` in the banner, or hides it (UI thread).
pub fn show(window: &MainWindow, view: Option<&KnownSourceView>) {
    let banner = window.global::<KnownSource<'_>>();
    match view {
        Some(view) => {
            banner.set_title(view.title.as_str().into());
            banner.set_detail(view.detail.as_str().into());
            let files: Vec<SharedString> = view.files.iter().map(|f| f.as_str().into()).collect();
            banner.set_files(ModelRc::new(VecModel::from(files)));
            banner.set_shown(true);
        }
        None => banner.set_shown(false),
    }
}

/// The open file's MD5 is known (runtime). This is where M3.1 asks RustView (`SyncCommand::CheckHash`) and posts
/// [`show`] with the [`view`] of the answer through the `UiDispatcher`.
pub fn source_hash_ready(path: &Path, md5: &str) {
    tracing::debug!(
        path = %path.display(),
        md5,
        "[Media] Source hash ready; the known-source check needs the RustView client (M3.1)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(known: bool, files: usize) -> HashCheck {
        HashCheck {
            success: true,
            known,
            hash: "8f618d1ebd606025257057ee18536e64".into(),
            hash_id: Some(7),
            date_time: Some("2026-10-01 12:30:00".into()),
            associated_files: (1..=files).map(|i| format!("/lib/clip - {i:02}.mkv")).collect(),
        }
    }

    #[test]
    fn unknown_sources_show_nothing() {
        assert_eq!(view(&check(false, 0)), None);
        let failed = HashCheck {
            success: false,
            ..check(true, 1)
        };
        assert_eq!(view(&failed), None);
    }

    #[test]
    fn known_sources_list_a_few_files() {
        let one = view(&check(true, 1)).unwrap();
        assert_eq!(one.title, "This video was cut before");
        assert_eq!(
            one.detail,
            "RustView first saw it on 2026-10-01 12:30:00. 1 file in the library was made from it:"
        );
        assert_eq!(one.files, ["/lib/clip - 01.mkv"]);
        let many = view(&check(true, 5)).unwrap();
        assert!(many.detail.ends_with("5 files in the library were made from it:"));
        assert_eq!(many.files.len(), 4);
        assert_eq!(many.files.last().map(String::as_str), Some("and 2 more"));
        let none = view(&HashCheck {
            date_time: None,
            ..check(true, 0)
        })
        .unwrap();
        assert_eq!(none.detail, "No file in the library was made from it.");
    }
}
