//! File-name templates in Jinja syntax (minijinja, the engine RustStart uses): `{{ name }}{% if counter %} - {{ counter | pad(2) }}{% endif %}`.
//! Undefined variables are errors, the engine runs with a fuel limit and has no file access.

use std::sync::LazyLock;

use minijinja::{Environment, ErrorKind, UndefinedBehavior, Value};
use serde::Serialize;

use super::variant::sanitize_stem;

/// PyCutRv's naming: the typed name, then ` - 01` when the counter is set.
pub const DEFAULT_TEMPLATE: &str = "{{ name }}{% if counter %} - {{ counter | pad(2) }}{% endif %}";

/// Variables a template can use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NameContext {
    /// The name typed in the export panel (defaults to the source file name without extension).
    pub name: String,
    /// The source file name without extension.
    pub source: String,
    /// The counter of the export panel; `None` (falsy in templates) when it is off.
    pub counter: Option<u32>,
    /// Local date and time of the save, `2026-10-23` and `14-05-09`.
    pub date: String,
    pub time: String,
    /// Name of the render profile.
    pub profile: String,
    /// Short codec name (`AV1`, `HEVC`, `H.264`).
    pub codec: String,
    /// Number of sections and the output length as `H-MM-SS`.
    pub sections: usize,
    pub duration: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("template error: {0}")]
    Template(String),
    #[error("the file name is empty")]
    Empty,
    #[error("`{0}` cannot be a file name")]
    Reserved(String),
}

static ENGINE: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.set_fuel(Some(20_000));
    env.add_filter("pad", pad);
    env
});

/// `{{ counter | pad(2) }}` = `07`: zero-padded to `width` digits (numbers) or left as it is (text).
fn pad(value: &Value, width: Option<usize>) -> String {
    let width = width.unwrap_or(2).min(12);
    match i64::try_from(value.clone()) {
        Ok(n) => format!("{n:0width$}"),
        Err(_) => value.to_string(),
    }
}

/// Renders the template and sanitizes the result into a file stem (no extension).
pub fn render_stem(template: &str, ctx: &NameContext) -> Result<String, NameError> {
    let raw = ENGINE
        .render_str(template, Value::from(minijinja::value::Serde(ctx)))
        .map_err(|e| NameError::Template(describe(&e)))?;
    sanitize_stem(&raw)
}

/// The top-level variables a template reads (for the settings page and its validation).
pub fn used_variables(template: &str) -> Result<Vec<String>, NameError> {
    let t = ENGINE
        .template_from_str(template)
        .map_err(|e| NameError::Template(describe(&e)))?;
    let mut names: Vec<String> = t.undeclared_variables(false).into_iter().collect();
    names.sort();
    Ok(names)
}

fn describe(e: &minijinja::Error) -> String {
    let detail = e.detail().unwrap_or("");
    match e.kind() {
        ErrorKind::UndefinedError => format!("unknown variable ({detail})"),
        ErrorKind::OutOfFuel => "the template is too complex".to_owned(),
        kind => format!("{kind}: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(counter: Option<u32>) -> NameContext {
        NameContext {
            name: "Holiday".into(),
            source: "VID_0001".into(),
            counter,
            date: "2026-10-23".into(),
            time: "14-05-09".into(),
            profile: "AV1 balanced".into(),
            codec: "AV1".into(),
            sections: 3,
            duration: "0-01-30".into(),
        }
    }

    #[test]
    fn default_template_matches_pycutrv() {
        assert_eq!(render_stem(DEFAULT_TEMPLATE, &ctx(None)).unwrap(), "Holiday");
        assert_eq!(render_stem(DEFAULT_TEMPLATE, &ctx(Some(1))).unwrap(), "Holiday - 01");
        assert_eq!(render_stem(DEFAULT_TEMPLATE, &ctx(Some(123))).unwrap(), "Holiday - 123");
    }

    #[test]
    fn other_variables_and_filters() {
        let t = "{{ date }} {{ source | lower }} [{{ codec }}] {{ counter | pad(3) }}";
        assert_eq!(render_stem(t, &ctx(Some(7))).unwrap(), "2026-10-23 vid_0001 [AV1] 007");
        assert_eq!(used_variables(t).unwrap(), vec!["codec", "counter", "date", "source"]);
    }

    #[test]
    fn errors_are_reported() {
        assert!(matches!(
            render_stem("{{ nmae }}", &ctx(None)),
            Err(NameError::Template(_))
        ));
        assert!(matches!(
            render_stem("{% if %}", &ctx(None)),
            Err(NameError::Template(_))
        ));
        assert_eq!(
            render_stem("{% if false %}x{% endif %}", &ctx(None)),
            Err(NameError::Empty)
        );
        let runaway = "{% for i in range(100000) %}{{ i }}{% endfor %}";
        assert!(matches!(render_stem(runaway, &ctx(None)), Err(NameError::Template(m)) if m.contains("complex")));
    }
}
