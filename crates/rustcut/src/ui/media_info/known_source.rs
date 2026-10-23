//! The known-source banner (parity: PyCutRv's known-source warning, without its modal box and without registering the
//! hash on open, bug 3). The bridge clears the banner when a file opens; once the open file's MD5 is known and
//! `warnings.known_source` is on, [`lookup`] asks RustView (`check-hash` through the sync actor, on the runtime) and the
//! bridge shows the [`view`] of the answer with [`show`] if that file is still open.

use std::path::Path;

use rustcut_core::naming::same_path;
use rustcut_core::rustview::{HashCheck, SyncCommand, SyncHandle};
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

/// The banner for RustView's answer about `source`; `None` when no job was made from it (or the call did not succeed).
/// RustView also lists the source's own record (the same MD5); that alone does not make it "cut before". A source is
/// known when its hash is in RustView's hash list (RustCut and PyCutRv register the sources of their jobs) or another
/// file in the library carries its hash (a pending output).
pub fn view(check: &HashCheck, source: &Path) -> Option<KnownSourceView> {
    let others: Vec<&String> = check
        .associated_files
        .iter()
        .filter(|f| !same_path(Path::new(f.as_str()), source))
        .collect();
    if !check.success || !check.known || (check.hash_id.is_none() && others.is_empty()) {
        return None;
    }
    let count = others.len();
    let mut detail = Vec::new();
    if let Some(date) = check.date_time.as_deref().filter(|d| !d.trim().is_empty()) {
        // RustView stores the time the hash was registered (when the first job was made) in UTC.
        detail.push(format!("First cut on {date} UTC."));
    }
    detail.push(match count {
        0 => "No file in the library was made from it.".to_owned(),
        1 => "1 file in the library was made from it:".to_owned(),
        n => format!("{n} files in the library were made from it:"),
    });
    let mut files: Vec<String> = others.iter().take(LISTED_FILES).map(|f| (*f).clone()).collect();
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

/// Asks RustView whether `source` (with this MD5) was cut before (runtime). An unreachable RustView shows nothing; its
/// state has one place (the Tag & Rate panel).
pub async fn lookup(sync: &SyncHandle, source: &Path, md5: &str) -> Option<KnownSourceView> {
    let answer = sync
        .request(|reply| SyncCommand::CheckHash {
            md5: md5.to_owned(),
            reply,
        })
        .await;
    match answer {
        Ok(Ok(check)) => {
            let shown = view(&check, source);
            tracing::info!(
                path = %source.display(),
                known = shown.is_some(),
                files = check.associated_files.len(),
                "[Media] Known-source check"
            );
            shown
        }
        Ok(Err(e)) => {
            tracing::info!(error = %e, "[Media] Known-source check not possible");
            None
        }
        Err(_gone) => None,
    }
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

    const SOURCE: &str = "/videos/source.mp4";

    #[test]
    fn unknown_sources_show_nothing() {
        let source = Path::new(SOURCE);
        assert_eq!(view(&check(false, 0), source), None);
        let failed = HashCheck {
            success: false,
            ..check(true, 1)
        };
        assert_eq!(view(&failed, source), None);
        let only_itself = HashCheck {
            hash_id: None,
            date_time: None,
            associated_files: vec![SOURCE.into()],
            ..check(true, 0)
        };
        assert_eq!(view(&only_itself, source), None, "the source's own library record");
    }

    #[test]
    fn known_sources_list_a_few_files() {
        let source = Path::new(SOURCE);
        let one = view(&check(true, 1), source).unwrap();
        assert_eq!(one.title, "This video was cut before");
        assert_eq!(
            one.detail,
            "First cut on 2026-10-01 12:30:00 UTC. 1 file in the library was made from it:"
        );
        assert_eq!(one.files, ["/lib/clip - 01.mkv"]);
        let mut five = check(true, 5);
        five.associated_files.push(SOURCE.into());
        let many = view(&five, source).unwrap();
        assert!(many.detail.ends_with("5 files in the library were made from it:"));
        assert_eq!(many.files.len(), 4);
        assert_eq!(many.files.last().map(String::as_str), Some("and 2 more"));
        let none = view(
            &HashCheck {
                date_time: None,
                ..check(true, 0)
            },
            source,
        )
        .unwrap();
        assert_eq!(none.detail, "No file in the library was made from it.");
    }
}
