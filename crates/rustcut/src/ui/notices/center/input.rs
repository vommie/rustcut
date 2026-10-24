//! What the user does on the toasts, the banner and the bell: clicks, buttons, the toasts' context menu and "Don't show
//! again", and the wiring of the `Notices` callbacks.

use std::rc::{Rc, Weak};
use std::time::Instant;

use slint::ComponentHandle as _;

use super::{NoticeCenter, SUMMARY, run_click};
use crate::Notices;
use crate::icons::Icon;
use crate::ui::menu::{MenuAction, MenuItem};
use crate::ui::notices::route::can_mute;

impl NoticeCenter {
    fn close_toast(&self, id: u64) {
        let now = Instant::now();
        if id == SUMMARY {
            self.toasts.borrow_mut().close_all(now);
            self.publish_toasts();
        } else if self.toasts.borrow_mut().close(id, now) {
            self.publish_toasts();
        }
    }

    fn toast_clicked(&self, id: u64) {
        if id == SUMMARY {
            self.toasts.borrow_mut().set_expanded(true);
            self.publish_toasts();
            return;
        }
        let click = self.toast_runs.borrow().get(&id).and_then(|r| r.click.clone());
        self.close_toast(id);
        run_click(click);
    }

    fn toast_action(&self, id: u64, index: usize) {
        let action = self
            .toast_runs
            .borrow()
            .get(&id)
            .and_then(|r| r.actions.get(index).cloned());
        let Some(action) = action else { return };
        if action.closes {
            self.close_toast(id);
        }
        (action.run)();
    }

    fn toast_menu(&self, id: u64, x: f32, y: f32) {
        let Some(toast) = self.toasts.borrow().get(id).cloned() else {
            return;
        };
        let Some(menu) = self.menu.upgrade() else { return };
        let mut items = vec![MenuItem::Action(MenuAction::new(
            "copy",
            "Copy the text",
            Some(Icon::Copy),
        ))];
        if can_mute(toast.kind) && toast.level.is_quiet() {
            items.push(MenuItem::Action(MenuAction::new(
                "mute",
                "Don't show this kind again",
                Some(Icon::NoticeMute),
            )));
        }
        items.push(MenuItem::Action(MenuAction::new(
            "close_all",
            "Close all messages",
            Some(Icon::Close),
        )));
        let me = Weak::clone(&self.me);
        let text = if toast.detail.is_empty() {
            toast.title
        } else {
            format!("{}: {}", toast.title.trim_end_matches('.'), toast.detail)
        };
        menu.show(&items, x, y, move |pick| {
            let Some(center) = me.upgrade() else { return };
            match pick {
                "copy" => {
                    crate::util::clipboard::copy(text.clone());
                }
                "mute" => center.mute(id),
                "close_all" => {
                    center.toasts.borrow_mut().close_all(Instant::now());
                    center.publish_toasts();
                }
                _ => {}
            }
        });
    }

    /// "Don't show this kind again" (hints and confirmations only, Q-N8).
    fn mute(&self, id: u64) {
        let Some(kind) = self.toasts.borrow().get(id).map(|t| t.kind) else {
            return;
        };
        if !can_mute(kind) {
            return;
        }
        tracing::info!(kind = kind.id(), "[Notice] Muted");
        self.settings.borrow_mut().muted.insert(kind.id().to_owned());
        self.writer.update(|s| {
            s.notices.muted.insert(kind.id().to_owned());
        });
        self.close_toast(id);
    }

    fn banner_key(&self) -> Option<String> {
        self.banners.borrow().current().map(|(b, _, _)| b.key.clone())
    }

    fn banner_closed(&self) {
        if let Some(key) = self.banner_key() {
            self.banners.borrow_mut().hide(&key);
            self.publish_banner();
        }
    }

    fn banner_clicked(&self) {
        let click = self
            .banner_key()
            .and_then(|key| self.banner_runs.borrow().get(&key).and_then(|r| r.click.clone()));
        run_click(click);
    }

    fn banner_action(&self, index: usize) {
        let Some(key) = self.banner_key() else { return };
        let action = self
            .banner_runs
            .borrow()
            .get(&key)
            .and_then(|r| r.actions.get(index).cloned());
        let Some(action) = action else { return };
        if action.closes {
            self.banners.borrow_mut().hide(&key);
            self.publish_banner();
        }
        (action.run)();
    }

    fn bell_toggled(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let notices = window.global::<Notices<'_>>();
        let open = !notices.get_bell_open();
        notices.set_bell_open(open);
        if open {
            self.history.borrow_mut().mark_read();
        }
        self.publish_bell();
    }

    pub(in crate::ui::notices) fn show_log(&self) {
        if let Some(show) = &self.hooks.borrow().show_log {
            show();
        }
    }

    fn show_all(&self) {
        if let Some(window) = self.window.upgrade() {
            window.global::<Notices<'_>>().set_bell_open(false);
        }
        self.history.borrow_mut().mark_read();
        self.publish_bell();
        if let Some(show) = &self.hooks.borrow().show_all {
            show();
        }
    }
}

fn id_of(id: i32) -> u64 {
    u64::try_from(id).unwrap_or(0)
}

pub(super) fn connect(center: &Rc<NoticeCenter>, notices: &Notices<'_>) {
    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident: $ty:ty)*| $body:expr) => {{
            let weak = Rc::downgrade(center);
            notices.$setter(move |$($arg: $ty),*| {
                if let Some($c) = weak.upgrade() {
                    $body;
                }
            });
        }};
    }
    on!(on_toast_clicked, |c, id: i32| c.toast_clicked(id_of(id)));
    on!(on_toast_closed, |c, id: i32| c.close_toast(id_of(id)));
    on!(on_toast_action, |c, id: i32, index: i32| c
        .toast_action(id_of(id), usize::try_from(index).unwrap_or(0)));
    on!(on_toast_hover, |c, id: i32, on: bool| {
        c.toasts.borrow_mut().hover(id_of(id), on, Instant::now());
        c.publish_toasts();
    });
    on!(on_toast_menu, |c, id: i32, x: f32, y: f32| c.toast_menu(
        id_of(id),
        x,
        y
    ));
    on!(on_toast_mute, |c, id: i32| c.mute(id_of(id)));
    on!(on_set_expanded, |c, on: bool| {
        c.toasts.borrow_mut().set_expanded(on);
        c.publish_toasts();
    });
    on!(on_page_toasts, |c, delta: i32| {
        c.toasts.borrow_mut().page(delta);
        c.publish_toasts();
    });
    on!(on_banner_closed, |c| c.banner_closed());
    on!(on_banner_clicked, |c| c.banner_clicked());
    on!(on_banner_action, |c, index: i32| c
        .banner_action(usize::try_from(index).unwrap_or(0)));
    on!(on_page_banners, |c, delta: i32| {
        c.banners.borrow_mut().page(delta);
        c.publish_banner();
    });
    on!(on_bell_toggled, |c| c.bell_toggled());
    on!(on_show_all, |c| c.show_all());
}
