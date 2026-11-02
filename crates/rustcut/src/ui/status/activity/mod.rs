//! The status bar's activity segments (P12 4.1, Q-P12c): one per thing that runs, a throbber with its text, then its
//! result for a few seconds, then gone. It stands between the message and the decoder and never writes the message.
//! Starting and ending belong to the owner of the work; the result comes through the notice center
//! (`Notice::activity`, place `Activity`), so the user can send it elsewhere in Settings > Messages and the history
//! keeps it. After posting, the owner calls [`finish`]: a result that went elsewhere leaves nothing behind.

mod model;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use model::Board;
use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::ui::notices::Level;
use crate::util::model::sync_keyed;
use crate::{ActivityView, MainWindow, Status};

/// How often the times are checked while a result shows.
const TICK: Duration = Duration::from_millis(50);

/// What the × runs.
type Cancel = Rc<dyn Fn()>;

struct Activities {
    board: RefCell<Board>,
    cancels: RefCell<HashMap<String, Cancel>>,
    rows: Rc<VecModel<ActivityView>>,
    timer: slint::Timer,
}

thread_local! {
    static ACTIVITIES: RefCell<Option<Rc<Activities>>> = const { RefCell::new(None) };
}

fn with(run: impl FnOnce(&Rc<Activities>)) {
    if let Some(activities) = ACTIVITIES.with_borrow(Option::clone) {
        run(&activities);
    }
}

/// Connects the status line (UI thread, once).
pub fn install(window: &MainWindow) {
    let activities = Rc::new(Activities {
        board: RefCell::default(),
        cancels: RefCell::default(),
        rows: Rc::new(VecModel::default()),
        timer: slint::Timer::default(),
    });
    let status = window.global::<Status<'_>>();
    status.set_activities(ModelRc::from(Rc::clone(&activities.rows)));
    status.on_activity_hover(|id, on| {
        with(|a| {
            if let Ok(id) = u64::try_from(id) {
                a.board.borrow_mut().hover(id, on, Instant::now());
                a.publish();
            }
        });
    });
    status.on_activity_cancel(|id| {
        let cancel = ACTIVITIES.with_borrow(Option::clone).and_then(|a| {
            let board = a.board.borrow();
            let key = board
                .items()
                .iter()
                .find(|i| i64::from(id) == i64::try_from(i.id).unwrap_or(-1))?
                .key
                .clone();
            a.cancels.borrow().get(&key).cloned()
        });
        if let Some(cancel) = cancel {
            cancel();
        }
    });
    ACTIVITIES.set(Some(activities));
}

/// `key` starts, or goes on with new words. `cancel` adds the ×.
pub fn begin(key: &str, text: impl Into<String>, cancel: Option<Cancel>) {
    let text = text.into();
    with(|a| {
        a.board.borrow_mut().begin(key, text, cancel.is_some());
        match cancel {
            Some(cancel) => a.cancels.borrow_mut().insert(key.to_owned(), cancel),
            None => a.cancels.borrow_mut().remove(key),
        };
        a.publish();
    });
}

/// New words while `key` runs.
pub fn update(key: &str, text: impl Into<String>) {
    let text = text.into();
    with(|a| {
        if a.board.borrow_mut().update(key, text) {
            a.publish();
        }
    });
}

/// How far `key` is (0 to 1; `None` = no bar), while it runs.
pub fn progress(key: &str, progress: Option<f32>) {
    with(|a| {
        if a.board.borrow_mut().progress(key, progress) {
            a.publish();
        }
    });
}

/// `key` is over without a result in the segment; a result that shows stays for its time.
pub fn finish(key: &str) {
    with(|a| {
        a.cancels.borrow_mut().remove(key);
        a.board.borrow_mut().finish(key, Instant::now());
        a.publish();
    });
}

/// The result of `key` (only the notice center calls it).
pub(crate) fn result(key: &str, level: Level, text: &str) {
    with(|a| {
        a.cancels.borrow_mut().remove(key);
        a.board.borrow_mut().result(key, level, text.to_owned(), Instant::now());
        a.publish();
    });
}

impl Activities {
    fn publish(self: &Rc<Self>) {
        let rows: Vec<ActivityView> = self
            .board
            .borrow()
            .items()
            .iter()
            .map(|item| ActivityView {
                id: i32::try_from(item.id).unwrap_or(i32::MAX),
                text: item.text.as_str().into(),
                level: item.level().map_or(-1, Level::index),
                running: item.is_running(),
                cancelable: item.cancelable && item.is_running(),
                progress: item
                    .progress
                    .filter(|_| item.is_running())
                    .map_or(-1.0, |p| f32::from(p) / 1000.0),
                leaving: item.is_leaving(),
            })
            .collect();
        sync_keyed(&self.rows, rows, |row| row.id);
        if !self.board.borrow().is_timed() {
            self.timer.stop();
        } else if !self.timer.running() {
            let me = Rc::downgrade(self);
            self.timer.start(slint::TimerMode::Repeated, TICK, move || {
                if let Some(me) = me.upgrade()
                    && me.board.borrow_mut().tick(Instant::now())
                {
                    me.publish();
                }
            });
        }
    }
}
