//! The folder navigator's bridge (Q-NF3): opens the bubble on the element the user clicked, follows the folder view's
//! listings, runs the search on the runtime, reads the shown files' sizes on the runtime, and turns the pure state into
//! the `FolderNavigator` global. The bubble stays open after a file opens; its list follows the new open video.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::pattern::{Pattern, PatternError};
use super::state::{self, Navigator, Slot};
use crate::ui::plugins::commands::{BRIDGE, CommandsBridge};
use crate::{FolderNavigator, NavRow, NavSegment, NavSlot};

/// What the bridge keeps besides the pure state.
#[derive(Default)]
struct Shared {
    nav: Navigator,
    open: bool,
    /// The folder of the last listing; another folder starts with an empty search.
    folder: Option<PathBuf>,
    query: String,
    regex: bool,
    /// The last search that compiled (the list stays at it while the typed one does not).
    pattern: Option<Pattern>,
    error: Option<PatternError>,
    /// Counts the searches: the hits of an older one are dropped.
    search_run: u64,
    /// The shown files' sizes, read on the runtime; cleared with the folder.
    sizes: HashMap<PathBuf, Option<u64>>,
    /// "Modified, newest first" / "Names A to Z".
    order: String,
}

pub(in crate::ui::plugins::commands) struct NavigatorUi {
    shared: RefCell<Shared>,
    rows: Rc<VecModel<NavRow>>,
    slots: Rc<VecModel<NavSlot>>,
}

impl NavigatorUi {
    pub fn new(regex: bool, rows: usize) -> Self {
        Self {
            shared: RefCell::new(Shared {
                nav: Navigator::new(rows),
                regex,
                ..Shared::default()
            }),
            rows: Rc::new(VecModel::default()),
            slots: Rc::new(VecModel::default()),
        }
    }

    pub fn regex(&self) -> bool {
        self.shared.borrow().regex
    }
}

impl CommandsBridge {
    pub(in crate::ui::plugins::commands) fn install_navigator(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let global = window.global::<FolderNavigator<'_>>();
        global.set_regex(self.navigator.regex());
        global.set_rows(ModelRc::from(Rc::clone(&self.navigator.rows)));
        global.set_slots(ModelRc::from(Rc::clone(&self.navigator.slots)));
        global.on_query_edited(|text| with(|b| b.search(text.as_str())));
        global.on_regex_toggled(|on| {
            with(|b| {
                b.navigator.shared.borrow_mut().regex = on;
                let query = b.navigator.shared.borrow().query.clone();
                b.search(&query);
            });
        });
        global.on_key(|key| with(|b| b.navigator_key(key.as_str())));
        global.on_hovered(|row| {
            with(|b| {
                b.navigator
                    .shared
                    .borrow_mut()
                    .nav
                    .hover(usize::try_from(row).unwrap_or(0));
                b.publish_navigator();
            });
        });
        global.on_activate(|row| with(|b| b.open_row(usize::try_from(row).ok())));
        global.on_page(|page| {
            with(|b| {
                b.navigator
                    .shared
                    .borrow_mut()
                    .nav
                    .go_to_page(usize::try_from(page).unwrap_or(0));
                b.publish_navigator();
            });
        });
        global.on_page_step(|step| {
            with(|b| {
                let mut shared = b.navigator.shared.borrow_mut();
                let page = shared.nav.view(None).page;
                let to = if step < 0 {
                    page.saturating_sub(1)
                } else {
                    page.saturating_add(1)
                };
                shared.nav.go_to_page(to);
                drop(shared);
                b.publish_navigator();
            });
        });
        global.on_to_current(|| with(CommandsBridge::navigator_to_current));
        global.on_closed(|| {
            with(|b| {
                b.navigator.shared.borrow_mut().open = false;
                tracing::debug!("[NextInFolder] The folder navigator closed");
            });
        });
    }

    /// A click on a plugin button, the bar or the list button: opens the navigator there, or closes it when open.
    pub(in crate::ui::plugins::commands) fn toggle_navigator(&self, anchor: [f32; 4], rows: usize) {
        let Some(window) = self.window.upgrade() else { return };
        let global = window.global::<FolderNavigator<'_>>();
        if global.get_open() {
            global.set_open(false);
            self.navigator.shared.borrow_mut().open = false;
            window.invoke_focus_keys();
            return;
        }
        if !self.navigator_listing() {
            return;
        }
        {
            let mut shared = self.navigator.shared.borrow_mut();
            shared.open = true;
            shared.nav.set_rows(rows);
            shared.nav.show_current();
        }
        let [x, y, width, height] = anchor;
        global.set_anchor_x(x);
        global.set_anchor_y(y);
        global.set_anchor_width(width);
        global.set_anchor_height(height);
        global.set_rows_per_page(i32::try_from(rows).unwrap_or(15));
        self.publish_navigator();
        global.set_open(true);
        tracing::info!("[NextInFolder] The folder navigator opened");
    }

    /// The folder view read the folder again (a video opened, the folder changed): the list follows. Returns whether
    /// there is a listing.
    pub(in crate::ui::plugins::commands) fn navigator_listing(&self) -> bool {
        let Some(snapshot) = self.located_snapshot() else {
            return false;
        };
        let same_folder = self.navigator.shared.borrow().folder.as_deref() == Some(snapshot.folder.as_path());
        {
            let mut shared = self.navigator.shared.borrow_mut();
            if !same_folder {
                shared.folder = Some(snapshot.folder.clone());
                shared.query.clear();
                shared.pattern = None;
                shared.error = None;
                shared.sizes.clear();
                if let Some(window) = self.window.upgrade() {
                    window.global::<FolderNavigator<'_>>().set_query("".into());
                }
            }
            shared.order = snapshot.order;
            let folder = snapshot.folder;
            shared.nav.set_files(snapshot.files, snapshot.current);
            if !same_folder || !shared.open {
                shared.nav.show_current();
            }
            set_folder_names(self, &folder);
        }
        let pattern = self.navigator.shared.borrow().pattern.clone();
        if let Some(pattern) = pattern {
            self.run_search(pattern, false);
        } else if self.navigator.shared.borrow().open {
            self.publish_navigator();
        }
        true
    }

    /// The typed search (or the regex switch changed).
    fn search(&self, text: &str) {
        let regex = {
            let mut shared = self.navigator.shared.borrow_mut();
            text.clone_into(&mut shared.query);
            shared.regex
        };
        match Pattern::parse(text, regex) {
            Err(error) => {
                self.navigator.shared.borrow_mut().error = Some(error);
                self.publish_navigator();
            }
            Ok(None) => {
                let mut shared = self.navigator.shared.borrow_mut();
                shared.error = None;
                shared.pattern = None;
                shared.search_run = shared.search_run.wrapping_add(1);
                shared.nav.set_hits(None);
                shared.nav.show_current();
                drop(shared);
                self.publish_navigator();
            }
            Ok(Some(pattern)) => {
                {
                    let mut shared = self.navigator.shared.borrow_mut();
                    shared.error = None;
                    shared.pattern = Some(pattern.clone());
                }
                self.run_search(pattern, true);
            }
        }
    }

    /// Filters the folder's names on the runtime (a folder may hold 100 000 files); `fresh`: a new search starts at
    /// its first hit, a new listing keeps the place.
    fn run_search(&self, pattern: Pattern, fresh: bool) {
        let (names, run) = {
            let mut shared = self.navigator.shared.borrow_mut();
            shared.search_run = shared.search_run.wrapping_add(1);
            (Arc::clone(shared.nav.names()), shared.search_run)
        };
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let Ok(hits) = tokio::task::spawn_blocking(move || state::filter(&names, &pattern)).await else {
                return;
            };
            ui.post(move || {
                with(|b| {
                    let mut shared = b.navigator.shared.borrow_mut();
                    if shared.search_run != run {
                        return;
                    }
                    shared.nav.set_hits(Some(hits));
                    if fresh {
                        shared.nav.home();
                    }
                    drop(shared);
                    b.publish_navigator();
                });
            });
        });
    }

    fn navigator_key(&self, key: &str) {
        {
            let mut shared = self.navigator.shared.borrow_mut();
            let rows = isize::try_from(shared.nav.page_paths().len().max(1)).unwrap_or(15);
            match key {
                "up" => shared.nav.step(-1),
                "down" => shared.nav.step(1),
                "page-up" => shared.nav.step(-rows),
                "page-down" => shared.nav.step(rows),
                "home" => shared.nav.home(),
                "end" => shared.nav.end(),
                "open" => {
                    let path = shared.nav.highlighted().cloned();
                    drop(shared);
                    self.open_path(path);
                    return;
                }
                _ => return,
            }
        }
        self.publish_navigator();
    }

    fn open_row(&self, row: Option<usize>) {
        let path = row.and_then(|row| self.navigator.shared.borrow().nav.page_paths().get(row).cloned());
        self.open_path(path);
    }

    fn open_path(&self, path: Option<PathBuf>) {
        let (Some(path), Some(opener)) = (path, self.opener.upgrade()) else {
            return;
        };
        tracing::info!("[NextInFolder] The navigator opens {}", path.display());
        opener.open(path);
    }

    fn navigator_to_current(&self) {
        let clear = !self.navigator.shared.borrow().nav.current_is_hit();
        if clear {
            if let Some(window) = self.window.upgrade() {
                window.global::<FolderNavigator<'_>>().set_query("".into());
            }
            self.search("");
        }
        self.navigator.shared.borrow_mut().nav.show_current();
        self.publish_navigator();
    }

    /// The page into the global (only what changed is replaced) and the missing sizes asked for.
    pub(in crate::ui::plugins::commands) fn publish_navigator(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let global = window.global::<FolderNavigator<'_>>();
        let shared = self.navigator.shared.borrow();
        let view = shared.nav.view(shared.pattern.as_ref());
        let files = shared.nav.files();
        let rows: Vec<NavRow> = view
            .rows
            .iter()
            .map(|row| NavRow {
                number: (row.index + 1).to_string().into(),
                segments: ModelRc::new(VecModel::from(
                    row.segments
                        .iter()
                        .map(|s| NavSegment {
                            text: s.text.as_str().into(),
                            hit: s.hit,
                            shrink: s.shrink,
                        })
                        .collect::<Vec<_>>(),
                )),
                size: files
                    .get(row.index)
                    .and_then(|p| shared.sizes.get(p).copied().flatten())
                    .map(|bytes| humansize::format_size(bytes, humansize::BINARY.decimal_places(1).decimal_zeroes(0)))
                    .unwrap_or_default()
                    .into(),
                current: row.current,
                highlighted: row.highlighted,
            })
            .collect();
        let slots: Vec<NavSlot> = view
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Page(p) => NavSlot {
                    label: (p + 1).to_string().into(),
                    page: i32::try_from(*p).unwrap_or(0),
                    on: *p == view.page,
                    has_current: Some(*p) == view.current_page,
                },
                Slot::Gap => NavSlot {
                    label: "".into(),
                    page: -1,
                    on: false,
                    has_current: false,
                },
            })
            .collect();
        let pages = shared.nav.pages();
        crate::util::model::sync(&self.navigator.rows, rows);
        crate::util::model::sync(&self.navigator.slots, slots);
        global.set_range(view.range.as_str().into());
        global.set_has_prev(view.page > 0);
        global.set_has_next(view.page + 1 < pages);
        global.set_can_current(shared.nav.has_current());
        global.set_widest_number(files.len().max(1).to_string().into());
        global.set_widest_page(pages.to_string().into());
        global.set_order(shared.order.as_str().into());
        let searching = shared.pattern.is_some();
        global.set_badge(
            if searching {
                shared.nav.found().to_string()
            } else {
                String::new()
            }
            .into(),
        );
        global.set_empty_text(if searching { "No file matches" } else { "No files" }.into());
        global.set_invalid(shared.error.is_some());
        global.set_error(shared.error.as_ref().map_or("", |e| e.message.as_str()).into());
        global.set_error_example(shared.error.as_ref().map_or("", |e| e.example.as_str()).into());
        let missing: Vec<PathBuf> = shared
            .nav
            .page_paths()
            .into_iter()
            .filter(|p| !shared.sizes.contains_key(p))
            .collect();
        drop(shared);
        if !missing.is_empty() {
            self.read_sizes(missing);
        }
    }

    fn read_sizes(&self, paths: Vec<PathBuf>) {
        {
            let mut shared = self.navigator.shared.borrow_mut();
            for path in &paths {
                shared.sizes.insert(path.clone(), None);
            }
        }
        let ui = self.ui.clone();
        self.runtime.spawn(async move {
            let read = tokio::task::spawn_blocking(move || {
                paths
                    .into_iter()
                    .map(|p| {
                        let size = std::fs::metadata(&p).ok().map(|m| m.len());
                        (p, size)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let Ok(read) = read else { return };
            ui.post(move || {
                with(|b| {
                    {
                        let mut shared = b.navigator.shared.borrow_mut();
                        for (path, size) in read {
                            if let Some(slot) = shared.sizes.get_mut(&path) {
                                *slot = size;
                            }
                        }
                    }
                    if b.navigator.shared.borrow().open {
                        b.publish_navigator();
                    }
                });
            });
        });
    }
}

fn set_folder_names(bridge: &CommandsBridge, folder: &Path) {
    let Some(window) = bridge.window.upgrade() else { return };
    let global = window.global::<FolderNavigator<'_>>();
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = folder
        .parent()
        .and_then(Path::file_name)
        .map_or_else(|| "/".to_owned(), |n| format!("…/{}/", n.to_string_lossy()));
    global.set_folder(name.into());
    global.set_parent_folder(parent.into());
}

fn with(run: impl FnOnce(&CommandsBridge)) {
    if let Some(bridge) = BRIDGE.with(|b| b.borrow().upgrade()) {
        run(&bridge);
    }
}
