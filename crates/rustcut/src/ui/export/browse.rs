//! "Show the files" of the name check's bubble (M6.5c, user decision Q-N10): the conflict dialog opens to look, not to
//! decide: the target file (or that none exists yet), the similar files with their pictures and the queued jobs that
//! write it; its only button closes it. The folder is read on the runtime first, like a save does.

use std::path::Path;
use std::rc::Rc;

use rustcut_core::naming::{CheckInput, ExistingFile, FolderScan, check, existing_file, scan_folder};

use super::ExportBridge;
use super::dialog::{ConflictDialog, DialogInput};
use super::save::Pending;
use super::state::JobFacts;

impl ExportBridge {
    /// Reads the target folder, then shows the dialog to look at it (nothing while a save or a dialog is open).
    pub(super) fn browse_target(self: &Rc<Self>) {
        if self.saving.get() || self.pending.borrow().is_some() || self.meta.borrow().is_some() {
            return;
        }
        let Some(facts) = self.facts() else { return };
        let (folder, file) = {
            let st = self.state.borrow();
            (st.target_folder(), st.file(&facts))
        };
        let (Some(folder), Ok((stem, name))) = (folder, file) else {
            return;
        };
        let target = folder.join(&name);
        let scanned = folder.clone();
        let task = self
            .runtime
            .spawn(async move { (scan_folder(&scanned).await, existing_file(&target).await) });
        let weak = Rc::downgrade(self);
        let spawned = slint::spawn_local(async move {
            let (Ok((scan, exists)), Some(bridge)) = (task.await, weak.upgrade()) else {
                return;
            };
            bridge.open_browse(&folder, &stem, &facts, scan, exists);
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Export] The event loop is gone; no file list");
        }
    }

    fn open_browse(
        self: &Rc<Self>,
        folder: &Path,
        stem: &str,
        facts: &JobFacts,
        scan: FolderScan,
        exists: Option<ExistingFile>,
    ) {
        if self.saving.get() || self.pending.borrow().is_some() {
            return;
        }
        let target = folder.join(rustcut_core::naming::file_name(stem, facts.container));
        let mut queue = self.queue.upgrade().map(|q| q.targets()).unwrap_or_default();
        let edited = self.editing.borrow().upgrade().and_then(|e| e.info()).map(|e| e.id);
        if let Some(id) = edited
            && self.updates(&target, false)
        {
            queue.retain(|q| q.id != id);
        }
        let base = self.state.borrow().name.clone();
        let mut conflicts = check(&CheckInput {
            target: &target,
            base: &base,
            scan: &scan,
            exists,
            queue: &queue,
            warnings: &self.warnings.borrow(),
        });
        // The dialog shows the existing file whatever the warnings say.
        conflicts.exists = exists;
        let dialog = {
            let st = self.state.borrow();
            ConflictDialog::new(DialogInput {
                folder,
                stem,
                conflicts,
                scan,
                queue,
                export: &st,
                facts,
                settings: &self.dialog_settings.borrow(),
            })
            .browsing()
        };
        tracing::info!(target = %target.display(), "[Export] Showing the target folder's files");
        *self.pending.borrow_mut() = Some(Pending { dialog, draft: None });
        self.publish_dialog();
        self.start_previews();
    }
}
