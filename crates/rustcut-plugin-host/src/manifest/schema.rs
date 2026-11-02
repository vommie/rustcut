//! The settings a plugin declares in `plugin.toml`: cards of fields that RustCut draws with its own controls.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// A card on the plugin's Settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub fields: Vec<Field>,
}

/// One setting (or one column of a table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// Lower case letters, digits and underscores: `show_messages`. The member's name in the settings JSON.
    pub key: String,
    pub label: String,
    /// The tooltip: what it does, with an example.
    #[serde(default)]
    pub help: String,
    /// Shown dimmed and ignored unless another field (on the same level) has a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_if: Option<Condition>,
    #[serde(flatten)]
    pub kind: FieldKind,
}

/// `enabled_if = { key = "when_no_match", equals = "default" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub key: String,
    pub equals: serde_json::Value,
}

/// What a field holds and how RustCut draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FieldKind {
    /// On or off (JSON `true`/`false`).
    Switch {
        #[serde(default)]
        default: bool,
    },
    /// A number between `min` and `max` (JSON number).
    Number {
        default: f64,
        min: f64,
        max: f64,
        #[serde(default = "one")]
        step: f64,
        /// Digits after the point; 0 = whole numbers.
        #[serde(default)]
        decimals: u8,
        #[serde(default)]
        unit: String,
    },
    /// One of a few values (JSON string). Up to three short options are drawn as segments.
    Choice {
        options: Vec<ChoiceOption>,
        default: String,
    },
    /// A line of text (JSON string).
    Text {
        #[serde(default)]
        default: String,
        #[serde(default)]
        placeholder: String,
    },
    /// A folder or file on disk (JSON string), with a button that opens the file dialog.
    Path {
        #[serde(default)]
        default: String,
        #[serde(default)]
        select: PathSelect,
        #[serde(default)]
        placeholder: String,
    },
    /// A folder of Settings > Target Folders (JSON number: the folder's id, or `null`).
    Target,
    /// A tag of RustView's tag tree (JSON number: the tag's id, or `null`).
    Tag,
    /// 0 to 5 stars (JSON number, or `null`).
    Rating {
        #[serde(default)]
        default: Option<u8>,
    },
    /// Rows of the same columns (JSON array of objects), for rules.
    Table {
        columns: Vec<Field>,
        /// The add button's text: "Add rule".
        #[serde(default)]
        add_label: String,
        /// Shown while the table is empty.
        #[serde(default)]
        empty: String,
        /// The user may reorder the rows by drag & drop: RustCut adds a grip to each row and does the rest (the
        /// plugin only gets the rows in their new order).
        #[serde(default)]
        reorder_drag: bool,
        /// Move-up and move-down buttons on each row (beside the grip or instead of it), also done by RustCut.
        #[serde(default)]
        reorder_buttons: bool,
    },
}

fn one() -> f64 {
    1.0
}

/// What a path field's button picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathSelect {
    #[default]
    Folder,
    File,
}

/// One value of a choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub help: String,
}

/// The most rows a table keeps.
pub const MAX_ROWS: usize = 500;

/// The longest text a text or path field keeps.
pub const MAX_TEXT: usize = 4096;

pub(super) fn validate(cards: &[Card]) -> Result<(), String> {
    let fields: Vec<&Field> = cards.iter().flat_map(|card| card.fields.iter()).collect();
    for card in cards {
        if card.title.trim().is_empty() {
            return Err("every card needs a title".to_owned());
        }
    }
    validate_level(&fields, false)
}

fn validate_level(fields: &[&Field], in_table: bool) -> Result<(), String> {
    let mut keys = HashSet::new();
    for field in fields {
        if !valid_key(&field.key) {
            return Err(format!(
                "key \"{}\" must be lower case letters, digits and underscores",
                field.key
            ));
        }
        if !keys.insert(field.key.as_str()) {
            return Err(format!("key \"{}\" is used twice", field.key));
        }
        if field.label.trim().is_empty() {
            return Err(format!("field \"{}\" needs a label", field.key));
        }
        validate_kind(field, in_table)?;
    }
    for field in fields {
        if let Some(cond) = &field.enabled_if
            && !keys.contains(cond.key.as_str())
        {
            return Err(format!(
                "field \"{}\": enabled_if names the unknown key \"{}\"",
                field.key, cond.key
            ));
        }
    }
    Ok(())
}

fn validate_kind(field: &Field, in_table: bool) -> Result<(), String> {
    let key = &field.key;
    match &field.kind {
        FieldKind::Number {
            default,
            min,
            max,
            step,
            decimals,
            ..
        } => {
            if !(min.is_finite() && max.is_finite() && min < max) {
                return Err(format!("number \"{key}\": min must be below max"));
            }
            if !(min..=max).contains(&default) {
                return Err(format!("number \"{key}\": default must lie between min and max"));
            }
            if !(step.is_finite() && *step > 0.0) || *decimals > 6 {
                return Err(format!(
                    "number \"{key}\": step must be positive and decimals at most 6"
                ));
            }
        }
        FieldKind::Choice { options, default } => {
            if options.is_empty() || options.len() > 12 {
                return Err(format!("choice \"{key}\" needs 1 to 12 options"));
            }
            let mut values = HashSet::new();
            if !options.iter().all(|o| values.insert(o.value.as_str())) {
                return Err(format!("choice \"{key}\" has an option twice"));
            }
            if !values.contains(default.as_str()) {
                return Err(format!("choice \"{key}\": default \"{default}\" is not an option"));
            }
        }
        FieldKind::Rating { default } => {
            if default.is_some_and(|stars| stars > 5) {
                return Err(format!("rating \"{key}\": default must be 0 to 5"));
            }
        }
        FieldKind::Table { columns, .. } => {
            if in_table {
                return Err(format!("table \"{key}\" cannot be inside a table"));
            }
            if columns.is_empty() || columns.len() > 8 {
                return Err(format!("table \"{key}\" needs 1 to 8 columns"));
            }
            let columns: Vec<&Field> = columns.iter().collect();
            validate_level(&columns, true)?;
        }
        FieldKind::Switch { .. }
        | FieldKind::Text { .. }
        | FieldKind::Path { .. }
        | FieldKind::Target
        | FieldKind::Tag => {}
    }
    Ok(())
}

fn valid_key(key: &str) -> bool {
    (1..=40).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
