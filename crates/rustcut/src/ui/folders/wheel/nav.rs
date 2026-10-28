//! The folder wheel's navigation without Slint (P11.5, user decision Q-FP2; ported from the questionnaire's
//! `wheel.js`): holding the trigger arms it (the hold ring fills), pointing highlights a place, dwelling on a group (or
//! "More ...") fills it and opens it, pushing past the outer edge opens it at once, the middle goes one level up after
//! the same dwell (once per visit), releasing over a folder picks it, over a group opens it and keeps the wheel for
//! clicks, in the middle cancels. After a level opens, the pointer must move before a dwell or a push counts again (no
//! falling through levels). Opened by its key, the wheel turns with the keys instead. A wheel without favorites is empty:
//! its middle leads to Settings > Target Folders (after the dwell, a click or Enter; the user's wish 2026-10-28). Times are milliseconds from any
//! fixed start; positions are relative to the wheel's center.

use rustcut_core::folders::{FolderId, FolderTree, Layout, Level, LevelKey, Slot, level};

/// The timing of the wheel (Settings > Folder Wheel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long the mouse trigger is held before the wheel opens.
    pub hold_ms: u64,
    /// How long the pointer rests on a group (or the middle) before it opens (goes up).
    pub dwell_ms: u64,
    /// Pushing past the outer edge opens a group at once.
    pub push: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            hold_ms: 250,
            dwell_ms: 600,
            push: true,
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
    pointer: (f32, f32),
    freeze: Option<(f32, f32)>,
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
            freeze: None,
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
            _ => None,
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
        place_at(x, y, n, self.radii.inner)
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
                    self.point(x, y, now, ctx)
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
            Phase::Open { .. } => self.point(x, y, now, ctx),
        }
    }

    fn point(&mut self, x: f32, y: f32, now: u64, ctx: Context<'_>) -> Outcome {
        let distance = x.hypot(y);
        if distance < self.radii.inner {
            self.highlight = None;
            match self.dwell {
                Dwell::MiddleDone => {}
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

    // --- The key trigger --------------------------------------------------------------------------------------

    /// Opened by its key: turns with the keys, the first place highlighted.
    pub fn open_by_key(&mut self, ctx: Context<'_>) -> Outcome {
        if self.is_open() {
            return self.key(KeyAction::Again, ctx);
        }
        self.open(Mode::Keys, ctx);
        self.keys_anchor = None;
        self.highlight = self.first_filled();
        Outcome::Opened
    }

    pub fn key(&mut self, action: KeyAction, ctx: Context<'_>) -> Outcome {
        if !self.is_open() {
            return Outcome::Nothing;
        }
        let current = self.highlight.unwrap_or(0);
        if self.is_empty() && matches!(action, KeyAction::Enter | KeyAction::Open) {
            self.close();
            return Outcome::Settings;
        }
        match action {
            KeyAction::Right => self.highlight = self.next_filled(current, true),
            KeyAction::Left => self.highlight = self.next_filled(current, false),
            KeyAction::Open | KeyAction::Enter => {
                if self.opens(current) {
                    self.enter(current, ctx);
                    self.highlight = self.first_filled();
                } else if action == KeyAction::Enter && self.highlight.is_some() {
                    return self.pick(current);
                }
            }
            KeyAction::Back | KeyAction::Again => {
                if self.depth() > 0 {
                    self.up(ctx);
                    self.highlight = self.first_filled();
                } else if action == KeyAction::Again {
                    self.close();
                    return Outcome::Closed;
                }
            }
            KeyAction::Escape => {
                self.close();
                return Outcome::Closed;
            }
        }
        if self.mode() == Some(Mode::Clicks) {
            self.phase = Phase::Open { mode: Mode::Keys };
        }
        Outcome::Changed
    }

    /// Closes the wheel (Escape, the video went away).
    pub fn close(&mut self) {
        self.phase = Phase::Idle;
        self.path.clear();
        self.highlight = None;
        self.dwell = Dwell::None;
        self.freeze = None;
    }

    // --- Levels -------------------------------------------------------------------------------------------------

    fn open(&mut self, mode: Mode, ctx: Context<'_>) {
        self.groups = groups(ctx.tree);
        self.path = level(ctx.tree, &LevelKey::Start, ctx.layout).into_iter().collect();
        self.phase = Phase::Open { mode };
        self.highlight = None;
        self.dwell = Dwell::None;
        self.freeze = None;
    }

    fn enter(&mut self, index: usize, ctx: Context<'_>) {
        let Some(key) = self.slot(index).and_then(|s| s.opens(ctx.tree)) else {
            return;
        };
        if let Some(next) = level(ctx.tree, &key, ctx.layout) {
            self.path.push(next);
        }
        self.after_level_change();
    }

    fn up(&mut self, _ctx: Context<'_>) {
        if self.path.len() > 1 {
            self.path.pop();
        }
        self.after_level_change();
    }

    fn after_level_change(&mut self) {
        self.highlight = None;
        self.dwell = Dwell::None;
        self.freeze = Some(self.pointer);
    }

    fn pick(&mut self, index: usize) -> Outcome {
        let picked = match self.slot(index) {
            Some(Slot::Entry(id)) => Some(*id),
            _ => None,
        };
        self.close();
        picked.map_or(Outcome::Closed, Outcome::Picked)
    }

    fn filled(&self) -> Vec<usize> {
        self.level().map_or_else(Vec::new, |l| {
            l.slots
                .iter()
                .enumerate()
                .filter(|(_, s)| !matches!(s, Slot::Empty))
                .map(|(i, _)| i)
                .collect()
        })
    }

    fn first_filled(&self) -> Option<usize> {
        self.filled().first().copied()
    }

    fn next_filled(&self, from: usize, forward: bool) -> Option<usize> {
        let n = self.level().map_or(0, |l| l.slots.len());
        let filled = self.filled();
        if filled.is_empty() || n == 0 {
            return None;
        }
        (1..=n)
            .map(|step| {
                if forward {
                    (from + step) % n
                } else {
                    (from + n * 2 - step) % n
                }
            })
            .find(|i| filled.contains(i))
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
mod tests {
    use std::path::Path;

    use rustcut_core::folders::Spot;

    use super::*;

    const RADII: Radii = Radii {
        inner: 30.0,
        outer: 100.0,
    };

    /// Favorites on 4 places: the source 0, Clients 1 (group: North 2, South 3), Drone 4 on place 2.
    fn tree() -> FolderTree {
        let mut t = FolderTree::default();
        let c = t.add_group("Clients", Spot::End).unwrap();
        t.add_folder(Path::new("/n"), Spot::Into(c)).unwrap();
        t.add_folder(Path::new("/s"), Spot::Into(c)).unwrap();
        let d = t.add_folder(Path::new("/d"), Spot::End).unwrap();
        t.put_on_place(FolderId::SOURCE, 0).unwrap();
        t.put_on_place(c, 1).unwrap();
        t.put_on_place(d, 2).unwrap();
        t
    }

    fn ctx(t: &FolderTree) -> Context<'_> {
        Context {
            tree: t,
            layout: Layout {
                places: 4,
                divided: false,
            },
        }
    }

    /// The middle of place `i` of 4 (0 top, 1 right, 2 bottom, 3 left).
    fn spot(i: usize) -> (f32, f32) {
        [(0.0, -65.0), (65.0, 0.0), (0.0, 65.0), (-65.0, 0.0)][i]
    }

    fn held(t: &FolderTree) -> WheelNav {
        let mut nav = WheelNav::new(Timing::default(), RADII);
        assert_eq!(nav.press(0), Outcome::Changed);
        assert!(nav.is_arming());
        assert!((nav.hold_progress(125) - 0.5).abs() < 0.01);
        assert_eq!(nav.moved(0.0, 0.0, 100, ctx(t)), Outcome::Changed);
        assert_eq!(nav.tick(250, ctx(t)), Outcome::Opened);
        nav
    }

    #[test]
    fn the_empty_wheels_middle_leads_to_the_settings() {
        let t = FolderTree::default();
        let mut nav = held(&t);
        assert!(nav.is_empty());
        nav.moved(5.0, 0.0, 300, ctx(&t));
        assert!(nav.middle(600).is_some_and(|p| p > 0.4));
        assert!(nav.needs_ticks());
        assert_eq!(nav.tick(900, ctx(&t)), Outcome::Settings);
        assert!(!nav.is_open());
        let mut nav = WheelNav::new(Timing::default(), RADII);
        nav.open_by_key(ctx(&t));
        assert_eq!(nav.highlight(), None, "nothing to point at");
        assert_eq!(nav.key(KeyAction::Enter, ctx(&t)), Outcome::Settings);
        nav.open_by_key(ctx(&t));
        nav.key(KeyAction::Right, ctx(&t));
        assert_eq!(nav.click(0.0, 0.0, ctx(&t)), Outcome::Settings);
        // Opened by key, the pointer takes over once it moves: resting in the middle leads there too.
        nav.open_by_key(ctx(&t));
        assert_eq!(nav.moved(90.0, 0.0, 1_000, ctx(&t)), Outcome::Nothing, "where it stood");
        assert_eq!(nav.moved(92.0, 0.0, 1_010, ctx(&t)), Outcome::Nothing, "a jitter");
        assert_eq!(nav.moved(5.0, 0.0, 1_100, ctx(&t)), Outcome::Changed);
        assert_eq!(nav.mode(), Some(Mode::Clicks));
        assert_eq!(nav.tick(1_700, ctx(&t)), Outcome::Settings);
    }

    #[test]
    fn places_are_found_clockwise_from_the_top() {
        assert_eq!(place_at(0.0, -50.0, 4, 30.0), Some(0));
        assert_eq!(place_at(50.0, 0.0, 4, 30.0), Some(1));
        assert_eq!(place_at(0.0, 50.0, 4, 30.0), Some(2));
        assert_eq!(place_at(-50.0, 0.0, 4, 30.0), Some(3));
        assert_eq!(
            place_at(-50.0, -45.0, 4, 30.0),
            Some(3),
            "just left of the top-left diagonal"
        );
        assert_eq!(place_at(5.0, 5.0, 4, 30.0), None, "the middle");
        assert_eq!(place_at(50.0, 0.0, 0, 30.0), None);
    }

    #[test]
    fn a_short_press_is_a_normal_click() {
        let t = tree();
        let mut nav = WheelNav::new(Timing::default(), RADII);
        nav.press(0);
        nav.moved(3.0, 0.0, 100, ctx(&t));
        assert_eq!(nav.release(ctx(&t)), Outcome::Closed);
        assert!(!nav.is_open() && !nav.is_arming());
    }

    #[test]
    fn releasing_over_a_folder_picks_it() {
        let t = tree();
        let mut nav = held(&t);
        let (x, y) = spot(2);
        nav.moved(x, y, 300, ctx(&t));
        assert_eq!(nav.highlight(), Some(2));
        assert_eq!(nav.release(ctx(&t)), Outcome::Picked(FolderId(4)));
        assert!(!nav.is_open());
    }

    #[test]
    fn dwelling_opens_a_group_and_the_middle_goes_back_once_per_visit() {
        let t = tree();
        let mut nav = held(&t);
        let (x, y) = spot(1);
        nav.moved(x, y, 300, ctx(&t));
        assert_eq!(nav.fill(600).map(|(i, _)| i), Some(1));
        assert!(nav.needs_ticks());
        nav.tick(899, ctx(&t));
        assert_eq!(nav.depth(), 0, "not yet");
        nav.tick(900, ctx(&t));
        assert_eq!(nav.depth(), 1, "Clients opened after the dwell");
        assert_eq!(nav.level().unwrap().slots.first(), Some(&Slot::Entry(FolderId(2))));
        // Frozen: resting on the same spot does not open anything again.
        nav.tick(2_000, ctx(&t));
        assert_eq!(nav.fill(2_000), None);
        // The middle: back after the dwell, once per visit.
        nav.moved(0.0, 0.0, 2_100, ctx(&t));
        assert_eq!(nav.middle(2_400).map(|p| p > 0.4), Some(true));
        nav.tick(2_700, ctx(&t));
        assert_eq!(nav.depth(), 0);
        nav.tick(4_000, ctx(&t));
        assert_eq!(nav.depth(), 0, "still at the start");
        // Releasing in the middle cancels.
        assert_eq!(nav.release(ctx(&t)), Outcome::Closed);
    }

    #[test]
    fn pushing_past_the_edge_opens_at_once_and_a_release_over_a_group_keeps_the_wheel() {
        let t = tree();
        let mut nav = held(&t);
        nav.moved(110.0, 0.0, 300, ctx(&t));
        assert_eq!(nav.depth(), 0, "inside the push edge");
        nav.moved(130.0, 0.0, 310, ctx(&t));
        assert_eq!(nav.depth(), 1, "pushed through");
        let mut nav = held(&t);
        let (x, y) = spot(1);
        nav.moved(x, y, 300, ctx(&t));
        assert_eq!(nav.release(ctx(&t)), Outcome::Changed);
        assert_eq!((nav.mode(), nav.depth()), (Some(Mode::Clicks), 1));
        // Clicks go on: the middle goes up, a folder picks.
        assert_eq!(nav.click(0.0, 0.0, ctx(&t)), Outcome::Changed);
        assert_eq!(nav.depth(), 0);
        let (x, y) = spot(2);
        assert_eq!(nav.click(x, y, ctx(&t)), Outcome::Picked(FolderId(4)));
    }

    #[test]
    fn the_keys_turn_open_go_back_and_close() {
        let t = tree();
        let mut nav = WheelNav::new(Timing::default(), RADII);
        assert_eq!(nav.open_by_key(ctx(&t)), Outcome::Opened);
        assert_eq!((nav.mode(), nav.highlight()), (Some(Mode::Keys), Some(0)));
        nav.key(KeyAction::Left, ctx(&t));
        assert_eq!(nav.highlight(), Some(2), "empty places are skipped");
        nav.key(KeyAction::Right, ctx(&t));
        nav.key(KeyAction::Right, ctx(&t));
        assert_eq!(nav.highlight(), Some(1));
        nav.key(KeyAction::Open, ctx(&t));
        assert_eq!((nav.depth(), nav.highlight()), (1, Some(0)));
        nav.key(KeyAction::Back, ctx(&t));
        assert_eq!(nav.depth(), 0);
        nav.key(KeyAction::Right, ctx(&t));
        nav.key(KeyAction::Enter, ctx(&t));
        assert_eq!(nav.depth(), 1, "Enter opens a group");
        assert_eq!(
            nav.open_by_key(ctx(&t)),
            Outcome::Changed,
            "the key again: one level up"
        );
        assert_eq!(nav.depth(), 0);
        assert_eq!(
            nav.key(KeyAction::Again, ctx(&t)),
            Outcome::Closed,
            "at the start it closes"
        );
        nav.open_by_key(ctx(&t));
        assert_eq!(nav.key(KeyAction::Enter, ctx(&t)), Outcome::Picked(FolderId::SOURCE));
        nav.open_by_key(ctx(&t));
        assert_eq!(nav.key(KeyAction::Escape, ctx(&t)), Outcome::Closed);
    }
}
