//! The known-source banner (parity: PyCutRv's known-source warning, without its modal box). The bridge clears the
//! banner when a file opens; once the open file's MD5 is known and the known-source notice is not routed to "nothing"
//! (`[notices.kinds.known_source]`), [`lookup`] asks RustView (`check-hash` through the sync actor, on the runtime) and
//! the bridge shows the [`view`] of the answer with [`show`] if that file is still open. PyCutRv's rule (0.0.68): a
//! video is known when RustView's answer says so (its hash was registered by an earlier open or job in RustCut or
//! PyCutRv, or a library file carries it); an unknown video is registered right away (the bridge sends
//! `insert-hash` while the Tag & Rate panel is "Active"), so this first open shows nothing and the next one warns.
//! When RustView knows files made from it, the banner's "Show N files" opens the dialog with them (M6.5d, user
//! decision Q-N10; [`install_dialog`]). A file RustCut opens itself (a job loaded into the editor) shows no banner.

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

/// The banner for RustView's answer about `source`; `None` when RustView does not know it (or the call did not
/// succeed). Known = RustView's own rule, as PyCutRv reads it: the hash is in RustView's hash list or a library file
/// carries it (also the source's own record). The dialog lists the other files.
pub fn view(check: &HashCheck, source: &Path) -> Option<KnownSourceView> {
    if !check.success || !check.known {
        return None;
    }
    let mut itself = false;
    let mut others = Vec::new();
    for file in &check.associated_files {
        if same_path(Path::new(file.as_str()), source) {
            itself = true;
        } else {
            others.push(PathBuf::from(file.as_str()));
        }
    }
    let mut detail = Vec::new();
    if let Some(date) = check.date_time.as_deref().filter(|d| !d.trim().is_empty()) {
        // RustView stores the time the hash was registered (the first open or job) in UTC.
        detail.push(format!("Known since {date} UTC."));
    }
    if itself {
        detail.push("The video itself is in the library.".to_owned());
    }
    detail.push(match others.len() {
        0 => "No other file in the library was made from it.".to_owned(),
        1 => "1 file in the library was made from it.".to_owned(),
        n => format!("{n} files in the library were made from it."),
    });
    Some(KnownSourceView {
        title: "RustView knows this video".into(),
        detail: detail.join(" "),
        files: others,
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

/// Asks RustView whether it knows `source` (with this MD5; runtime): the banner, and whether RustView knows the hash
/// (`None` = no answer). An unreachable RustView shows nothing; its state has one place (the Tag & Rate panel).
pub async fn lookup(sync: &SyncHandle, source: &Path, md5: &str) -> (Option<KnownSourceView>, Option<bool>) {
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
            (shown, check.success.then_some(check.known))
        }
        Ok(Err(e)) => {
            tracing::info!(error = %e, "[Media] Known-source check not possible");
            (None, None)
        }
        Err(_gone) => (None, None),
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
    }

    #[test]
    fn a_registered_hash_or_a_library_record_is_known_like_in_pycutrv() {
        let source = Path::new(SOURCE);
        let opened_before = view(&check(true, 0), source).unwrap();
        assert_eq!(opened_before.title, "RustView knows this video");
        assert_eq!(
            opened_before.detail,
            "Known since 2026-10-01 12:30:00 UTC. No other file in the library was made from it."
        );
        assert!(opened_before.files.is_empty());
        let only_itself = HashCheck {
            hash_id: None,
            date_time: None,
            associated_files: vec![SOURCE.into()],
            ..check(true, 0)
        };
        let itself = view(&only_itself, source).expect("the source's own library record makes it known (PyCutRv)");
        assert_eq!(
            itself.detail,
            "The video itself is in the library. No other file in the library was made from it."
        );
        assert!(itself.files.is_empty(), "the dialog lists only other files");
    }

    #[test]
    fn known_sources_list_their_files() {
        let source = Path::new(SOURCE);
        let one = view(&check(true, 1), source).unwrap();
        assert_eq!(
            one.detail,
            "Known since 2026-10-01 12:30:00 UTC. 1 file in the library was made from it."
        );
        assert_eq!(one.files, [PathBuf::from("/lib/clip - 01.mkv")]);
        let mut five = check(true, 5);
        five.associated_files.push(SOURCE.into());
        let many = view(&five, source).unwrap();
        assert!(
            many.detail
                .ends_with("The video itself is in the library. 5 files in the library were made from it.")
        );
        assert_eq!(many.files.len(), 5, "every file, the source's own record left out");
    }
}
