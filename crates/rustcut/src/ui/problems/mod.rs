//! A start that cannot go on as usual (M6.5d, user decision Q-N10): the dialog about settings files from a newer
//! RustCut (`newer.rs`) and the dialog about an unusable queue database (`database.rs`), both modal over the main
//! window right after it shows (the newer files first). Every way that writes backs the file up first; a file that
//! cannot be backed up is left alone. The database's ways run on the runtime: the rescued or fresh database replaces
//! the temporary one in the store thread (`StoreCommand::Replace`) and the queue reads it again
//! (`QueueCommand::Reload`). While the queue stays temporary the queue panel says so in red, and quitting with
//! unfinished jobs asks first (held).

pub mod database;
pub mod newer;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use rustcut_core::queue::QueueCommand;
use rustcut_core::settings::{AppPaths, Settings, UiState, backup_file, save};
use rustcut_core::store::{Problem, Store, StoreCommand, StoreError, StoreHandle, rescue, start_empty};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use self::database::{Choice as DbChoice, DatabaseDialog};
use self::newer::{Choice as NewerChoice, File, NewerDialog};
use crate::app::SettingsWriter;
use crate::ui::notices::{self, Kind, Level, Notice};
use crate::ui::power::PowerBridge;
use crate::ui::queue::QueueBridge;
use crate::{ConflictOptionView, MainWindow, Problems, Queue};

/// Why the queue database cannot be used (from the app's start).
#[derive(Debug, Clone)]
pub struct StoreProblem {
    pub problem: Problem,
    pub error: String,
}

/// What the dialogs act on.
pub struct ProblemParts<'a> {
    pub runtime: tokio::runtime::Handle,
    pub paths: &'a AppPaths,
    pub writer: SettingsWriter,
    pub ui_read_only: Rc<Cell<bool>>,
    pub store: StoreHandle,
    pub queue: rustcut_core::actor::Handle<QueueCommand>,
    pub queue_bridge: &'a Rc<QueueBridge>,
    pub power: &'a Rc<PowerBridge>,
}

pub struct ProblemsBridge {
    me: Weak<Self>,
    window: slint::Weak<MainWindow>,
    runtime: tokio::runtime::Handle,
    paths: AppPaths,
    writer: SettingsWriter,
    ui_read_only: Rc<Cell<bool>>,
    store: StoreHandle,
    queue: rustcut_core::actor::Handle<QueueCommand>,
    queue_bridge: Weak<QueueBridge>,
    newer: RefCell<Option<NewerDialog>>,
    db: RefCell<Option<DatabaseDialog>>,
    /// The database dialog's last state, kept while the queue is temporary (the red line reopens it).
    temporary: RefCell<Option<DatabaseDialog>>,
}

impl ProblemsBridge {
    /// Shows the dialogs `newer` and `store` call for. The app owns the only strong reference.
    pub fn install(
        window: &MainWindow,
        parts: ProblemParts<'_>,
        newer: Vec<newer::NewerFile>,
        store: Option<StoreProblem>,
    ) -> Rc<Self> {
        let db = store.map(|s| DatabaseDialog::new(s.problem, s.error, parts.paths.database()));
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: window.as_weak(),
            runtime: parts.runtime,
            paths: parts.paths.clone(),
            writer: parts.writer,
            ui_read_only: parts.ui_read_only,
            store: parts.store,
            queue: parts.queue,
            queue_bridge: Rc::downgrade(parts.queue_bridge),
            newer: RefCell::new(NewerDialog::new(newer)),
            db: RefCell::new(db),
            temporary: RefCell::new(None),
        });
        connect(&bridge, window);
        let me = Weak::clone(&bridge.me);
        parts
            .power
            .set_loss_check(Box::new(move || me.upgrade().and_then(|b| b.loss())));
        if let Some(d) = bridge.newer.borrow().as_ref() {
            let names: Vec<&str> = d.files.iter().map(|f| f.file.name()).collect();
            tracing::warn!(files = ?names, "[App] Settings files from a newer RustCut");
            notices::post(
                Notice::new(
                    Kind::SettingsNewer,
                    format!("{} is from a newer RustCut", names.join(" and ")),
                )
                .detail(d.text()),
            );
        }
        if let Some(d) = bridge.db.borrow().as_ref() {
            notices::post(Notice::new(Kind::DatabaseUnusable, d.title()).detail(d.text()));
        }
        bridge.publish();
        bridge
    }

    /// What quitting would lose: the unfinished jobs of a temporary queue.
    fn loss(&self) -> Option<String> {
        self.temporary.borrow().as_ref()?;
        let status = self.queue_bridge.upgrade()?.status();
        let unfinished = status.open + status.held;
        (unfinished > 0).then(|| {
            let jobs = if unfinished == 1 {
                "1 unfinished job".to_owned()
            } else {
                format!("{unfinished} unfinished jobs")
            };
            format!(
                "The queue is temporary (its database cannot be used): {jobs} and every other job of this session are \
                 lost when RustCut quits."
            )
        })
    }

    // ---- the newer files

    fn newer_select(&self, index: usize) {
        if let Some(d) = self.newer.borrow_mut().as_mut() {
            d.choose(index);
        }
        self.publish();
    }

    fn newer_step(&self, delta: i32) {
        if let Some(d) = self.newer.borrow_mut().as_mut() {
            d.step(delta);
        }
        self.publish();
    }

    fn newer_tab(&self, tab: usize) {
        if let Some(d) = self.newer.borrow_mut().as_mut() {
            d.set_tab(tab);
        }
        self.publish();
    }

    /// Escape: continue without saving, on every tab.
    fn newer_escape(&self) {
        if let Some(d) = self.newer.borrow_mut().as_mut() {
            d.chosen.fill(NewerChoice::Continue);
        }
        self.newer_confirm();
    }

    fn newer_confirm(&self) {
        let Some(dialog) = self.newer.borrow_mut().take() else {
            return;
        };
        self.publish();
        if dialog.quits() {
            tracing::info!("[App] Quit from the dialog about newer settings files");
            self.quit();
            return;
        }
        for (file, choice) in dialog.outcome() {
            tracing::info!(file = file.file.name(), ?choice, "[App] Newer settings file");
            match choice {
                NewerChoice::Continue | NewerChoice::Quit => {}
                NewerChoice::SaveThis | NewerChoice::Defaults => self.replace_newer(file, choice),
            }
        }
        self.focus_keys();
    }

    /// Backs `file` up, then writes it with this version or its defaults (nothing happens when the backup fails).
    fn replace_newer(&self, file: &newer::NewerFile, choice: NewerChoice) {
        let backup = match backup_file(&file.path, &self.paths.config_backups(), "newer") {
            Ok(backup) => backup,
            Err(e) => {
                tracing::error!(error = %e, file = file.file.name(), "[App] The newer file could not be backed up; left alone");
                notices::post(
                    Notice::new(Kind::SettingsReadOnly, format!("{} was left alone", file.file.name()))
                        .level(Level::Error)
                        .detail(format!("The copy could not be written: {e}")),
                );
                return;
            }
        };
        let what = match (file.file, choice) {
            (File::Settings, NewerChoice::Defaults) => {
                self.writer.make_writable();
                self.writer.update(|s| *s = Settings::default());
                "back to the defaults"
            }
            (File::Settings, _) => {
                self.writer.make_writable();
                "saved with this version"
            }
            (File::UiState, NewerChoice::Defaults) => {
                // This run keeps its layout and writes nothing at quit; the next start begins from the defaults.
                if let Err(e) = save(&self.paths.config_dir, &UiState::default()) {
                    tracing::warn!(error = %e, "[App] ui-state.toml could not be reset");
                }
                "reset for the next start"
            }
            (File::UiState, _) => {
                self.ui_read_only.set(false);
                "saved with this version when RustCut quits"
            }
        };
        notices::post(
            Notice::new(Kind::SettingsReplaced, format!("{} {what}", file.file.name()))
                .detail(format!("The copy: {}", backup.display())),
        );
    }

    // ---- the queue database

    fn db_select(&self, index: usize) {
        if let Some(d) = self.db.borrow_mut().as_mut() {
            d.select(index);
        }
        self.publish();
    }

    fn db_step(&self, delta: i32) {
        if let Some(d) = self.db.borrow_mut().as_mut() {
            d.step(delta);
        }
        self.publish();
    }

    fn db_escape(&self) {
        if let Some(d) = self.db.borrow_mut().as_mut() {
            d.selected = DbChoice::Temporary;
        }
        self.db_confirm();
    }

    fn db_confirm(&self) {
        let Some(choice) = self.db.borrow().as_ref().filter(|d| !d.busy).map(|d| d.selected) else {
            return;
        };
        tracing::info!(?choice, "[Store] Way out of the unusable database");
        match choice {
            DbChoice::Quit => {
                self.db.borrow_mut().take();
                self.publish();
                self.quit();
            }
            DbChoice::Temporary => {
                let dialog = self.db.borrow_mut().take();
                self.temporary.replace(dialog);
                self.publish();
                self.focus_keys();
            }
            DbChoice::Rescue | DbChoice::StartEmpty | DbChoice::Retry => self.run_db(choice),
        }
    }

    /// Runs a way that touches the file on the runtime; the dialog waits meanwhile. The jobs the temporary queue holds
    /// already go into the new database too.
    fn run_db(&self, choice: DbChoice) {
        let rendering = self.queue_bridge.upgrade().is_some_and(|q| q.status().active > 0);
        if let Some(d) = self.db.borrow_mut().as_mut() {
            if rendering {
                "A job renders in the temporary queue: wait until it is done or stop it.".clone_into(&mut d.failed);
            } else {
                d.busy = true;
                d.failed.clear();
            }
        }
        self.publish();
        if rendering {
            return;
        }
        let path = self.paths.database();
        let backups = self.paths.data_backups();
        let store = self.store.clone();
        let queue = self.queue.clone();
        let task = self.runtime.spawn(async move {
            let opened = tokio::task::spawn_blocking(move || open_with(choice, &path, &backups))
                .await
                .map_err(|e| e.to_string())?;
            let (fresh, words) = opened.map_err(|e| e.to_string())?;
            let session = store
                .request(|reply| StoreCommand::Jobs { reply })
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            store
                .request(|reply| StoreCommand::Replace {
                    store: Box::new(fresh),
                    reply,
                })
                .await
                .map_err(|_gone| "the database thread has stopped".to_owned())?;
            for job in session {
                let rustcut_core::store::SpecSlot::Decoded(spec) = job.spec else {
                    continue;
                };
                let kept = store
                    .request(|reply| StoreCommand::InsertJob {
                        spec,
                        state: job.state,
                        reply,
                    })
                    .await;
                if !matches!(kept, Ok(Ok(_))) {
                    tracing::warn!(job = job.id.0, "[Store] A job of the temporary queue could not be kept");
                }
            }
            let jobs = queue
                .request(|reply| QueueCommand::Reload { reply })
                .await
                .map_err(|_gone| "the queue has stopped".to_owned())?
                .map_err(|e| e.to_string())?;
            Ok::<(String, usize), String>((words, jobs))
        });
        let me = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            if let Some(bridge) = me.upgrade() {
                bridge.db_done(choice, result);
            }
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Store] The event loop is gone; the database way is not finished");
        }
    }

    fn db_done(&self, choice: DbChoice, result: Result<(String, usize), String>) {
        match result {
            Ok((words, jobs)) => {
                tracing::info!(?choice, jobs, "[Store] The queue database works again");
                self.db.borrow_mut().take();
                self.temporary.borrow_mut().take();
                let title = match choice {
                    DbChoice::Rescue if jobs == 0 => "No job could be rescued".to_owned(),
                    DbChoice::Rescue if jobs == 1 => "1 job rescued".to_owned(),
                    DbChoice::Rescue => format!("{jobs} jobs rescued"),
                    DbChoice::StartEmpty => "The queue starts empty".to_owned(),
                    _ => "The queue database works again".to_owned(),
                };
                notices::post(Notice::new(Kind::DatabaseRepaired, title).detail(words));
                self.publish();
                self.focus_keys();
            }
            Err(e) => {
                tracing::warn!(?choice, error = %e, "[Store] The way out did not work");
                if let Some(d) = self.db.borrow_mut().as_mut() {
                    d.busy = false;
                    d.failed = format!("{} did not work: {e}", choice.title());
                }
                self.publish();
            }
        }
    }

    /// The red line of the queue panel: the dialog again, with its last state.
    fn reopen_db(&self) {
        let dialog = self.temporary.borrow().clone();
        if let Some(mut dialog) = dialog {
            dialog.selected = match dialog.problem {
                Problem::Damaged => DbChoice::Rescue,
                Problem::Newer { .. } | Problem::NoAccess => DbChoice::Retry,
            };
            self.db.replace(Some(dialog));
            self.publish();
        }
    }

    fn db_folder(&self) {
        reveal(&self.paths.database());
    }

    fn db_copy(&self) {
        let details = self
            .db
            .borrow()
            .as_ref()
            .or(self.temporary.borrow().as_ref())
            .map(DatabaseDialog::details);
        if let Some(details) = details {
            crate::util::clipboard::copy(details);
            notices::post(Notice::new(Kind::LogCopied, "Details copied"));
        }
    }

    fn newer_folder(&self) {
        let path = self
            .newer
            .borrow()
            .as_ref()
            .and_then(|d| d.current().map(|f| f.path.clone()))
            .unwrap_or_else(|| self.paths.config_dir.join("config.toml"));
        reveal(&path);
    }

    fn quit(&self) {
        if let Some(window) = self.window.upgrade() {
            window.invoke_quit();
        }
    }

    fn focus_keys(&self) {
        if let Some(window) = self.window.upgrade() {
            window.invoke_focus_keys();
        }
    }

    // ---- publishing

    fn publish(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let p = window.global::<Problems<'_>>();
        // The newer files come first; the database dialog waits for them.
        let newer = self.newer.borrow();
        p.set_newer_open(newer.is_some());
        if let Some(d) = newer.as_ref() {
            p.set_newer_tabs(strings(d.files.iter().map(|f| f.file.name().to_owned())));
            p.set_newer_tab(i32::try_from(d.tab).unwrap_or(0));
            p.set_newer_text(d.text().into());
            p.set_newer_unknown_title(d.unknown_title().into());
            p.set_newer_unknown(strings(d.current().map(|f| f.unknown.clone()).unwrap_or_default()));
            p.set_newer_unknown_open(d.unknown_open);
            let options: Vec<ConflictOptionView> = NewerChoice::ALL
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    option(
                        i,
                        c.title(),
                        &d.current().map(|f| c.detail(f)).unwrap_or_default(),
                        true,
                        false,
                    )
                })
                .collect();
            p.set_newer_options(ModelRc::new(VecModel::from(options)));
            p.set_newer_selected(
                NewerChoice::ALL
                    .iter()
                    .position(|c| *c == d.choice())
                    .and_then(|i| i32::try_from(i).ok())
                    .unwrap_or(0),
            );
            p.set_newer_confirm_text(d.confirm_text().into());
        }
        let db = self.db.borrow();
        p.set_db_open(newer.is_none() && db.is_some());
        if let Some(d) = db.as_ref() {
            p.set_db_title(d.title().into());
            p.set_db_text(d.text().into());
            p.set_db_failed(d.failed.as_str().into());
            let options: Vec<ConflictOptionView> = DbChoice::ALL
                .iter()
                .enumerate()
                .map(|(i, c)| option(i, c.title(), &d.detail(*c), d.enabled(*c), c.destructive()))
                .collect();
            p.set_db_options(ModelRc::new(VecModel::from(options)));
            p.set_db_selected(
                DbChoice::ALL
                    .iter()
                    .position(|c| *c == d.selected)
                    .and_then(|i| i32::try_from(i).ok())
                    .unwrap_or(0),
            );
            p.set_db_confirm_text(d.selected.confirm().into());
            p.set_db_busy(d.busy);
        }
        window.global::<Queue<'_>>().set_temporary_line(
            self.temporary
                .borrow()
                .as_ref()
                .map(database::temporary_line)
                .unwrap_or_default()
                .into(),
        );
    }
}

/// The way `choice` opens the database at `path`, with what it did in words.
fn open_with(
    choice: DbChoice,
    path: &std::path::Path,
    backups: &std::path::Path,
) -> Result<(Store, String), StoreError> {
    match choice {
        DbChoice::Rescue => rescue(path, backups).map(|r| {
            let lost = match r.lost {
                0 => String::new(),
                1 => " 1 job could not be read.".to_owned(),
                n => format!(" {n} jobs could not be read."),
            };
            let words = if r.opened {
                format!("The old file is kept: {}.{lost}", r.kept.display())
            } else {
                format!("Nothing could be read from the old file (kept: {}).", r.kept.display())
            };
            (r.store, words)
        }),
        DbChoice::StartEmpty => start_empty(path, backups)
            .map(|(store, kept)| (store, format!("The old file is kept: {}.", kept.display()))),
        DbChoice::Retry | DbChoice::Temporary | DbChoice::Quit => {
            Store::open(path, backups).map(|store| (store, path.display().to_string()))
        }
    }
}

fn option(index: usize, title: &str, detail: &str, enabled: bool, destructive: bool) -> ConflictOptionView {
    ConflictOptionView {
        key: i32::try_from(index).unwrap_or(0),
        title: title.into(),
        file: SharedString::new(),
        detail: detail.into(),
        enabled,
        destructive,
        typed: false,
    }
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(SharedString::from).collect::<Vec<_>>(),
    ))
}

/// Shows `path` in the file manager (its folder when it is gone).
fn reveal(path: &std::path::Path) {
    let target: PathBuf = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent()
            .map_or_else(|| path.to_path_buf(), std::path::Path::to_path_buf)
    };
    if let Err(e) = opener::reveal(&target) {
        tracing::warn!(path = %target.display(), error = %e, "[App] The folder could not be shown");
    }
}

fn connect(bridge: &Rc<ProblemsBridge>, window: &MainWindow) {
    let p = window.global::<Problems<'_>>();
    macro_rules! on {
        ($setter:ident, |$b:ident $(, $arg:ident: $ty:ty)*| $body:expr) => {{
            let weak = Rc::downgrade(bridge);
            p.$setter(move |$($arg: $ty),*| {
                if let Some($b) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_newer_select, |b, i: i32| b
        .newer_select(usize::try_from(i).unwrap_or(usize::MAX)));
    on!(on_newer_step, |b, d: i32| b.newer_step(d));
    on!(on_newer_set_tab, |b, i: i32| b
        .newer_tab(usize::try_from(i).unwrap_or(0)));
    on!(on_newer_confirm, |b| b.newer_confirm());
    on!(on_newer_escape, |b| b.newer_escape());
    on!(on_newer_open_folder, |b| b.newer_folder());
    on!(on_db_select, |b, i: i32| b
        .db_select(usize::try_from(i).unwrap_or(usize::MAX)));
    on!(on_db_step, |b, d: i32| b.db_step(d));
    on!(on_db_confirm, |b| b.db_confirm());
    on!(on_db_escape, |b| b.db_escape());
    on!(on_db_open_folder, |b| b.db_folder());
    on!(on_db_copy, |b| b.db_copy());
    let weak = Rc::downgrade(bridge);
    window.global::<Queue<'_>>().on_temporary_clicked(move || {
        if let Some(b) = weak.upgrade() {
            b.reopen_db();
        }
    });
}
