//! The known-source banner (parity: PyCutRv's known-source warning, without its modal box and without registering the
//! hash on open, bug 3). The bridge clears the banner when a file opens; once the open file's MD5 is known and the
//! known-source notice is not routed to "nothing" (`[notices.kinds.known_source]`), [`lookup`] asks RustView
//! (`check-hash` through the sync actor, on the runtime) and the bridge shows the [`view`] of the answer with [`show`]
//! if that file is still open. When RustView knows files made from it, the banner's "Show N files" opens the dialog
//! with them (M6.5d, user decision Q-N10; [`install_dialog`]); a source RustView knows without files gets the banner
//! alone. A file RustCut opens itself (a job loaded into the editor) shows no banner.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustcut_core::naming::same_path;
use rustcut_core::rustview::{HashCheck, SyncCommand, SyncHandle};
use rustcut_core::settings::{NoticePlace, Settings};

use crate::ui::notices::{self, Kind, Level, Notice};

use crate::ui::export::KnownList;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSourceView {
    pub title: String,
    pub detail: String,
    /// Every library file made from it (the dialog lists them).
    pub files: Vec<PathBuf>,
}

/// Opens the known-source dialog (the export bridge owns it).
type OpenDialog = Rc<dyn Fn(KnownList)>;

thread_local! {
    static DIALOG: RefCell<Option<OpenDialog>> = const { RefCell::new(None) };
}

/// The function the banner's "Show N files" runs (UI thread).
pub fn install_dialog(open: impl Fn(KnownList) + 'static) {
    DIALOG.with(|d| *d.borrow_mut() = Some(Rc::new(open)));
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
        1 => "1 file in the library was made from it.".to_owned(),
        n => format!("{n} files in the library were made from it."),
    });
    Some(KnownSourceView {
        title: "This video was cut before".into(),
        detail: detail.join(" "),
        files: others.iter().map(|f| PathBuf::from(f.as_str())).collect(),
    })
}

/// Whether the check runs at all: RustCut reads the source's MD5 only while the known-source notice shows somewhere.
pub fn wanted(settings: &Settings) -> bool {
    notices::route(Kind::KnownSource, Level::Warning, &settings.notices).place != NoticePlace::Nothing
}

/// Shows `view` as the known-source notice (a banner by default, M6.5), or ends it (UI thread).
pub fn show(view: Option<&KnownSourceView>) {
    match view {
        Some(view) => {
            let mut notice = Notice::new(Kind::KnownSource, view.title.as_str()).detail(view.detail.as_str());
            if !view.files.is_empty() {
                let list = KnownList {
                    title: view.title.clone(),
                    text: view.detail.clone(),
                    files: view.files.clone(),
                };
                let label = match view.files.len() {
                    1 => "Show the file".to_owned(),
                    n => format!("Show {n} files"),
                };
                notice = notice.action(label, move || {
                    if let Some(open) = DIALOG.with(|d| d.borrow().clone()) {
                        open(list.clone());
                    }
                });
            }
            notices::post(notice);
        }
        None => notices::clear_kind(Kind::KnownSource),
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
            "First cut on 2026-10-01 12:30:00 UTC. 1 file in the library was made from it."
        );
        assert_eq!(one.files, [PathBuf::from("/lib/clip - 01.mkv")]);
        let mut five = check(true, 5);
        five.associated_files.push(SOURCE.into());
        let many = view(&five, source).unwrap();
        assert!(many.detail.ends_with("5 files in the library were made from it."));
        assert_eq!(many.files.len(), 5, "every file, the source's own record left out");
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
