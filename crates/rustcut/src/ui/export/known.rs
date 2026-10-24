//! The known-source dialog (M6.5d, user decisions Q-N8 #11 and Q-N10): the banner's "Show N files" opens the conflict
//! dialog in a third mode. "Made from it" lists every file RustView knows was made from the open video (picture,
//! name, folder, length and size, date, rating, tags, "file missing", the queued job that writes it) with play and
//! reveal; "Similar files" the names in the card's target folder that look like the video's (the conflict dialog's
//! list, for cuts RustView never heard of). A playing video pauses while the dialog is open and plays on when it
//! closes, only if the dialog paused it. The files and RustView are read on the runtime first.

use std::path::PathBuf;
use std::rc::Rc;

use rustcut_core::naming::{CheckInput, FolderScan, check, same_path, scan_folder};
use rustcut_core::rustview::{GetMedia, SyncCommand};

use super::ExportBridge;
use super::dialog::{ConflictDialog, DialogInput, KnownFile, KnownFiles};
use super::save::Pending;

/// What the banner knows about the files of the open video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownList {
    pub title: String,
    pub text: String,
    pub files: Vec<PathBuf>,
}

impl ExportBridge {
    /// "Show N files": reads the files, RustView's records and the target folder, then shows the dialog (nothing while
    /// a save or another dialog is open).
    pub fn show_known(self: &Rc<Self>, list: KnownList) {
        if self.saving.get() || self.pending.borrow().is_some() || self.meta.borrow().is_some() {
            return;
        }
        if let Some(player) = self.player.upgrade()
            && !player.is_paused()
        {
            player.set_paused(true);
            self.resume_after_dialog.set(true);
        }
        let folder = self.state.borrow().target_folder();
        let sync = self.tags.upgrade().map(|t| t.sync_handle());
        let files = list.files.clone();
        let scanned = folder.clone();
        let task = self.runtime.spawn(async move {
            let scan = match &scanned {
                Some(folder) => scan_folder(folder).await,
                None => FolderScan::default(),
            };
            let mut found = Vec::with_capacity(files.len());
            for path in files {
                let missing = !tokio::fs::try_exists(&path).await.unwrap_or(false);
                let media = match &sync {
                    Some(sync) => sync
                        .request(|reply| SyncCommand::GetMedia {
                            path: path.clone(),
                            reply,
                        })
                        .await
                        .ok()
                        .and_then(Result::ok),
                    None => None,
                };
                found.push((path, missing, media));
            }
            (scan, found)
        });
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let Some(bridge) = weak.upgrade() else { return };
            match task.await {
                Ok((scan, found)) => bridge.open_known(&list, folder, scan, found),
                Err(e) => {
                    tracing::warn!(error = %e, "[Export] The known source's files could not be read");
                    bridge.resume_video();
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; no known-source dialog");
        }
    }

    fn open_known(
        self: &Rc<Self>,
        list: &KnownList,
        folder: Option<PathBuf>,
        scan: FolderScan,
        found: Vec<(PathBuf, bool, Option<GetMedia>)>,
    ) {
        if self.saving.get() || self.pending.borrow().is_some() {
            self.resume_video();
            return;
        }
        let queue = self.queue.upgrade().map(|q| q.targets()).unwrap_or_default();
        let files: Vec<KnownFile> = found
            .into_iter()
            .map(|(path, missing, media)| {
                let record = media.and_then(|m| m.media.filter(|_| m.found));
                let job = queue
                    .iter()
                    .find(|q| same_path(&q.target, &path))
                    .map(|q| format!("Job {}", q.id))
                    .unwrap_or_default();
                KnownFile {
                    rating: record.as_ref().map(|r| r.rating),
                    tags: record
                        .as_ref()
                        .map(|r| self.tags.upgrade().map(|t| t.names_of(&r.tags)).unwrap_or_default())
                        .unwrap_or_default(),
                    job,
                    missing,
                    path,
                }
            })
            .collect();
        let known = KnownFiles {
            title: list.title.clone(),
            text: list.text.clone(),
            files,
        };
        let facts = self.facts();
        let dialog = {
            let st = self.state.borrow();
            let stem = facts
                .as_ref()
                .and_then(|f| st.file(f).ok())
                .map_or_else(|| st.name.clone(), |(stem, _)| stem);
            let folder = folder.unwrap_or_default();
            let Some(facts) = facts else {
                drop(st);
                self.resume_video();
                return;
            };
            let target = folder.join(rustcut_core::naming::file_name(&stem, facts.container));
            let conflicts = check(&CheckInput {
                target: &target,
                base: &st.name,
                scan: &scan,
                exists: None,
                queue: &queue,
                warnings: &self.warnings.borrow(),
            });
            ConflictDialog::new(DialogInput {
                folder: &folder,
                stem: &stem,
                conflicts,
                scan,
                queue,
                export: &st,
                facts: &facts,
                settings: &self.dialog_settings.borrow(),
            })
            .knowing(known)
        };
        tracing::info!(
            files = dialog.known.as_ref().map_or(0, |k| k.files.len()),
            "[Export] Showing the files made from the open video"
        );
        *self.pending.borrow_mut() = Some(Pending { dialog, draft: None });
        self.publish_dialog();
        self.start_previews();
    }

    /// The dialog closed: a video it paused plays on.
    pub(super) fn resume_video(&self) {
        if self.resume_after_dialog.replace(false)
            && let Some(player) = self.player.upgrade()
        {
            player.set_paused(false);
        }
    }
}
