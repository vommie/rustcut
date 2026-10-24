//! The notice system (M6.5, user decisions Q-N8 and Q-N10): every message of RustCut is a typed [`Notice`] of a
//! [`Kind`]; the router decides where it appears (toast, banner, status line, nothing, plus the desktop) from the kind's
//! defaults and the user's `[notices]` settings, critical kinds keep their place. Any bridge posts with [`post`] and
//! ends a state with [`clear`] on the UI thread; nothing is lost before the center is installed (it is kept and shown
//! then). `ui/notice_*.slint` draws the toasts, the banner over the video and the bell.

mod banners;
mod center;
mod history;
mod kind;
mod notice;
mod route;
mod toasts;
mod view;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub use center::{Hooks, NoticeCenter, Parts};
pub use history::Entry;
pub use kind::{Group, Kind, KindInfo, Level};
pub use notice::Notice;
pub use route::{Route, can_mute, desktop_of, place_of, route, set_desktop, set_place};

use crate::MainWindow;

thread_local! {
    static CENTER: RefCell<Weak<NoticeCenter>> = const { RefCell::new(Weak::new()) };
    /// Messages posted before the center exists (the start), shown once it does.
    static PENDING: RefCell<Vec<Notice>> = const { RefCell::new(Vec::new()) };
}

/// Makes [`post`] work and shows what came before; the app owns the only strong reference.
pub fn install(window: &MainWindow, parts: Parts<'_>) -> Rc<NoticeCenter> {
    let center = NoticeCenter::new(window, parts);
    CENTER.with(|c| *c.borrow_mut() = Rc::downgrade(&center));
    for notice in PENDING.with(|p| std::mem::take(&mut *p.borrow_mut())) {
        center.post(notice);
    }
    center
}

fn center() -> Option<Rc<NoticeCenter>> {
    CENTER.with(|c| c.borrow().upgrade())
}

/// Shows a message where its kind goes (UI thread).
pub fn post(notice: Notice) {
    match center() {
        Some(center) => center.post(notice),
        None => PENDING.with(|p| p.borrow_mut().push(notice)),
    }
}

/// Ends the state with `key` (a live notice, or an event banner by its kind's id).
pub fn clear(key: &str) {
    if let Some(center) = center() {
        center.clear(key);
    } else {
        PENDING.with(|p| p.borrow_mut().retain(|n| n.key.as_deref() != Some(key)));
    }
}

/// Ends the banner or live notice of `kind` that has no own key.
pub fn clear_kind(kind: Kind) {
    clear(kind.id());
}

/// The number of the history's newest entry (0 = none).
pub fn history_seq() -> u64 {
    center().map_or(0, |center| center.history().seq())
}

/// The history's number of its last entry and a copy of the entries newer than `after` (the Logs panel).
pub fn history_since(after: u64) -> (u64, Vec<Entry>) {
    center().map_or((0, Vec::new()), |center| {
        let history = center.history();
        (
            history.seq(),
            history.entries().filter(|e| e.seq > after).cloned().collect(),
        )
    })
}

/// "Show log": the Logs panel with RustCut's own log of this run, where a message's details are.
pub fn show_app_log() {
    if let Some(center) = center() {
        center.show_log();
    }
}

/// A desktop notification's button (from the dispatcher).
fn run_desktop(id: u64) {
    if let Some(center) = center() {
        center.run_desktop(id);
    }
}
