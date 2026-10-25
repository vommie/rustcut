//! The Slint forms of the notice models (`ui/notice_types.slint`).

use std::time::Instant;

use rustcut_core::settings::{BannerAnimation, ToastAnimation, ToastPosition, ToastStacking};

use super::banners::Banner;
use super::history::Entry;
use super::notice::Label;
use super::route::can_mute;
use super::toasts::{Shown, Summary, Timer, Toast, ToastStack};
use crate::{BannerView, HistoryView, ToastView};

fn action(actions: &[Label], index: usize) -> slint::SharedString {
    actions.get(index).map(|a| a.text.as_str()).unwrap_or_default().into()
}

fn jumps(actions: &[Label], index: usize) -> bool {
    actions.get(index).is_some_and(|a| a.jump)
}

pub fn toast(t: &Toast, shown: Shown, now: Instant) -> ToastView {
    let remaining = ToastStack::remaining(t, now);
    let (running, left_ms) = match t.timer {
        Timer::Running { deadline, .. } => (
            true,
            i32::try_from(deadline.saturating_duration_since(now).as_millis()).unwrap_or(i32::MAX),
        ),
        Timer::Paused { .. } | Timer::Stays => (false, 0),
    };
    ToastView {
        id: i32::try_from(t.id).unwrap_or(i32::MAX),
        level: t.level.index(),
        title: t.title.as_str().into(),
        detail: t.detail.as_str().into(),
        time: t.time.as_str().into(),
        count: i32::try_from(t.count).unwrap_or(i32::MAX),
        progress: t.progress.unwrap_or(-1.0),
        remaining: remaining.unwrap_or(-1.0),
        running,
        left_ms,
        timer: i32::try_from(t.restarts % 1_000_000).unwrap_or(0),
        action_1: action(&t.actions, 0),
        action_2: action(&t.actions, 1),
        action_3: action(&t.actions, 2),
        jump_1: jumps(&t.actions, 0),
        jump_2: jumps(&t.actions, 1),
        jump_3: jumps(&t.actions, 2),
        clickable: t.clickable,
        mutable: can_mute(t.kind) && t.level.is_quiet(),
        leaving: t.leaving.is_some(),
        depth: i32::try_from(shown.depth).unwrap_or(0),
    }
}

pub fn banner(b: &Banner) -> BannerView {
    BannerView {
        level: b.level.index(),
        title: b.title.as_str().into(),
        detail: b.detail.as_str().into(),
        action_1: action(&b.actions, 0),
        action_2: action(&b.actions, 1),
        action_3: action(&b.actions, 2),
        jump_1: jumps(&b.actions, 0),
        jump_2: jumps(&b.actions, 1),
        jump_3: jumps(&b.actions, 2),
        clickable: b.clickable,
        progress: b.progress.unwrap_or(-1.0),
        time: b.time.as_str().into(),
    }
}

pub fn history(e: &Entry) -> HistoryView {
    HistoryView {
        time: e.time.as_str().into(),
        level: e.level.index(),
        title: e.title.as_str().into(),
        detail: e.detail.as_str().into(),
    }
}

/// "5 messages: 1 error, 2 warnings".
pub fn summary(s: &Summary) -> String {
    let mut parts = Vec::new();
    match s.errors {
        0 => {}
        1 => parts.push("1 error".to_owned()),
        n => parts.push(format!("{n} errors")),
    }
    match s.warnings {
        0 => {}
        1 => parts.push("1 warning".to_owned()),
        n => parts.push(format!("{n} warnings")),
    }
    if parts.is_empty() {
        format!("{} messages", s.total)
    } else {
        format!("{} messages: {}", s.total, parts.join(", "))
    }
}

pub const fn position(p: ToastPosition) -> i32 {
    match p {
        ToastPosition::VideoTopRight => 0,
        ToastPosition::VideoTopCenter => 1,
        ToastPosition::VideoBottomRight => 2,
        ToastPosition::VideoBottomCenter => 3,
        ToastPosition::WindowTopRight => 4,
        ToastPosition::WindowBottomRight => 5,
    }
}

pub const fn stacking(s: ToastStacking) -> i32 {
    match s {
        ToastStacking::Some => 0,
        ToastStacking::All => 1,
        ToastStacking::Pager => 2,
        ToastStacking::Deck => 3,
        ToastStacking::Summary => 4,
    }
}

pub const fn animation(a: ToastAnimation) -> i32 {
    match a {
        ToastAnimation::Slide => 0,
        ToastAnimation::Fade => 1,
        ToastAnimation::Drop => 2,
        ToastAnimation::None => 3,
    }
}

pub const fn banner_animation(a: BannerAnimation) -> i32 {
    match a {
        BannerAnimation::Curtain => 0,
        BannerAnimation::Fade => 1,
        BannerAnimation::None => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::super::kind::Level;
    use super::*;

    #[test]
    fn the_summary_names_errors_and_warnings() {
        let s = |total, errors, warnings| Summary {
            total,
            errors,
            warnings,
            level: Level::Info,
        };
        assert_eq!(summary(&s(5, 1, 2)), "5 messages: 1 error, 2 warnings");
        assert_eq!(summary(&s(3, 2, 0)), "3 messages: 2 errors");
        assert_eq!(summary(&s(2, 0, 0)), "2 messages");
    }
}
