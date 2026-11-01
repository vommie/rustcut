//! The wheel opened and turned by its key (P11.5).

use super::{Context, KeyAction, Mode, Outcome, Phase, WheelNav};
use rustcut_core::folders::Slot;

impl WheelNav {
    /// Opened by its key: turns with the keys; nothing is chosen yet, the selection is in the middle (the user's note).
    pub fn open_by_key(&mut self, ctx: Context<'_>) -> Outcome {
        if self.is_open() {
            return self.key(KeyAction::Again, ctx);
        }
        self.open(Mode::Keys, ctx);
        self.keys_anchor = None;
        Outcome::Opened
    }

    pub fn key(&mut self, action: KeyAction, ctx: Context<'_>) -> Outcome {
        if !self.is_open() {
            return Outcome::Nothing;
        }
        if self.is_empty() && matches!(action, KeyAction::Enter | KeyAction::Open) {
            self.close();
            return Outcome::Settings;
        }
        match action {
            KeyAction::Right | KeyAction::Left => {
                self.highlight = match self.highlight {
                    Some(from) => self.next_filled(from, action == KeyAction::Right),
                    // The first turn starts at the current folder.
                    None => self.place_of(ctx).or_else(|| self.first_filled()),
                };
            }
            KeyAction::Open | KeyAction::Enter => {
                if let Some(chosen) = self.highlight {
                    if self.opens(chosen) {
                        self.enter(chosen, ctx);
                    } else if action == KeyAction::Enter {
                        return self.pick(chosen);
                    }
                }
            }
            KeyAction::Back | KeyAction::Again => {
                if self.depth() > 0 {
                    self.up(ctx);
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
