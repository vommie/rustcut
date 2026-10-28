//! Which target folders are missing, checked on the runtime (ADR-4: the UI thread never asks the file system; before
//! P11.1 the Settings page called `is_dir()` for every row while publishing). The UI reads the last result and is told
//! when it changed.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

#[derive(Debug, Default)]
pub struct FolderPresence {
    missing: RefCell<HashSet<PathBuf>>,
    /// A check runs; the paths asked for meanwhile wait in `next`.
    running: Cell<bool>,
    next: RefCell<Option<Vec<PathBuf>>>,
}

impl FolderPresence {
    /// The last check found no folder at `path` (unknown paths count as present until checked).
    pub fn is_missing(&self, path: &Path) -> bool {
        self.missing.borrow().contains(path)
    }

    /// Checks `paths` on the runtime; `changed` runs on the UI thread when the missing ones differ from before. Calls
    /// while a check runs are merged into one more check.
    pub fn refresh(
        self: &Rc<Self>,
        runtime: &tokio::runtime::Handle,
        paths: Vec<PathBuf>,
        changed: impl Fn() + Clone + 'static,
    ) {
        if self.running.get() {
            *self.next.borrow_mut() = Some(paths);
            return;
        }
        self.running.set(true);
        let task =
            runtime.spawn_blocking(move || paths.into_iter().filter(|p| !p.is_dir()).collect::<HashSet<PathBuf>>());
        let weak: Weak<Self> = Rc::downgrade(self);
        let runtime = runtime.clone();
        let spawned = slint::spawn_local(async move {
            let found = task.await;
            let Some(me) = weak.upgrade() else { return };
            me.running.set(false);
            if let Ok(found) = found
                && *me.missing.borrow() != found
            {
                *me.missing.borrow_mut() = found;
                changed();
            }
            let next = me.next.borrow_mut().take();
            if let Some(paths) = next {
                me.refresh(&runtime, paths, changed);
            }
        });
        if let Err(e) = spawned {
            self.running.set(false);
            tracing::error!(error = %e, "[Folders] The event loop is gone; the folders are not checked");
        }
    }
}
