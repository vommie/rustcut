//! A plugin's setting values: defaults from its manifest, sanitizing, and the conversion between the TOML that
//! `config.toml` stores and the JSON a plugin reads.
//!
//! Stored values that the manifest no longer declares are kept in storage (an older or newer version of the plugin
//! may use them) but never passed to the plugin.

use serde_json::{Map, Value};

use crate::manifest::{Field, FieldKind, Manifest, schema_limits};

/// The values of a plugin's declared fields, with defaults for what is missing or unusable.
#[must_use]
pub fn effective(manifest: &Manifest, stored: &Map<String, Value>) -> Map<String, Value> {
    let fields: Vec<&Field> = manifest.fields().collect();
    sanitize_level(&fields, stored)
}

/// The default values of a plugin's declared fields.
#[must_use]
pub fn defaults(manifest: &Manifest) -> Map<String, Value> {
    effective(manifest, &Map::new())
}

/// One empty row of a table field: every column's default.
#[must_use]
pub fn new_row(columns: &[Field]) -> Value {
    let columns: Vec<&Field> = columns.iter().collect();
    Value::Object(sanitize_level(&columns, &Map::new()))
}

/// Whether a field is enabled, given the values on its level.
#[must_use]
pub fn enabled(field: &Field, level: &Map<String, Value>) -> bool {
    field
        .enabled_if
        .as_ref()
        .is_none_or(|cond| level.get(&cond.key) == Some(&cond.equals))
}

/// The value of one field, sanitized.
#[must_use]
pub fn sanitize(field: &Field, value: Option<&Value>) -> Value {
    match &field.kind {
        FieldKind::Switch { default } => Value::Bool(value.and_then(Value::as_bool).unwrap_or(*default)),
        FieldKind::Number {
            default,
            min,
            max,
            decimals,
            ..
        } => {
            let number = value
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .unwrap_or(*default);
            number_value(round(number.clamp(*min, *max), *decimals), *decimals)
        }
        FieldKind::Choice { options, default } => {
            let text = value
                .and_then(Value::as_str)
                .filter(|v| options.iter().any(|o| o.value == *v));
            Value::String(text.unwrap_or(default).to_owned())
        }
        FieldKind::Text { default, .. } | FieldKind::Path { default, .. } => {
            let text = value.and_then(Value::as_str).unwrap_or(default);
            Value::String(text.chars().take(schema_limits::MAX_TEXT).collect())
        }
        FieldKind::Target | FieldKind::Tag => value
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok())
            .map_or(Value::Null, Value::from),
        FieldKind::Rating { default } => value
            .and_then(Value::as_u64)
            .and_then(|stars| u8::try_from(stars).ok())
            .filter(|stars| *stars <= 5)
            .or(*default)
            .map_or(Value::Null, Value::from),
        FieldKind::Table { columns, .. } => {
            let columns: Vec<&Field> = columns.iter().collect();
            let rows = value.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            Value::Array(
                rows.iter()
                    .filter_map(Value::as_object)
                    .take(schema_limits::MAX_ROWS)
                    .map(|row| Value::Object(sanitize_level(&columns, row)))
                    .collect(),
            )
        }
    }
}

fn sanitize_level(fields: &[&Field], stored: &Map<String, Value>) -> Map<String, Value> {
    fields
        .iter()
        .map(|field| (field.key.clone(), sanitize(field, stored.get(&field.key))))
        .collect()
}

fn round(number: f64, decimals: u8) -> f64 {
    let factor = 10_f64.powi(i32::from(decimals));
    (number * factor).round() / factor
}

fn number_value(number: f64, decimals: u8) -> Value {
    if decimals == 0 {
        // Clamped to the field's range before, so the integer conversion only fails for ranges beyond i64.
        let whole = format!("{number:.0}").parse::<i64>().ok();
        if let Some(whole) = whole {
            return Value::from(whole);
        }
    }
    serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number)
}

/// The stored TOML table as JSON.
#[must_use]
pub fn from_toml(table: &toml::Table) -> Map<String, Value> {
    table
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), toml_to_json(value)?)))
        .collect()
}

/// The JSON values as a TOML table; `null` members are left out (TOML has no null).
#[must_use]
pub fn to_toml(values: &Map<String, Value>) -> toml::Table {
    values
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), json_to_toml(value)?)))
        .collect()
}

fn toml_to_json(value: &toml::Value) -> Option<Value> {
    Some(match value {
        toml::Value::String(text) => Value::String(text.clone()),
        toml::Value::Integer(n) => Value::from(*n),
        toml::Value::Float(n) => serde_json::Number::from_f64(*n).map(Value::Number)?,
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(items) => Value::Array(items.iter().filter_map(toml_to_json).collect()),
        toml::Value::Table(table) => Value::Object(from_toml(table)),
    })
}

fn json_to_toml(value: &Value) -> Option<toml::Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => n.as_i64().map_or_else(
            || toml::Value::Float(n.as_f64().unwrap_or_default()),
            toml::Value::Integer,
        ),
        Value::String(text) => toml::Value::String(text.clone()),
        Value::Array(items) => toml::Value::Array(items.iter().filter_map(json_to_toml).collect()),
        Value::Object(map) => toml::Value::Table(to_toml(map)),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn manifest() -> Manifest {
        Manifest::parse(
            r#"
            id = "demo"
            name = "Demo"
            version = "1.0.0"
            api = "1.0"
            [[cards]]
            title = "Main"
            [[cards.fields]]
            key = "on"
            label = "On"
            kind = "switch"
            default = true
            [[cards.fields]]
            key = "limit"
            label = "Limit"
            kind = "number"
            default = 5
            min = 1
            max = 10
            [[cards.fields]]
            key = "mode"
            label = "Mode"
            kind = "choice"
            default = "a"
            options = [{ value = "a", label = "A" }, { value = "b", label = "B" }]
            [[cards.fields]]
            key = "rules"
            label = "Rules"
            kind = "table"
            [[cards.fields.columns]]
            key = "source"
            label = "Source"
            kind = "path"
            [[cards.fields.columns]]
            key = "target"
            label = "Target"
            kind = "target"
            [[cards.fields.columns]]
            key = "stars"
            label = "Stars"
            kind = "rating"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn defaults_fill_every_declared_field() {
        let values = defaults(&manifest());
        assert_eq!(
            Value::Object(values),
            json!({"on": true, "limit": 5, "mode": "a", "rules": []})
        );
    }

    #[test]
    fn unusable_values_fall_back_and_numbers_are_clamped() {
        let stored = json!({"on": "yes", "limit": 99.6, "mode": "c", "extra": 1,
            "rules": [{"source": "/a", "target": 3, "stars": 9}, "junk", {"target": -1}]});
        let values = effective(&manifest(), stored.as_object().unwrap());
        assert_eq!(
            Value::Object(values),
            json!({"on": true, "limit": 10, "mode": "a", "rules": [
                {"source": "/a", "target": 3, "stars": null},
                {"source": "", "target": null, "stars": null}]})
        );
    }

    #[test]
    fn toml_round_trip_drops_nulls_only() {
        let values = json!({"on": false, "limit": 3, "rules": [{"source": "/a", "target": null}]});
        let table = to_toml(values.as_object().unwrap());
        assert_eq!(
            Value::Object(from_toml(&table)),
            json!({"on": false, "limit": 3, "rules": [{"source": "/a"}]})
        );
    }

    #[test]
    fn a_condition_enables_a_field() {
        let mut field = manifest().fields().next().unwrap().clone();
        field.enabled_if = Some(crate::manifest::Condition {
            key: "mode".into(),
            equals: json!("b"),
        });
        assert!(!enabled(&field, json!({"mode": "a"}).as_object().unwrap()));
        assert!(enabled(&field, json!({"mode": "b"}).as_object().unwrap()));
    }
}
