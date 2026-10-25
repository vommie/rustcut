//! The toasts (pure, Q-N8): cards that leave by themselves after their level's time (errors stay), a repeat counts up
//! in the visible card and starts its time again, the pointer on a card pauses it, a card with a key is one thing that
//! changes in place (the probe's progress), and the layout follows the stacking setting. Times come from the caller.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use rustcut_core::settings::{ToastOrder, ToastSettings, ToastStacking};

use super::kind::{Kind, Level};
use super::notice::Label;

/// How long a closed card fades out before it is removed (the Slint side animates it).
pub const LEAVE: Duration = Duration::from_millis(220);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// Until it is closed (errors by default, keyed messages while they change).
    Stays,
    Running {
        total: Duration,
        deadline: Instant,
    },
    /// The pointer rests on it.
    Paused {
        total: Duration,
        left: Duration,
    },
}

/// A card as the stack keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub key: Option<String>,
    pub progress: Option<f32>,
    /// The labels of its buttons (the runs stay with the center).
    pub actions: Vec<Label>,
    pub clickable: bool,
    /// How often it came ("×3").
    pub count: u32,
    /// "15:42", when it came (last).
    pub time: String,
    /// Order of arrival; a repeat counts as new.
    pub seq: u64,
    pub timer: Timer,
    /// Counts the times its time started again (the line under it starts over).
    pub restarts: u32,
    pub leaving: Option<Instant>,
}

/// A card to add.
#[derive(Debug, Clone, PartialEq)]
pub struct NewToast {
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub key: Option<String>,
    pub progress: Option<f32>,
    pub actions: Vec<Label>,
    pub clickable: bool,
    pub time: String,
    /// `None` = stays until closed.
    pub shown_for: Option<Duration>,
}

/// What [`ToastStack::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    New(u64),
    /// The card with the same key changed.
    Updated(u64),
    /// The same message again: its count went up.
    Repeated(u64),
}

impl Added {
    pub const fn id(self) -> u64 {
        match self {
            Self::New(id) | Self::Updated(id) | Self::Repeated(id) => id,
        }
    }
}

/// A card in the layout; `depth` = its rank in the deck (0 in front).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub id: u64,
    pub depth: u32,
}

/// "5 messages: 1 error, 2 warnings".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// The cards from top to bottom (the deck: the front card first).
    pub shown: Vec<Shown>,
    /// Cards that do not fit ("+2 more").
    pub more: usize,
    /// The pager: (shown one, of how many).
    pub pager: Option<(usize, usize)>,
    pub summary: Option<Summary>,
}

#[derive(Debug, Default)]
pub struct ToastStack {
    toasts: Vec<Toast>,
    next_id: u64,
    next_seq: u64,
    /// "+N more" opened, the deck fanned out, the summary opened.
    expanded: bool,
    page: usize,
    /// The cards of the last layout: only they fade out, the others go at once.
    shown: HashSet<u64>,
}

impl ToastStack {
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn get(&self, id: u64) -> Option<&Toast> {
        self.toasts.iter().find(|t| t.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// The cards that are not leaving.
    pub fn active(&self) -> usize {
        self.toasts.iter().filter(|t| t.leaving.is_none()).count()
    }

    pub fn add(&mut self, new: NewToast, now: Instant) -> Added {
        self.next_seq += 1;
        let seq = self.next_seq;
        let timer = start(new.shown_for, now);
        if let Some(key) = &new.key
            && let Some(toast) = self
                .toasts
                .iter_mut()
                .find(|t| t.leaving.is_none() && t.key.as_ref() == Some(key))
        {
            let paused = matches!(toast.timer, Timer::Paused { .. });
            *toast = Toast {
                id: toast.id,
                count: toast.count,
                seq: toast.seq,
                timer: if paused { pause(timer, now) } else { timer },
                restarts: toast.restarts.wrapping_add(1),
                leaving: None,
                kind: new.kind,
                level: new.level,
                title: new.title,
                detail: new.detail,
                key: new.key,
                progress: new.progress,
                actions: new.actions,
                clickable: new.clickable,
                time: new.time,
            };
            return Added::Updated(toast.id);
        }
        if new.key.is_none()
            && let Some(toast) = self.toasts.iter_mut().find(|t| {
                t.leaving.is_none()
                    && t.key.is_none()
                    && t.kind == new.kind
                    && t.title == new.title
                    && t.detail == new.detail
            })
        {
            toast.count = toast.count.saturating_add(1);
            toast.seq = seq;
            toast.time = new.time;
            toast.level = toast.level.max(new.level);
            toast.timer = if matches!(toast.timer, Timer::Paused { .. }) {
                pause(timer, now)
            } else {
                timer
            };
            toast.restarts = toast.restarts.wrapping_add(1);
            return Added::Repeated(toast.id);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.toasts.push(Toast {
            id,
            kind: new.kind,
            level: new.level,
            title: new.title,
            detail: new.detail,
            key: new.key,
            progress: new.progress,
            actions: new.actions,
            clickable: new.clickable,
            count: 1,
            time: new.time,
            seq,
            timer,
            restarts: 0,
            leaving: None,
        });
        Added::New(id)
    }

    /// Closes a card: it fades out when it is shown, else it goes at once.
    pub fn close(&mut self, id: u64, now: Instant) -> bool {
        let shown = self.shown.contains(&id);
        let Some(pos) = self.toasts.iter().position(|t| t.id == id && t.leaving.is_none()) else {
            return false;
        };
        if shown {
            if let Some(toast) = self.toasts.get_mut(pos) {
                toast.leaving = Some(now);
            }
        } else {
            self.toasts.remove(pos);
        }
        true
    }

    /// Closes the card with `key` (a state that ended).
    pub fn close_key(&mut self, key: &str, now: Instant) -> Option<u64> {
        let id = self
            .toasts
            .iter()
            .find(|t| t.leaving.is_none() && t.key.as_deref() == Some(key))?
            .id;
        self.close(id, now).then_some(id)
    }

    pub fn close_all(&mut self, now: Instant) {
        let ids: Vec<u64> = self
            .toasts
            .iter()
            .filter(|t| t.leaving.is_none())
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.close(id, now);
        }
        self.expanded = false;
    }

    /// The pointer came onto (`true`) or left a card: its time stops and goes on.
    pub fn hover(&mut self, id: u64, on: bool, now: Instant) {
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
            toast.timer = if on {
                pause(toast.timer, now)
            } else {
                resume(toast.timer, now)
            };
        }
    }

    /// Times out cards and removes faded ones; `true` when something changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let expired: Vec<u64> = self
            .toasts
            .iter()
            .filter(|t| t.leaving.is_none() && matches!(t.timer, Timer::Running { deadline, .. } if deadline <= now))
            .map(|t| t.id)
            .collect();
        for id in expired {
            changed |= self.close(id, now);
        }
        let before = self.toasts.len();
        self.toasts.retain(|t| {
            t.leaving
                .is_none_or(|since| now.saturating_duration_since(since) < LEAVE)
        });
        changed |= self.toasts.len() != before;
        if self.active() == 0 {
            self.expanded = false;
        }
        changed
    }

    /// How much of its time a card has left (the line under it); `None` = it stays.
    pub fn remaining(toast: &Toast, now: Instant) -> Option<f32> {
        let (left, total) = match toast.timer {
            Timer::Stays => return None,
            Timer::Running { total, deadline } => (deadline.saturating_duration_since(now), total),
            Timer::Paused { total, left } => (left, total),
        };
        (!total.is_zero()).then(|| (left.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0))
    }

    pub const fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, on: bool) {
        self.expanded = on;
    }

    /// The pager: one card further (+1) or back (-1).
    pub fn page(&mut self, delta: i32) {
        let n = self.active();
        if n == 0 {
            self.page = 0;
            return;
        }
        let step = usize::try_from(delta.unsigned_abs()).unwrap_or(0) % n;
        self.page = if delta < 0 {
            (self.page + n - step) % n
        } else {
            (self.page + step) % n
        };
    }

    /// The cards to draw for `settings`; remembers them for [`Self::close`].
    pub fn layout(&mut self, settings: &ToastSettings) -> Layout {
        let mut order: Vec<&Toast> = self.toasts.iter().collect();
        order.sort_by(|a, b| match settings.order {
            ToastOrder::NewestTop => b.seq.cmp(&a.seq),
            ToastOrder::NewestBottom => a.seq.cmp(&b.seq),
            ToastOrder::Severity => b.level.cmp(&a.level).then(b.seq.cmp(&a.seq)),
        });
        let active: Vec<&Toast> = order.iter().copied().filter(|t| t.leaving.is_none()).collect();
        let max = usize::from(settings.max_visible.max(1));
        let mut layout = Layout::default();
        // Leaving cards stay where they were drawn until they faded.
        let leaving = |t: &Toast| t.leaving.is_some() && self.shown.contains(&t.id);
        match settings.stacking {
            ToastStacking::Pager => {
                let n = active.len();
                self.page = self.page.min(n.saturating_sub(1));
                let current = active.get(self.page).map(|t| t.id);
                layout.shown = order
                    .iter()
                    .filter(|t| Some(t.id) == current || leaving(t))
                    .map(|t| Shown { id: t.id, depth: 0 })
                    .collect();
                layout.pager = (n > 1).then_some((self.page, n));
            }
            ToastStacking::Summary if active.len() >= 2 && !self.expanded => {
                let errors = active.iter().filter(|t| t.level == Level::Error).count();
                let warnings = active.iter().filter(|t| t.level == Level::Warning).count();
                layout.summary = Some(Summary {
                    total: active.len(),
                    errors,
                    warnings,
                    level: active.iter().map(|t| t.level).max().unwrap_or(Level::Info),
                });
            }
            stacking => {
                let limit = match stacking {
                    ToastStacking::All | ToastStacking::Summary => usize::MAX,
                    ToastStacking::Some if self.expanded => usize::MAX,
                    _ => max,
                };
                let mut counted = 0_u32;
                for toast in &order {
                    if toast.leaving.is_some() {
                        if leaving(toast) {
                            layout.shown.push(Shown { id: toast.id, depth: 0 });
                        }
                    } else if usize::try_from(counted).unwrap_or(usize::MAX) < limit {
                        // The deck's rank stays while it is fanned out (the Slint side places the cards).
                        let depth = if stacking == ToastStacking::Deck { counted } else { 0 };
                        layout.shown.push(Shown { id: toast.id, depth });
                        counted += 1;
                    } else {
                        layout.more += 1;
                    }
                }
            }
        }
        self.shown = layout.shown.iter().map(|s| s.id).collect();
        layout
    }
}

fn start(shown_for: Option<Duration>, now: Instant) -> Timer {
    match shown_for {
        None => Timer::Stays,
        Some(total) => match now.checked_add(total) {
            Some(deadline) => Timer::Running { total, deadline },
            None => Timer::Stays,
        },
    }
}

fn pause(timer: Timer, now: Instant) -> Timer {
    match timer {
        Timer::Running { total, deadline } => Timer::Paused {
            total,
            left: deadline.saturating_duration_since(now),
        },
        other => other,
    }
}

fn resume(timer: Timer, now: Instant) -> Timer {
    match timer {
        Timer::Paused { total, left } => start(Some(left), now).with_total(total),
        other => other,
    }
}

impl Timer {
    const fn with_total(self, total: Duration) -> Self {
        match self {
            Self::Running { deadline, .. } => Self::Running { total, deadline },
            other => other,
        }
    }
}

#[cfg(test)]
#[path = "toasts_tests.rs"]
mod tests;
