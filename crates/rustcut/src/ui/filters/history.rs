//! The crop history's panel (P12 4.3, Q-P12c): the list that hangs from its button on the Crop row, the preview of an
//! entry on the video, and what makes a crop count as used: auto-crop's result, an edit on the video that was kept, a
//! saved job. The entries live in `rustcut_core::edit::CropHistory`, the rows in `history_view`; the list is saved at
//! every change (`set_saver`), so it survives a crash.

use std::rc::{Rc, Weak};
use std::time::Duration;

use chrono::Local;
use rustcut_core::edit::{Crop, CropHistory, CropMemory, FilterKind};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use super::FiltersBridge;
use super::history_view::{self, Context, ENTRY_HEIGHT, HEADING_HEIGHT, Line, PAGE, Row};
use crate::ui::notices::{Kind, Notice};
use crate::util::model;
use crate::{CropHistoryRow, CropHistoryUi, MainWindow};

/// How long the pointer rests on an entry before the video shows it.
const PREVIEW_DELAY: Duration = Duration::from_millis(180);
/// How long the preview stays after the pointer left an entry (moving to the next one does not flicker).
const PREVIEW_END_DELAY: Duration = Duration::from_millis(120);
/// A key opens the list after its card showed.
const OPEN_DELAY: Duration = Duration::from_millis(120);

/// Keeps the list where it survives a crash.
type Saver = Box<dyn Fn(&[CropMemory])>;

/// The list and the panel showing it.
pub struct HistoryPanel {
    list: CropHistory,
    open: bool,
    page: usize,
    highlight: Option<usize>,
    /// The rows as shown.
    shown: Vec<Row>,
    /// When this start began (entries used since carry the dot).
    session_start: i64,
    saver: Option<Saver>,
    rows: Rc<VecModel<CropHistoryRow>>,
    preview_timer: slint::Timer,
}

impl HistoryPanel {
    pub fn new(stored: Vec<CropMemory>, length: u8) -> Self {
        Self {
            list: CropHistory::new(stored, usize::from(length)),
            open: false,
            page: 0,
            highlight: None,
            shown: Vec::new(),
            session_start: Local::now().timestamp(),
            saver: None,
            rows: Rc::new(VecModel::default()),
            preview_timer: slint::Timer::default(),
        }
    }

    pub fn entries(&self) -> &[CropMemory] {
        self.list.entries()
    }

    fn line(&self, row: usize) -> Option<&Line> {
        match self.shown.get(row)? {
            Row::Entry(line) => Some(line),
            Row::Heading(_) => None,
        }
    }

    fn save(&self) {
        if let Some(saver) = &self.saver {
            saver(self.list.entries());
        }
    }
}

pub(super) fn connect(bridge: &Rc<FiltersBridge>, window: &MainWindow) {
    let ui = window.global::<CropHistoryUi<'_>>();
    ui.set_rows(ModelRc::from(Rc::clone(&bridge.history.borrow().rows)));
    let with = |run: fn(&FiltersBridge, i32)| {
        let this = Rc::downgrade(bridge);
        move |value: i32| {
            if let Some(bridge) = this.upgrade() {
                run(&bridge, value);
            }
        }
    };
    let toggle = with(|b, _| b.toggle_history());
    ui.on_toggle(move || toggle(0));
    let closed = with(|b, _| b.close_history());
    ui.on_closed(move || closed(0));
    let quick = with(|b, _| b.quick_history());
    ui.on_quick(move || quick(0));
    let clear = with(|b, _| {
        b.history.borrow_mut().list.clear();
        b.history_changed();
    });
    ui.on_clear(move || clear(0));
    ui.on_pick(with(FiltersBridge::pick_row));
    ui.on_hover(with(FiltersBridge::hover_row));
    ui.on_page(with(FiltersBridge::turn_page));
    ui.on_pin(with(|b, row| b.change_entry(row, CropHistory::toggle_pin)));
    ui.on_remove(with(|b, row| b.change_entry(row, CropHistory::remove)));
    let this = Rc::downgrade(bridge);
    ui.on_key(move |text| this.upgrade().is_some_and(|bridge| bridge.history_key(text.as_str())));
}

impl FiltersBridge {
    /// Where the list is saved at every change (the app connects it to the UI state writer).
    pub fn set_history_saver(&self, saver: impl Fn(&[CropMemory]) + 'static) {
        self.history.borrow_mut().saver = Some(Box::new(saver));
    }

    /// The crop in use as an entry; `None` while it cuts nothing or the picture is not known.
    fn current_memory(&self) -> Option<CropMemory> {
        let st = self.state.borrow();
        let crop = st.crop();
        let frame = st.crop_frame()?;
        (!crop.is_identity()).then(|| CropMemory {
            frame,
            crop,
            pinned: false,
            used: Local::now().timestamp(),
            source: self
                .file
                .borrow()
                .as_deref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        })
    }

    /// The crop in use counts as used (auto-crop's result, a kept edit, a saved job).
    pub(super) fn remember_crop(&self) {
        let Some(memory) = self.current_memory() else { return };
        let length = usize::from(self.state.borrow().defaults().sanitized().crop_history_length);
        if self.history.borrow_mut().list.remember(memory, length) {
            self.history_changed();
        }
    }

    /// A job was saved: its crop counts when the filter is on.
    pub fn job_saved(&self) {
        if self.state.borrow().chain().is_enabled(FilterKind::Crop) {
            self.remember_crop();
        }
    }

    /// The length changed in the Settings window: the list follows at once.
    pub(super) fn history_length_changed(&self) {
        let length = usize::from(self.state.borrow().defaults().sanitized().crop_history_length);
        let before = self.history.borrow().entries().len();
        self.history.borrow_mut().list.trim(length);
        if self.history.borrow().entries().len() != before {
            self.history.borrow().save();
        }
        self.publish_history();
    }

    fn history_changed(&self) {
        self.history.borrow().save();
        self.publish_history();
    }

    fn context(&self) -> Context {
        let st = self.state.borrow();
        Context {
            frame: st.crop_frame(),
            current: st.chain().is_enabled(FilterKind::Crop).then(|| st.crop()),
            orientation: st.chain().orientation_after(FilterKind::Crop),
            sar: st.crop_sar(),
            session_start: self.history.borrow().session_start,
            today: Local::now().date_naive(),
        }
    }

    /// The list and its footer as they are now (only while it is open).
    pub(super) fn publish_history(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let ui = window.global::<CropHistoryUi<'_>>();
        let context = self.context();
        let length = self.state.borrow().defaults().sanitized().crop_history_length;
        let mut panel = self.history.borrow_mut();
        ui.set_has_entry(!panel.list.is_empty());
        if !panel.open {
            return;
        }
        let lines = history_view::lines(panel.list.entries(), &context);
        let paged = history_view::is_paged(lines.len(), length);
        let pages = history_view::page_count(lines.len()).max(1);
        panel.page = panel.page.min(pages - 1);
        let rows = history_view::rows(lines, paged.then_some(panel.page));
        panel.highlight = panel
            .highlight
            .filter(|row| matches!(rows.get(*row), Some(Row::Entry(_))));
        model::sync(&panel.rows, rows.iter().map(view).collect());
        let free = panel.list.entries().iter().filter(|e| !e.pinned).count();
        let height = if paged {
            precise(PAGE) * ENTRY_HEIGHT + 2.0 * HEADING_HEIGHT
        } else {
            rows.iter().map(row_height).sum()
        };
        ui.set_rows_height(height);
        ui.set_highlight(panel.highlight.map_or(-1, super::index));
        ui.set_count(format!("{free} of {length}").into());
        ui.set_paged(paged);
        ui.set_page_text(format!("{}/{pages}", panel.page + 1).into());
        ui.set_can_previous(panel.page > 0);
        ui.set_can_next(panel.page + 1 < pages);
        ui.set_can_clear(free > 0);
        panel.shown = rows;
    }

    fn toggle_history(&self) {
        if self.history.borrow().open {
            self.close_history();
        } else {
            let mut panel = self.history.borrow_mut();
            panel.open = true;
            panel.page = 0;
            panel.highlight = None;
            drop(panel);
            self.publish_history();
            if let Some(window) = self.window.upgrade() {
                window.global::<CropHistoryUi<'_>>().set_open(true);
            }
        }
    }

    pub(super) fn close_history(&self) {
        self.history.borrow_mut().open = false;
        self.end_preview();
        if let Some(window) = self.window.upgrade() {
            window.global::<CropHistoryUi<'_>>().set_open(false);
        }
    }

    /// The history key: the card shows, then the list opens from its button.
    pub fn open_history(&self) {
        if self.file.borrow().is_none() || self.history.borrow().open {
            return;
        }
        crate::ui::jump::go(crate::ui::jump::JumpTarget::FilterCard);
        let window = self.window.clone();
        slint::Timer::single_shot(OPEN_DELAY, move || {
            if let Some(window) = window.upgrade() {
                let ui = window.global::<CropHistoryUi<'_>>();
                ui.set_request(ui.get_request().wrapping_add(1));
            }
        });
    }

    fn line_of(&self, row: i32) -> Option<Line> {
        let row = usize::try_from(row).ok()?;
        self.history.borrow().line(row).cloned()
    }

    fn change_entry(&self, row: i32, change: fn(&mut CropHistory, &CropMemory)) {
        let Some(line) = self.line_of(row) else { return };
        change(&mut self.history.borrow_mut().list, &line.memory);
        self.end_preview();
        self.history_changed();
    }

    fn pick_row(&self, row: i32) {
        let Some(line) = self.line_of(row) else { return };
        let Some(crop) = line.crop else { return };
        self.apply_entry(&line.memory, crop);
        self.close_history();
    }

    /// Sets a crop of the list: it goes to the top, and the crop it replaces into the list, so the way back stays.
    fn apply_entry(&self, memory: &CropMemory, crop: Crop) {
        self.user_took_over();
        self.end_preview();
        let replaced = self.current_memory();
        let length = usize::from(self.state.borrow().defaults().sanitized().crop_history_length);
        {
            let mut panel = self.history.borrow_mut();
            if let Some(replaced) = replaced {
                panel.list.remember(replaced, length);
            }
            panel.list.remember(
                CropMemory {
                    used: Local::now().timestamp(),
                    ..memory.clone()
                },
                length,
            );
        }
        self.update(|s| s.apply_crop(crop));
        self.history_changed();
    }

    /// The button's right click: the newest crop that fits and is not set already.
    fn quick_history(&self) {
        let context = self.context();
        let entries = self.history.borrow().entries().to_vec();
        let found = entries.iter().find_map(|memory| {
            let frame = context.frame?;
            let crop = match rustcut_core::edit::fit(memory, frame) {
                rustcut_core::edit::Fit::Same => memory.crop,
                rustcut_core::edit::Fit::Scaled(crop) => crop,
                rustcut_core::edit::Fit::Other => return None,
            };
            (Some(crop) != context.current).then_some((memory, crop))
        });
        let Some((memory, crop)) = found else {
            Self::tell(Notice::new(
                Kind::CropFromHistory,
                "No other crop in the history fits this video",
            ));
            return;
        };
        self.apply_entry(memory, crop);
        let size = self.state.borrow().shown_kept();
        Self::tell(Notice::new(
            Kind::CropFromHistory,
            format!("Crop {} set from the history", size.unwrap_or_default()),
        ));
    }

    fn turn_page(&self, step: i32) {
        {
            let mut panel = self.history.borrow_mut();
            panel.page = panel.page.saturating_add_signed(isize::try_from(step).unwrap_or(0));
            panel.highlight = None;
        }
        self.end_preview();
        self.publish_history();
    }

    fn history_key(&self, text: &str) -> bool {
        use slint::platform::Key::{DownArrow, LeftArrow, PageDown, PageUp, Return, RightArrow, UpArrow};
        let key = |k: slint::platform::Key| text == slint::SharedString::from(k).as_str();
        if key(Return) {
            let row = self.history.borrow().highlight;
            if let Some(row) = row {
                self.pick_row(super::index(row));
            }
            return true;
        }
        if key(LeftArrow) || key(PageUp) {
            self.turn_page(-1);
            return true;
        }
        if key(RightArrow) || key(PageDown) {
            self.turn_page(1);
            return true;
        }
        let step = if key(DownArrow) {
            1
        } else if key(UpArrow) {
            -1
        } else {
            return false;
        };
        let row = {
            let mut panel = self.history.borrow_mut();
            let row = history_view::next_usable(&panel.shown, panel.highlight, step);
            panel.highlight = row;
            row
        };
        if let Some(window) = self.window.upgrade() {
            window
                .global::<CropHistoryUi<'_>>()
                .set_highlight(row.map_or(-1, super::index));
        }
        self.hover_row(row.map_or(-1, super::index));
        true
    }

    /// The pointer or the keys rest on `row` (-1: on none): after a moment the video shows that crop.
    fn hover_row(&self, row: i32) {
        let crop = self.line_of(row).and_then(|line| line.crop);
        let me = Weak::clone(&self.me);
        let delay = if crop.is_some() {
            PREVIEW_DELAY
        } else {
            PREVIEW_END_DELAY
        };
        self.history
            .borrow()
            .preview_timer
            .start(slint::TimerMode::SingleShot, delay, move || {
                if let Some(bridge) = me.upgrade() {
                    bridge.show_preview(crop);
                }
            });
    }

    fn show_preview(&self, crop: Option<Crop>) {
        if self.suspended.get() || self.state.borrow().history_preview == crop {
            return;
        }
        self.state.borrow_mut().history_preview = crop;
        self.publish_overlay();
        self.send_preview();
    }

    fn end_preview(&self) {
        self.history.borrow().preview_timer.stop();
        self.show_preview(None);
    }
}

fn row_height(row: &Row) -> f32 {
    match row {
        Row::Heading(_) => HEADING_HEIGHT,
        Row::Entry(_) => ENTRY_HEIGHT,
    }
}

/// A count as a length factor (ten or two).
#[expect(clippy::cast_precision_loss, reason = "the page size is ten")]
const fn precise(value: usize) -> f32 {
    value as f32
}

fn view(row: &Row) -> CropHistoryRow {
    match row {
        Row::Heading(text) => CropHistoryRow {
            heading: (*text).into(),
            ..CropHistoryRow::default()
        },
        Row::Entry(line) => CropHistoryRow {
            heading: "".into(),
            title: line.title.as_str().into(),
            detail: line.detail.as_str().into(),
            tag: line.tag.into(),
            tip: line.tip.as_str().into(),
            session: line.session,
            current: line.current,
            pinned: line.memory.pinned,
            usable: line.crop.is_some(),
            frame_width: line.mini.width,
            frame_height: line.mini.height,
            kept_x: line.mini.kept.0,
            kept_y: line.mini.kept.1,
            kept_width: line.mini.kept.2,
            kept_height: line.mini.kept.3,
        },
    }
}
