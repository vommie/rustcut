//! The configurable keymap: which keys trigger which [`Action`], read from `config.toml`'s `[keymap]` table (the core
//! stores it as plain strings; this module validates and sanitizes it), with conflict detection. The editing UI is part
//! of the Settings window (M4.2).
//!
//! Key presses reach the keymap through the main window's key scope: every press that no focused element took (a text
//! field keeps its plain keys). The menu bar's entries only show the first key of their action (P9.12: RustCut's own
//! menu bar; Slint's took its shortcuts before any element).

mod action;
mod chord;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

pub use action::Action;
pub use chord::{Chord, ChordError, KeyPress};

/// The raw table in `Settings.keymap`: action id -> key texts (`None` = a value that was not a list of strings).
pub type KeymapTable = BTreeMap<String, Option<Vec<String>>>;

/// A chord that is already bound to another action.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{chord} is already used by \"{}\"", other.title())]
pub struct Conflict {
    pub chord: String,
    pub other: Action,
}

/// The bindings of every action, in [`Action::ALL`] order. No two chords of different actions overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<(Action, Vec<Chord>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_table(&KeymapTable::new()).0
    }
}

impl Keymap {
    /// Builds the keymap from the stored table and returns a note for every repair (logged at startup): unknown actions
    /// and keys that do not parse are dropped, an unreadable value keeps the default, and a key bound twice stays with the
    /// action the user configured (configured actions before defaults, then in [`Action::ALL`] order).
    pub fn from_table(table: &KeymapTable) -> (Self, Vec<String>) {
        let mut notes: Vec<String> = table
            .keys()
            .filter(|id| Action::from_id(id).is_none())
            .map(|id| format!("unknown action `{id}` ignored"))
            .collect();
        let mut wanted: Vec<(Action, Vec<String>, bool)> = Vec::with_capacity(Action::ALL.len());
        for action in Action::ALL {
            let defaults = || action.default_keys().iter().map(|k| (*k).to_owned()).collect();
            match table.get(action.id()) {
                Some(Some(texts)) => wanted.push((action, texts.clone(), true)),
                Some(None) => {
                    notes.push(format!(
                        "{}: the value is not a list of keys; defaults used",
                        action.id()
                    ));
                    wanted.push((action, defaults(), false));
                }
                None => wanted.push((action, defaults(), false)),
            }
        }
        let mut keymap = Self {
            bindings: Action::ALL.iter().map(|a| (*a, Vec::new())).collect(),
        };
        // Configured actions claim their keys first.
        for configured in [true, false] {
            for (action, texts, _) in wanted.iter().filter(|(_, _, c)| *c == configured) {
                for text in texts {
                    let chord = match Chord::parse(text) {
                        Ok(chord) => chord,
                        Err(e) => {
                            notes.push(format!("{}: {e}", action.id()));
                            continue;
                        }
                    };
                    if let Err(conflict) = keymap.bind(*action, chord) {
                        notes.push(format!("{}: {conflict}; dropped", action.id()));
                    }
                }
            }
        }
        (keymap, notes)
    }

    /// The table to store: every action with its keys (an empty list = unbound on purpose).
    pub fn to_table(&self) -> KeymapTable {
        self.bindings
            .iter()
            .map(|(action, chords)| {
                (
                    action.id().to_owned(),
                    Some(chords.iter().map(|c| c.text().to_owned()).collect()),
                )
            })
            .collect()
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, chords)| chords.as_slice())
    }

    /// The action `press` triggers, if any.
    pub fn lookup(&self, press: &KeyPress<'_>) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(_, chords)| chords.iter().any(|c| c.matches(press)))
            .map(|(action, _)| *action)
    }

    /// The action other than `except` that already has a chord overlapping `chord`.
    pub fn conflict(&self, chord: &Chord, except: Action) -> Option<Action> {
        self.bindings
            .iter()
            .filter(|(a, _)| *a != except)
            .find(|(_, chords)| chords.iter().any(|c| c.overlaps(chord)))
            .map(|(action, _)| *action)
    }

    /// Adds `chord` to `action` (a chord it already has is not added twice); refused when another action has it.
    pub fn bind(&mut self, action: Action, chord: Chord) -> Result<(), Conflict> {
        if let Some(other) = self.conflict(&chord, action) {
            return Err(Conflict {
                chord: chord.text().to_owned(),
                other,
            });
        }
        if let Some((_, chords)) = self.bindings.iter_mut().find(|(a, _)| *a == action)
            && !chords.iter().any(|c| c.overlaps(&chord))
        {
            chords.push(chord);
        }
        Ok(())
    }

    /// Removes the chord with `text` from `action`; `false` when it had none.
    pub fn unbind(&mut self, action: Action, text: &str) -> bool {
        let Some((_, chords)) = self.bindings.iter_mut().find(|(a, _)| *a == action) else {
            return false;
        };
        let before = chords.len();
        chords.retain(|c| c.text() != text);
        chords.len() != before
    }

    /// Every pair of actions whose chords overlap (empty for a keymap built by [`Keymap::from_table`] and [`Keymap::bind`]).
    pub fn conflicts(&self) -> Vec<(Action, Action)> {
        let mut pairs = Vec::new();
        for (i, (a, chords_a)) in self.bindings.iter().enumerate() {
            for (b, chords_b) in self.bindings.iter().skip(i + 1) {
                if chords_a.iter().any(|x| chords_b.iter().any(|y| x.overlaps(y))) {
                    pairs.push((*a, *b));
                }
            }
        }
        pairs
    }

    /// The keys of `action` as its tooltip shows them: every chord, joined by " or " ("" when unbound).
    pub fn tooltip_keys(&self, action: Action) -> String {
        self.chords(action)
            .iter()
            .map(Chord::label)
            .collect::<Vec<_>>()
            .join(crate::ui::tooltip::keys::OR)
    }

    /// The first key of `action` as a menu row or a hint text shows it ("" when unbound).
    pub fn first_label(&self, action: Action) -> String {
        self.chords(action).first().map(Chord::label).unwrap_or_default()
    }
}
