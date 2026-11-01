//! The control bar's parts that are more than playback (P12 block 5): the normalization button with its menu and keys, the
//! switch for the resting pointer, the current time's menu (Open in mpv, Copy time, Go to time), what fits at the
//! bar's width and the "..." menu. Choices are settings: everything goes through the [`SettingsWriter`], and
//! `PlayerBridge::apply_settings` gives the player and the `Playback` global the result, so the button and the
//! Settings window always show one value.

pub mod layout;
mod menus;

use std::io;
use std::rc::{Rc, Weak};
use std::time::Duration;

use rustcut_core::external::mpv::MpvStart;
use rustcut_core::settings::Normalization;
use rustcut_core::time::Timestamp;
use slint::platform::WindowEvent;
use slint::{ComponentHandle as _, LogicalPosition};

use crate::app::SettingsWriter;
use crate::ui::dispatcher::UiDispatcher;
use crate::ui::editor::Editor;
use crate::ui::jump::JumpTarget;
use crate::ui::menu::ContextMenuBridge;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::player::{FlashKind, PlayerBridge};
use crate::ui::settings::page::Page;
use crate::util::clipboard;
use crate::{BarFit, MainWindow, Playback, PlayerPopup, Sections, Shortcuts};

/// What the control bar works with.
pub struct Parts<'a> {
    pub writer: SettingsWriter,
    pub player: &'a Rc<PlayerBridge>,
    pub editor: &'a Rc<Editor>,
    pub menu: &'a Rc<ContextMenuBridge>,
    pub ui: &'a UiDispatcher,
}

/// The bridge; the app keeps it for as long as the window lives.
pub struct ControlBar {
    window: slint::Weak<MainWindow>,
    writer: SettingsWriter,
    player: Weak<PlayerBridge>,
    editor: Weak<Editor>,
    menu: Rc<ContextMenuBridge>,
    ui: UiDispatcher,
}

impl ControlBar {
    pub fn install(window: &MainWindow, parts: Parts<'_>) -> Rc<Self> {
        let bar = Rc::new(Self {
            window: window.as_weak(),
            writer: parts.writer.clone(),
            player: Rc::downgrade(parts.player),
            editor: Rc::downgrade(parts.editor),
            menu: Rc::clone(parts.menu),
            ui: parts.ui.clone(),
        });
        bar.connect(window);
        // A kind chosen in the Settings window also counts as "used last".
        let writer = parts.writer.clone();
        parts.writer.subscribe(move |before, after| {
            let left = before.player.normalization;
            if left != after.player.normalization
                && left != Normalization::None
                && after.player.last_normalization != left
            {
                writer.update(|s| s.player.last_normalization = left);
            }
        });
        bar
    }

    fn connect(self: &Rc<Self>, window: &MainWindow) {
        let playback = window.global::<Playback<'_>>();
        playback.on_bar_fit(|width, time, slash| {
            let fit = layout::fit(width, time, slash);
            BarFit {
                percent: fit.percent,
                slider: fit.slider,
                total: fit.total,
                frames: fit.frames,
                sections: fit.sections,
                audio: fit.audio,
                more: fit.more,
                spare: fit.spare,
            }
        });
        let this = Rc::downgrade(self);
        playback.on_normalization_menu(move |x, y, width, height| {
            if let Some(bar) = this.upgrade() {
                bar.show_normalization([x, y, width, height]);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_normalization_toggle(move || {
            if let Some(bar) = this.upgrade() {
                bar.writer.update(|s| s.player.toggle_normalization());
            }
        });
        let this = Rc::downgrade(self);
        playback.on_normalization_key(move |next| {
            if let Some(bar) = this.upgrade() {
                bar.normalization_key(next);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_toggle_hide_pointer(move || {
            if let Some(bar) = this.upgrade() {
                bar.writer.update(|s| s.player.hide_pointer = !s.player.hide_pointer);
            }
        });
        let this = window.as_weak();
        playback.on_pointer_rest_changed(move |x, y| repeat_pointer_event(this.clone(), x, y));
        let this = Rc::downgrade(self);
        playback.on_time_menu(move |x, y| {
            if let Some(bar) = this.upgrade() {
                bar.show_time_menu(x, y);
            }
        });
        let this = Rc::downgrade(self);
        playback.on_go_to_time(move |text| this.upgrade().is_some_and(|bar| bar.go_to_time(&text)));
        let this = Rc::downgrade(self);
        playback.on_more_menu(move |x, y, width, height, fit| {
            if let Some(bar) = this.upgrade() {
                bar.show_more([x, y, width, height], &fit);
            }
        });
    }

    fn current_kind(&self) -> Normalization {
        self.writer.current().player.normalization
    }

    fn choose(&self, kind: Normalization) {
        self.writer.update(|s| s.player.choose_normalization(kind));
    }

    fn show_normalization(self: &Rc<Self>, anchor: [f32; 4]) {
        let this = Rc::downgrade(self);
        self.menu
            .show_at(&menus::normalization(self.current_kind()), anchor, move |id| {
                if let (Some(bar), Some(kind)) = (this.upgrade(), menus::picked_kind(id)) {
                    bar.choose(kind);
                }
            });
    }

    /// The two keys: on/off (`next` = false) or the next kind; the new kind shows over the video.
    fn normalization_key(&self, next: bool) {
        self.writer.update(|s| {
            if next {
                s.player.next_normalization();
            } else {
                s.player.toggle_normalization();
            }
        });
        let settings = self.writer.current().player;
        if let Some(window) = self.window.upgrade()
            && settings.flash_normalization
        {
            let playback = window.global::<Playback<'_>>();
            playback.set_flash_text(settings.normalization.title().into());
            crate::ui::player::flash(&playback, FlashKind::Normalization);
        }
    }

    fn show_time_menu(self: &Rc<Self>, x: f32, y: f32) {
        let Some(player) = self.player.upgrade() else { return };
        let position = player.position();
        let in_section = self.editor.upgrade().is_some_and(|e| e.section_at(position).is_some());
        let items = menus::time(player.is_loaded(), in_section);
        let this = Rc::downgrade(self);
        self.menu.show(&items, x, y, move |id| {
            if let Some(bar) = this.upgrade() {
                bar.time_action(id);
            }
        });
    }

    fn time_action(&self, id: &str) {
        match id {
            menus::OPEN_MPV => self.open_in_mpv(false),
            menus::OPEN_SECTION => self.open_in_mpv(true),
            menus::COPY_TIME => self.copy_time(),
            menus::GO_TO_TIME => {
                if let Some(window) = self.window.upgrade() {
                    window.global::<PlayerPopup<'_>>().set_time_open(true);
                }
            }
            other => tracing::warn!(id = other, "[Player] Unknown entry of the time menu"),
        }
    }

    /// Opens the video in mpv at the playhead or as an A-B loop of the section around it. RustCut pauses, so the two do
    /// not play over each other (user decision Q-P12d).
    fn open_in_mpv(&self, section: bool) {
        let Some(player) = self.player.upgrade() else { return };
        let Some((file, position, paused)) = player.resume_point() else {
            return;
        };
        let range = section
            .then(|| self.editor.upgrade().and_then(|e| e.section_at(position)))
            .flatten();
        if section && range.is_none() {
            return;
        }
        if !paused {
            player.set_paused(true);
        }
        let start = MpvStart {
            program: self.writer.current().player.mpv,
            file,
            start: range.map_or(position, |r| r.0),
            loop_end: range.map(|r| r.1),
        };
        let ui = self.ui.clone();
        start.launch(move |e| ui.post(move || mpv_failed(&e)));
    }

    fn copy_time(&self) {
        let Some(player) = self.player.upgrade() else { return };
        let text = player.position().to_string();
        if clipboard::copy(text.clone()) {
            notices::post(Notice::new(Kind::TimeCopied, format!("{text} copied")));
        } else {
            notices::post(Notice::new(Kind::ClipboardFailed, "The clipboard cannot be used"));
        }
    }

    /// "Go to time": a readable time jumps there, past the end to the end.
    fn go_to_time(&self, text: &str) -> bool {
        let (Some(player), Ok(time)) = (self.player.upgrade(), Timestamp::parse(text)) else {
            return false;
        };
        let target = player.duration().map_or(time, |d| time.min(d));
        player.seek_to(target, true);
        true
    }

    fn show_more(self: &Rc<Self>, anchor: [f32; 4], fit: &BarFit) {
        let Some(window) = self.window.upgrade() else { return };
        let playback = window.global::<Playback<'_>>();
        let sections = window.global::<Sections<'_>>();
        let (back, forward) = (playback.get_key_frame_back(), playback.get_key_frame_forward());
        let (start, end, add) = (
            sections.get_key_set_start(),
            sections.get_key_set_end(),
            sections.get_key_add(),
        );
        let settings = self.writer.current().player;
        let items = menus::more(
            layout::Fit {
                percent: fit.percent,
                slider: fit.slider,
                total: fit.total,
                frames: fit.frames,
                sections: fit.sections,
                audio: fit.audio,
                more: fit.more,
                spare: fit.spare,
            },
            &menus::More {
                has_video: playback.get_has_video(),
                has_media: sections.get_has_media(),
                can_add: sections.get_can_add(),
                normalization: settings.normalization,
                hide_pointer: settings.hide_pointer,
                keys: [&back, &forward, &start, &end, &add],
            },
        );
        let this = Rc::downgrade(self);
        self.menu.show_at(&items, anchor, move |id| {
            let Some(bar) = this.upgrade() else { return };
            if let Some(kind) = menus::picked_kind(id) {
                bar.choose(kind);
            } else if id == menus::TOGGLE_POINTER {
                bar.writer.update(|s| s.player.hide_pointer = !s.player.hide_pointer);
            } else if let Some(window) = bar.window.upgrade() {
                window.global::<Shortcuts<'_>>().invoke_run(id.into());
            }
        });
    }
}

/// The kinds' symbols in the order of the menu and the Settings choice.
pub fn normalization_icons() -> Vec<crate::icons::Icon> {
    std::iter::once(Normalization::None)
        .chain(Normalization::ACTIVE)
        .map(menus::kind_icon)
        .collect()
}

/// The text of "Hide the resting pointer" for the tooltip: `3 s`, `1.5 s`.
pub fn seconds_text(ms: u32) -> String {
    let tenths = ms / 100;
    if tenths.is_multiple_of(10) {
        format!("{} s", tenths / 10)
    } else {
        format!("{}.{} s", tenths / 10, tenths % 10)
    }
}

/// Slint picks the cursor only at a pointer event, and a resting pointer sends none: the last event is repeated, after the
/// current one is done. The video's area sees the same place and does not count it as movement.
fn repeat_pointer_event(window: slint::Weak<MainWindow>, x: f32, y: f32) {
    slint::Timer::single_shot(Duration::ZERO, move || {
        if let Some(window) = window.upgrade()
            && let Err(e) = window.window().dispatch_event_with_result(WindowEvent::PointerMoved {
                position: LogicalPosition::new(x, y),
            })
        {
            tracing::debug!(error = %e, "[Player] The pointer event was not repeated");
        }
    });
}

fn mpv_failed(error: &io::Error) {
    tracing::warn!(error = %error, "[Player] mpv did not start");
    let settings = JumpTarget::Settings {
        page: Page::Player,
        key: "player.mpv",
    };
    let notice = if error.kind() == io::ErrorKind::NotFound {
        Notice::new(Kind::MpvFailed, "mpv cannot be found").detail("Install it with: sudo pacman -S mpv")
    } else {
        Notice::new(Kind::MpvFailed, "mpv did not start").detail(error.to_string())
    };
    notices::post(notice.jump_action("Settings", settings));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rest_time_reads_in_seconds() {
        assert_eq!(seconds_text(3_000), "3 s");
        assert_eq!(seconds_text(1_500), "1.5 s");
        assert_eq!(seconds_text(60_000), "60 s");
    }
}
