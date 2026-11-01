//! The folder wheel's navigation without Slint (P11.5, user decision Q-FP2; ported from the questionnaire's
//! `wheel.js`): holding the trigger arms it (the hold ring fills), pointing highlights a place, dwelling on a group (or
//! "More ...") fills it and opens it, pushing past the outer edge opens it at once, the middle goes one level up after
//! the same dwell (once per visit), releasing over a folder picks it, over a group opens it and keeps the wheel for
//! clicks, in the middle cancels. After a level opens, the pointer must move before a dwell or a push counts again (no
//! falling through levels). Opened by its key, the wheel turns with the keys instead. A wheel without favorites is empty:
//! its middle leads to Settings > Target Folders (after the dwell, a click or Enter; the user's wish 2026-10-28). Times are milliseconds from any
//! fixed start; positions are relative to the wheel's center. With "pointer in the middle" (P12 3.8) every level measures
//! the pointer from where it stood when the level opened, so it starts in the middle wherever it really is, and a short
//! move (`Radii::dead`) chooses a spoke.

use rustcut_core::folders::{FolderId, FolderTree, Layout, Level, LevelKey, Slot, level};

mod keys;

/// The timing of the wheel (Settings > Folder Wheel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long the mouse trigger is held before the wheel opens.
    pub hold_ms: u64,
    /// How long the pointer rests on a group (or the middle) before it opens (goes up).
    pub dwell_ms: u64,
    /// Pushing past the outer edge opens a group at once.
    pub push: bool,
    /// The pointer starts in the middle of every level (virtual: the real one stays where it is).
    pub center: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            hold_ms: 250,
            dwell_ms: 600,
            push: true,
            center: false,
        }
    }
}

/// The wheel's radii (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    /// The middle's edge.
    pub inner: f32,
    /// The places' outer edge.
    pub outer: f32,
    /// How far the pointer moves until it chooses a spoke, while it starts in the middle.
    pub dead: f32,
}

impl Radii {
    /// Beyond this a push opens a group.
    fn push_edge(self) -> f32 {
        self.outer + (self.outer - self.inner) * 0.25
    }
}

/// How far the pointer must move after a level opened before a dwell or a push counts again.
const UNFREEZE_PX: f32 = 14.0;

/// What the wheel and its key presses can be told by keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// A/D or Left/Right.
    Left,
    Right,
    /// W/Up: opens a group.
    Open,
    /// Enter: opens a group or picks a folder.
    Enter,
    /// S/Down: one level up.
    Back,
    /// The wheel's own key again: one level up, closes at the start.
    Again,
    Escape,
}

/// How the wheel is driven while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The trigger is held: releasing decides.
    Held,
    /// Released over a group: clicks go on.
    Clicks,
    /// Opened by its key.
    Keys,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dwell {
    None,
    /// The level just opened with the pointer in the middle: it did not point at anything yet.
    Start,
    Place {
        index: usize,
        since: u64,
    },
    Middle {
        since: u64,
    },
    /// The middle went up once; the pointer must leave it first.
    MiddleDone,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Arming { since: u64 },
    Open { mode: Mode },
}

/// What a step did (the bridge redraws, sets the folder or closes the overlay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// Something to draw changed (the hold ring, a highlight, a fill, a level).
    Changed,
    Opened,
    Picked(FolderId),
    Closed,
    /// The empty wheel's middle: Settings > Target Folders (the wheel closed).
    Settings,
}

/// The tree and how levels are laid out, for the steps that open levels.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub tree: &'a FolderTree,
    pub layout: Layout,
    /// The job's target folder (the keys start from its place).
    pub current: Option<FolderId>,
}

/// A level opened: into a place, or back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChange {
    pub back: bool,
    pub place: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct WheelNav {
    timing: Timing,
    radii: Radii,
    phase: Phase,
    /// The levels opened, the start first (empty while closed).
    path: Vec<Level>,
    highlight: Option<usize>,
    dwell: Dwell,
    /// Where the pointer really is.
    pointer: (f32, f32),
    /// Where it stood when the level opened (with `Timing::center`).
    origin: (f32, f32),
    freeze: Option<(f32, f32)>,
    /// The last level change, until the bridge took it (it animates).
    change: Option<LevelChange>,
    /// The groups of the tree when the wheel opened (their places open a level).
    groups: Vec<FolderId>,
    /// Opened by its key: where the pointer stood (a real move hands the wheel to the pointer: clicks and dwells).
    keys_anchor: Option<(f32, f32)>,
}

impl WheelNav {
    pub fn new(timing: Timing, radii: Radii) -> Self {
        Self {
            timing,
            radii,
            phase: Phase::Idle,
            path: Vec::new(),
            highlight: None,
            dwell: Dwell::None,
            pointer: (0.0, 0.0),
            origin: (0.0, 0.0),
            freeze: None,
            change: None,
            groups: Vec::new(),
            keys_anchor: None,
        }
    }

    pub fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
    }

    pub fn set_radii(&mut self, radii: Radii) {
        self.radii = radii;
    }

    /// The pointer as the wheel counts it: from the middle while it starts there.
    fn seen(&self, x: f32, y: f32) -> (f32, f32) {
        if self.timing.center {
            (x - self.origin.0, y - self.origin.1)
        } else {
            (x, y)
        }
    }

    /// The radius inside which the pointer is in the middle.
    fn middle_radius(&self) -> f32 {
        if self.timing.center {
            self.radii.dead
        } else {
            self.radii.inner
        }
    }

    /// The pointer in the wheel while it starts in the middle (the bridge draws a dot there); it stays on the wheel.
    pub fn virtual_pointer(&self) -> Option<(f32, f32)> {
        if !self.timing.center || !self.is_open() || self.mode() == Some(Mode::Keys) {
            return None;
        }
        let (x, y) = self.seen(self.pointer.0, self.pointer.1);
        let scale = (self.radii.outer / x.hypot(y).max(1.0)).min(1.0);
        Some((x * scale, y * scale))
    }

    /// The level change since the last call.
    pub const fn take_change(&mut self) -> Option<LevelChange> {
        self.change.take()
    }

    pub const fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Open { .. })
    }

    pub const fn is_arming(&self) -> bool {
        matches!(self.phase, Phase::Arming { .. })
    }

    pub fn mode(&self) -> Option<Mode> {
        match self.phase {
            Phase::Open { mode } => Some(mode),
            _ => None,
        }
    }

    /// The level shown.
    pub fn level(&self) -> Option<&Level> {
        self.path.last()
    }

    /// The opened levels below the start (the optional breadcrumb).
    pub fn depth(&self) -> usize {
        self.path.len().saturating_sub(1)
    }

    pub fn path(&self) -> &[Level] {
        &self.path
    }

    pub const fn highlight(&self) -> Option<usize> {
        self.highlight
    }

    /// The hold ring's fill (0..1) while arming.
    pub fn hold_progress(&self, now: u64) -> f32 {
        match self.phase {
            Phase::Arming { since } => fraction(now.saturating_sub(since), self.timing.hold_ms),
            _ => 0.0,
        }
    }

    /// The place filling while the pointer dwells on a group, and how far (0..1).
    pub fn fill(&self, now: u64) -> Option<(usize, f32)> {
        match self.dwell {
            Dwell::Place { index, since } if self.freeze.is_none() && self.opens(index) => {
                Some((index, fraction(now.saturating_sub(since), self.timing.dwell_ms)))
            }
            _ => None,
        }
    }

    /// The first level has no entries (no favorites): the middle leads to the settings.
    pub fn is_empty(&self) -> bool {
        self.depth() == 0
            && self
                .level()
                .is_some_and(|l| l.slots.iter().all(|s| matches!(s, Slot::Empty)))
    }

    /// The pointer is in the middle (shown as "back", "cancel" or the settings jump) and how far its dwell is (0..1; 0
    /// where the middle only cancels).
    pub fn middle(&self, now: u64) -> Option<f32> {
        match self.dwell {
            Dwell::Middle { since } if self.depth() > 0 || self.is_empty() => {
                Some(fraction(now.saturating_sub(since), self.timing.dwell_ms))
            }
            Dwell::Middle { .. } | Dwell::MiddleDone => Some(0.0),
            Dwell::None | Dwell::Start | Dwell::Place { .. } => None,
        }
    }

    /// Whether a timer must keep calling [`WheelNav::tick`] (a hold or a dwell runs).
    pub fn needs_ticks(&self) -> bool {
        match self.phase {
            Phase::Arming { .. } => true,
            Phase::Open {
                mode: Mode::Held | Mode::Clicks,
            } => {
                matches!(self.dwell, Dwell::Middle { .. }) && (self.depth() > 0 || self.is_empty())
                    || matches!(self.dwell, Dwell::Place { index, .. } if self.freeze.is_none() && self.opens(index))
            }
            _ => false,
        }
    }

    fn slot(&self, index: usize) -> Option<&Slot> {
        self.level()?.slots.get(index)
    }

    /// The place opens a level (a group or "More ...").
    fn opens(&self, index: usize) -> bool {
        match self.slot(index) {
            Some(Slot::More(_)) => true,
            Some(Slot::Entry(id)) => self.is_group(*id),
            _ => false,
        }
    }

    fn is_group(&self, id: FolderId) -> bool {
        self.groups.contains(&id)
    }

    /// The place under a point (relative to the center): `None` in the middle.
    pub fn place_at(&self, x: f32, y: f32) -> Option<usize> {
        let n = self.level().map_or(0, |l| l.slots.len());
        place_at(x, y, n, self.middle_radius())
    }

    // --- The mouse trigger ------------------------------------------------------------------------------------

    /// The trigger went down (over the video): the hold ring starts.
    pub fn press(&mut self, now: u64) -> Outcome {
        if !matches!(self.phase, Phase::Idle) {
            return Outcome::Nothing;
        }
        self.phase = Phase::Arming { since: now };
        Outcome::Changed
    }

    /// Time passes (the bridge's timer while [`WheelNav::needs_ticks`]).
    pub fn tick(&mut self, now: u64, ctx: Context<'_>) -> Outcome {
        let (x, y) = self.pointer;
        self.moved(x, y, now, ctx)
    }

    /// The pointer moved to (x, y) relative to the wheel's center.
    pub fn moved(&mut self, x: f32, y: f32, now: u64, ctx: Context<'_>) -> Outcome {
        self.pointer = (x, y);
        match self.phase {
            Phase::Idle => Outcome::Nothing,
            // Turned by keys: the pointer takes over once it really moves.
            Phase::Open { mode: Mode::Keys } => match self.keys_anchor {
                None => {
                    self.keys_anchor = Some((x, y));
                    Outcome::Nothing
                }
                Some((ax, ay)) if (x - ax).hypot(y - ay) > UNFREEZE_PX => {
                    self.phase = Phase::Open { mode: Mode::Clicks };
                    self.freeze = None;
                    self.origin = (ax, ay);
                    let (sx, sy) = self.seen(x, y);
                    self.point(sx, sy, now, ctx)
                }
                Some(_) => Outcome::Nothing,
            },
            Phase::Arming { since } => {
                if now.saturating_sub(since) >= self.timing.hold_ms {
                    self.open(Mode::Held, ctx);
                    Outcome::Opened
                } else {
                    Outcome::Changed
                }
            }
            Phase::Open { .. } => {
                let (sx, sy) = self.seen(x, y);
                self.point(sx, sy, now, ctx)
            }
        }
    }

    fn point(&mut self, x: f32, y: f32, now: u64, ctx: Context<'_>) -> Outcome {
        let distance = x.hypot(y);
        if distance < self.middle_radius() {
            self.highlight = None;
            match self.dwell {
                Dwell::MiddleDone | Dwell::Start => {}
                Dwell::Middle { since } => {
                    if self.is_empty() && now.saturating_sub(since) >= self.timing.dwell_ms {
                        self.close();
                        return Outcome::Settings;
                    }
                    if self.depth() > 0 && now.saturating_sub(since) >= self.timing.dwell_ms {
                        self.up(ctx);
                        // One level per visit of the middle.
                        self.dwell = Dwell::MiddleDone;
                    }
                }
                _ => self.dwell = Dwell::Middle { since: now },
            }
            return Outcome::Changed;
        }
        if let Some((fx, fy)) = self.freeze
            && (x - fx).hypot(y - fy) > UNFREEZE_PX
        {
            self.freeze = None;
            if let Dwell::Place { index, .. } = self.dwell {
                self.dwell = Dwell::Place { index, since: now };
            }
        }
        let Some(index) = self.place_at(x, y) else {
            return Outcome::Nothing;
        };
        let filled = !matches!(self.slot(index), None | Some(Slot::Empty));
        let wanted = filled.then_some(index);
        if self.highlight != wanted || !matches!(self.dwell, Dwell::Place { index: i, .. } if i == index) {
            self.highlight = wanted;
            self.dwell = Dwell::Place { index, since: now };
        }
        if self.freeze.is_none() && self.opens(index) {
            let pushed = self.timing.push && distance > self.radii.push_edge();
            let dwelt =
                matches!(self.dwell, Dwell::Place { since, .. } if now.saturating_sub(since) >= self.timing.dwell_ms);
            if pushed || dwelt {
                self.enter(index, ctx);
            }
        }
        Outcome::Changed
    }

    /// The trigger was released.
    pub fn release(&mut self, ctx: Context<'_>) -> Outcome {
        match self.phase {
            Phase::Arming { .. } => {
                self.close();
                Outcome::Closed
            }
            Phase::Open { mode: Mode::Held } => match self.highlight {
                Some(index) if self.opens(index) => {
                    self.enter(index, ctx);
                    self.phase = Phase::Open { mode: Mode::Clicks };
                    Outcome::Changed
                }
                Some(index) => self.pick(index),
                None => {
                    self.close();
                    Outcome::Closed
                }
            },
            _ => Outcome::Nothing,
        }
    }

    /// A click while the wheel waits for clicks (released over a group): a folder picks, a group opens, the middle
    /// goes up (closes at the start), outside the wheel closes.
    pub fn click(&mut self, x: f32, y: f32, ctx: Context<'_>) -> Outcome {
        if !matches!(
            self.phase,
            Phase::Open {
                mode: Mode::Clicks | Mode::Keys
            }
        ) {
            return Outcome::Nothing;
        }
        // A real click where the pointer is; with the pointer in the middle it counts where the wheel sees it.
        let (x, y) = if self.timing.center && self.mode() != Some(Mode::Keys) {
            self.seen(self.pointer.0, self.pointer.1)
        } else {
            (x, y)
        };
        let distance = x.hypot(y);
        if distance > self.radii.push_edge() {
            self.close();
            return Outcome::Closed;
        }
        match self.place_at(x, y) {
            None if self.depth() > 0 => {
                self.up(ctx);
                Outcome::Changed
            }
            None if self.is_empty() => {
                self.close();
                Outcome::Settings
            }
            None => {
                self.close();
                Outcome::Closed
            }
            Some(index) if self.opens(index) => {
                self.enter(index, ctx);
                Outcome::Changed
            }
            Some(index) => self.pick(index),
        }
    }

    /// Closes the wheel (Escape, the video went away).
    pub fn close(&mut self) {
        self.phase = Phase::Idle;
        self.path.clear();
        self.highlight = None;
        self.dwell = Dwell::None;
        self.freeze = None;
        self.change = None;
    }

    // --- Levels -------------------------------------------------------------------------------------------------

    fn open(&mut self, mode: Mode, ctx: Context<'_>) {
        self.groups = groups(ctx.tree);
        self.path = level(ctx.tree, &LevelKey::Start, ctx.layout).into_iter().collect();
        self.phase = Phase::Open { mode };
        self.highlight = None;
        self.dwell = if self.timing.center { Dwell::Start } else { Dwell::None };
        self.freeze = None;
        self.origin = self.pointer;
    }

    fn enter(&mut self, index: usize, ctx: Context<'_>) {
        let Some(key) = self.slot(index).and_then(|s| s.opens(ctx.tree)) else {
            return;
        };
        if let Some(next) = level(ctx.tree, &key, ctx.layout) {
            self.path.push(next);
        }
        self.change = Some(LevelChange {
            back: false,
            place: Some(index),
        });
        self.after_level_change();
    }

    fn up(&mut self, _ctx: Context<'_>) {
        if self.path.len() > 1 {
            self.path.pop();
        }
        self.change = Some(LevelChange {
            back: true,
            place: None,
        });
        self.after_level_change();
    }

    fn after_level_change(&mut self) {
        self.highlight = None;
        if self.timing.center {
            // Every level starts in the middle again.
            self.origin = self.pointer;
            self.dwell = Dwell::Start;
        } else {
            self.dwell = Dwell::None;
            self.freeze = Some(self.pointer);
        }
    }

    /// The place holding the current folder on this level.
    fn place_of(&self, ctx: Context<'_>) -> Option<usize> {
        let current = ctx.current?;
        self.level()?.slots.iter().position(|s| s.holds(ctx.tree, current))
    }

    fn pick(&mut self, index: usize) -> Outcome {
        let picked = match self.slot(index) {
            Some(Slot::Entry(id)) => Some(*id),
            _ => None,
        };
        self.close();
        picked.map_or(Outcome::Closed, Outcome::Picked)
    }
}

/// The ids of every group (the places that open a level).
fn groups(tree: &FolderTree) -> Vec<FolderId> {
    tree.walk()
        .iter()
        .filter(|w| w.node.is_group())
        .map(|w| w.node.id)
        .collect()
}

fn fraction(elapsed: u64, total: u64) -> f32 {
    if total == 0 {
        return 1.0;
    }
    // Up to a minute of milliseconds: exact in f32.
    let part = u16::try_from(elapsed.min(total).min(60_000)).unwrap_or(u16::MAX);
    let whole = u16::try_from(total.min(60_000)).unwrap_or(u16::MAX).max(1);
    (f32::from(part) / f32::from(whole)).clamp(0.0, 1.0)
}

/// The place under (x, y) of `n` places clockwise from the top; `None` inside `inner` or without places.
pub fn place_at(x: f32, y: f32, n: usize, inner: f32) -> Option<usize> {
    if n == 0 || x.hypot(y) < inner {
        return None;
    }
    // Clockwise from the top: atan2 of (x, -y).
    let angle = x.atan2(-y).rem_euclid(std::f32::consts::TAU);
    let step = std::f32::consts::TAU / f32::from(u16::try_from(n).unwrap_or(u16::MAX));
    let index: usize = az::saturating_cast((angle / step).round());
    Some(index % n)
}

#[cfg(test)]
mod tests;
