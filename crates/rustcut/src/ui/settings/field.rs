//! The generic rows of the Settings window (M4.2): what a row shows and how a click, a choice, a typed text or a wheel
//! step changes `Settings`. Every row of the simple pages is one [`Field`] in the catalog (`catalog/`); the pages with
//! their own editors (target folders, profiles, hotkeys, About) have their own modules.

use rustcut_core::settings::Settings;

/// The control of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Switch,
    Choice(&'static [&'static str]),
    /// A whole number in `min..=max`; a wheel step or an arrow key moves it by `step`.
    Number {
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    },
    Text {
        placeholder: &'static str,
    },
    /// A number typed as text, drawn like [`Kind::Number`] (its width, its unit) without steps: an empty field or a
    /// decimal means something (the setter checks it; "No limit", "0.25").
    Amount {
        placeholder: &'static str,
        unit: &'static str,
    },
    /// A path typed or picked with a dialog (a folder or a file); empty = not set.
    Path {
        placeholder: &'static str,
        folder: bool,
    },
    /// A button with this text; it runs the row's key as an action (open a folder, test the connection).
    Button(&'static str),
    /// A color as `#rrggbb`: a swatch that opens the color picker (P8); typed text is still taken.
    Color,
}

/// A row's value as the row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Index(usize),
    Number(i64),
    Text(String),
    /// Buttons have none.
    None,
}

impl Value {
    pub fn as_bool(&self) -> Result<bool, String> {
        match self {
            Self::Bool(b) => Ok(*b),
            _ => Err(WRONG_VALUE.into()),
        }
    }

    pub fn as_index(&self) -> Result<usize, String> {
        match self {
            Self::Index(i) => Ok(*i),
            _ => Err(WRONG_VALUE.into()),
        }
    }

    pub fn as_number(&self) -> Result<i64, String> {
        match self {
            Self::Number(n) => Ok(*n),
            _ => Err(WRONG_VALUE.into()),
        }
    }

    pub fn as_text(&self) -> Result<&str, String> {
        match self {
            Self::Text(t) => Ok(t),
            _ => Err(WRONG_VALUE.into()),
        }
    }
}

/// Only a bug in the catalog gives a row another kind of value than its control makes.
pub const WRONG_VALUE: &str = "This value does not fit the option.";

/// What the user did to a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Toggle,
    Choose(usize),
    Type(String),
    Step(i32),
}

/// One option of a settings page.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// The option's id: its place in `config.toml` (`queue.workers`), or the action of a button.
    pub key: &'static str,
    pub label: &'static str,
    /// What it does and what it costs; shown under the label.
    pub help: &'static str,
    pub kind: Kind,
    /// `false`: RustCut uses the new value from the next start on (the row says so).
    pub live: bool,
    pub get: fn(&Settings) -> Value,
    /// Writes a value the control made; `Err` = the reason it was refused (the field stays open and marked).
    pub set: fn(&mut Settings, Value) -> Result<(), String>,
    /// Whether the row can be changed now (a sub-option of a switch that is off cannot).
    pub enabled: fn(&Settings) -> bool,
    /// A sub-option of the row above: drawn indented.
    pub indent: bool,
}

/// The `enabled` of a row that can always be changed.
pub const fn always(_: &Settings) -> bool {
    true
}

/// The `get`/`set` of a button.
pub const fn no_value(_: &Settings) -> Value {
    Value::None
}

#[expect(clippy::unnecessary_wraps, reason = "the signature of `Field::set`")]
pub fn no_set(_: &mut Settings, _: Value) -> Result<(), String> {
    Ok(())
}

impl Field {
    /// The same row, used from the next start on.
    pub const fn next_start(self) -> Self {
        Self { live: false, ..self }
    }

    /// The same row as a sub-option that can be changed only while `enabled` says so.
    pub const fn under(self, enabled: fn(&Settings) -> bool) -> Self {
        Self {
            enabled,
            indent: true,
            ..self
        }
    }

    /// A button row.
    pub const fn button(key: &'static str, label: &'static str, help: &'static str, text: &'static str) -> Self {
        Self {
            key,
            label,
            help,
            kind: Kind::Button(text),
            live: true,
            get: no_value,
            set: no_set,
            enabled: always,
            indent: false,
        }
    }

    /// Changes `settings` by `edit`. `Ok(true)` = the value changed, `Ok(false)` = it was already so (or the edit does not
    /// fit this control), `Err` = the typed text is refused with this reason.
    pub fn apply(&self, settings: &mut Settings, edit: Edit) -> Result<bool, String> {
        if !(self.enabled)(settings) {
            return Ok(false);
        }
        let before = (self.get)(settings);
        let value = match (self.kind, edit, &before) {
            (Kind::Switch, Edit::Toggle, Value::Bool(b)) => Value::Bool(!b),
            (Kind::Choice(items), Edit::Choose(index), _) if index < items.len() => Value::Index(index),
            (Kind::Number { min, max, unit, .. }, Edit::Type(text), _) => {
                Value::Number(parse_number(&text, min, max, unit)?)
            }
            (Kind::Number { min, max, step, .. }, Edit::Step(steps), Value::Number(n)) => {
                Value::Number(n.saturating_add(step.saturating_mul(i64::from(steps))).clamp(min, max))
            }
            (Kind::Text { .. } | Kind::Amount { .. } | Kind::Path { .. } | Kind::Color, Edit::Type(text), _) => {
                Value::Text(text.trim().to_owned())
            }
            _ => return Ok(false),
        };
        if value == before {
            return Ok(false);
        }
        (self.set)(settings, value)?;
        Ok((self.get)(settings) != before)
    }

    /// Puts the default value back.
    pub fn reset(&self, settings: &mut Settings) -> bool {
        let wanted = (self.get)(&Settings::default());
        if matches!(wanted, Value::None) || (self.get)(settings) == wanted {
            return false;
        }
        (self.set)(settings, wanted).is_ok()
    }
}

/// A typed whole number, with or without its unit (`"300 ms"`), within `min..=max`.
fn parse_number(text: &str, min: i64, max: i64, unit: &str) -> Result<i64, String> {
    let trimmed = text.trim();
    let digits = if unit.is_empty() {
        trimmed
    } else {
        trimmed.strip_suffix(unit).unwrap_or(trimmed).trim_end()
    };
    let n: i64 = digits
        .parse()
        .map_err(|_not_a_number| format!("Type a whole number from {min} to {max}."))?;
    if (min..=max).contains(&n) {
        Ok(n)
    } else {
        Err(format!("Type a whole number from {min} to {max}."))
    }
}

/// A switch row for the boolean at `path` (`switch!("key", "Label", "Help", general.single_instance)`).
macro_rules! switch {
    ($key:literal, $label:literal, $help:literal, $($path:ident).+) => {
        $crate::ui::settings::field::Field {
            key: $key,
            label: $label,
            help: $help,
            kind: $crate::ui::settings::field::Kind::Switch,
            live: true,
            get: |s| $crate::ui::settings::field::Value::Bool(s.$($path).+),
            set: |s, v| {
                s.$($path).+ = v.as_bool()?;
                Ok(())
            },
            enabled: $crate::ui::settings::field::always,
            indent: false,
        }
    };
}

/// A number row for the integer of type `$t` at `path`.
macro_rules! number {
    ($key:literal, $label:literal, $help:literal, $($path:ident).+ : $t:ty, $min:literal..=$max:literal, $step:literal, $unit:literal) => {
        $crate::ui::settings::field::Field {
            key: $key,
            label: $label,
            help: $help,
            kind: $crate::ui::settings::field::Kind::Number { min: $min, max: $max, step: $step, unit: $unit },
            live: true,
            get: |s| $crate::ui::settings::field::Value::Number(i64::from(s.$($path).+)),
            set: |s, v| {
                s.$($path).+ = <$t>::try_from(v.as_number()?).map_err(|_out_of_range| $crate::ui::settings::field::WRONG_VALUE.to_owned())?;
                Ok(())
            },
            enabled: $crate::ui::settings::field::always,
            indent: false,
        }
    };
}

/// A choice row for the value at `path`: the variants in the order of their labels.
macro_rules! choice {
    ($key:literal, $label:literal, $help:literal, $($path:ident).+, [$(($variant:expr, $text:literal)),+ $(,)?]) => {
        $crate::ui::settings::field::Field {
            key: $key,
            label: $label,
            help: $help,
            kind: $crate::ui::settings::field::Kind::Choice(&[$($text),+]),
            live: true,
            get: |s| {
                let all = [$($variant),+];
                $crate::ui::settings::field::Value::Index(all.iter().position(|v| *v == s.$($path).+).unwrap_or(0))
            },
            set: |s, v| {
                let all = [$($variant),+];
                s.$($path).+ = *all.get(v.as_index()?).ok_or_else(|| $crate::ui::settings::field::WRONG_VALUE.to_owned())?;
                Ok(())
            },
            enabled: $crate::ui::settings::field::always,
            indent: false,
        }
    };
}

/// A path row for the `Option<PathBuf>` at `path` (empty = `None`).
macro_rules! path_field {
    ($key:literal, $label:literal, $help:literal, $($path:ident).+, $placeholder:literal, folder: $folder:literal) => {
        $crate::ui::settings::field::Field {
            key: $key,
            label: $label,
            help: $help,
            kind: $crate::ui::settings::field::Kind::Path { placeholder: $placeholder, folder: $folder },
            live: true,
            get: |s| {
                $crate::ui::settings::field::Value::Text(
                    s.$($path).+.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                )
            },
            set: |s, v| {
                let text = v.as_text()?;
                s.$($path).+ = (!text.is_empty()).then(|| std::path::PathBuf::from(text));
                Ok(())
            },
            enabled: $crate::ui::settings::field::always,
            indent: false,
        }
    };
}

pub(crate) use {choice, number, path_field, switch};
