//! One message as a bridge describes it: its kind, the words, the buttons and what a click on it does. Where it
//! appears is the router's business (`route.rs`), never the bridge's.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::bubbles::Area;
use super::kind::{Kind, Level};
use crate::ui::jump::JumpTarget;

/// A button of a toast, banner or bubble (UI thread).
#[derive(Clone)]
pub struct Action {
    /// Its words; the tooltip of an icon button.
    pub label: String,
    pub run: Rc<dyn Fn()>,
    /// The message goes away after the button ran.
    pub closes: bool,
    /// A bubble shows it as an icon button (P8b); toasts and banners show the words.
    pub icon: Option<crate::icons::Icon>,
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Action")
            .field("label", &self.label)
            .field("closes", &self.closes)
            .field("icon", &self.icon)
            .finish_non_exhaustive()
    }
}

/// What a click on the message itself does ("leads to the cause", Q-N8).
#[derive(Clone)]
pub enum Click {
    Jump(JumpTarget),
    Run(Rc<dyn Fn()>),
}

impl fmt::Debug for Click {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jump(target) => f.debug_tuple("Jump").field(target).finish(),
            Self::Run(_) => f.write_str("Run(..)"),
        }
    }
}

/// Where a bubble stands: its scroll area and the anchors' keys in order of preference (`BubbleAnchor.key`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub area: Area,
    pub keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Notice {
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub actions: Vec<Action>,
    pub click: Option<Click>,
    /// A message with a key is one thing that changes (a progress, a state): showing it again updates it in place.
    pub key: Option<String>,
    /// 0..=1 for a running task (the probe render).
    pub progress: Option<f32>,
    /// A toast that stays until it is shown again or cleared (a running task), whatever its level's time.
    pub stays: bool,
    /// Goes to the desktop whatever the routing says ("when done: notify").
    pub force_desktop: bool,
    /// Seconds the desktop notification stays instead of the setting's.
    pub desktop_secs: Option<u32>,
    /// A bubble's place (kinds that go to a bubble).
    pub anchor: Option<Anchor>,
    /// A bubble waits this long for the next change of its key (an input check while typing).
    pub grace: Duration,
}

impl Notice {
    /// A message of `kind` at its usual level.
    pub fn new(kind: Kind, title: impl Into<String>) -> Self {
        Self {
            kind,
            level: kind.info().level,
            title: title.into(),
            detail: String::new(),
            actions: Vec::new(),
            click: None,
            key: None,
            progress: None,
            stays: false,
            force_desktop: false,
            desktop_secs: None,
            anchor: None,
            grace: Duration::ZERO,
        }
    }

    /// A bubble at the first of `keys` that is shown in `area` (its key is the first anchor unless it has one).
    #[must_use]
    pub fn at<S: Into<String>>(mut self, area: Area, keys: impl IntoIterator<Item = S>) -> Self {
        let keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        if self.key.is_none() {
            self.key = keys.first().cloned();
        }
        self.anchor = Some(Anchor { area, keys });
        self
    }

    /// A bubble that waits until its key had no other change for `grace` (an input check while typing).
    #[must_use]
    pub const fn grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    #[must_use]
    pub const fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// A button; the message closes after it ran.
    #[must_use]
    pub fn action(mut self, label: impl Into<String>, run: impl Fn() + 'static) -> Self {
        self.actions.push(Action {
            label: label.into(),
            run: Rc::new(run),
            closes: true,
            icon: None,
        });
        self
    }

    /// An icon button of a bubble, explained by `tooltip` (a bubble's link stays its first action without an icon).
    #[must_use]
    pub fn icon_action(
        mut self,
        icon: crate::icons::Icon,
        tooltip: impl Into<String>,
        run: impl Fn() + 'static,
    ) -> Self {
        self.actions.push(Action {
            label: tooltip.into(),
            run: Rc::new(run),
            closes: true,
            icon: Some(icon),
        });
        self
    }

    /// A button that leaves the message open (a "Stop" on a progress).
    #[must_use]
    pub fn action_keep(mut self, label: impl Into<String>, run: impl Fn() + 'static) -> Self {
        self.actions.push(Action {
            label: label.into(),
            run: Rc::new(run),
            closes: false,
            icon: None,
        });
        self
    }

    #[must_use]
    pub fn jump(mut self, target: JumpTarget) -> Self {
        self.click = Some(Click::Jump(target));
        self
    }

    #[must_use]
    pub fn on_click(mut self, run: impl Fn() + 'static) -> Self {
        self.click = Some(Click::Run(Rc::new(run)));
        self
    }

    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub const fn progress(mut self, fraction: f32) -> Self {
        self.progress = Some(fraction);
        self
    }

    /// A state that changes in place until it is cleared (keep its title while it runs; a new title is a new entry in
    /// the history).
    #[must_use]
    pub fn live(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self.stays = true;
        self
    }

    #[must_use]
    pub const fn force_desktop(mut self) -> Self {
        self.force_desktop = true;
        self
    }

    #[must_use]
    pub const fn desktop_secs(mut self, secs: u32) -> Self {
        self.desktop_secs = Some(secs);
        self
    }

    /// "Title: detail" (the status line, the log, the clipboard).
    pub fn text(&self) -> String {
        if self.detail.is_empty() {
            self.title.clone()
        } else {
            format!("{}: {}", self.title.trim_end_matches('.'), self.detail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_joins_title_and_detail() {
        let n = Notice::new(Kind::JobQueued, "Job 3 queued").detail("clip - 03.mkv");
        assert_eq!(n.text(), "Job 3 queued: clip - 03.mkv");
        assert_eq!(n.level, Level::Success);
        let n = Notice::new(Kind::SaveCanceled, "Not saved.").detail("The dialog was canceled.");
        assert_eq!(n.text(), "Not saved: The dialog was canceled.");
        assert_eq!(Notice::new(Kind::Ready, "Ready").text(), "Ready");
    }
}
