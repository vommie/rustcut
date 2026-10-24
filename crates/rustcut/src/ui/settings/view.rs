//! What the generic cards of a page show: the rows with their current values, the refused texts and the notes (pure; the
//! bridge turns them into Slint structs).

use std::collections::BTreeMap;

use rustcut_core::settings::Settings;

use super::catalog::{self, Card};
use super::field::{Field, Kind, Value};
use super::page::Page;

/// The control of a row as Slint draws it (`kind` in `SettingRowView`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RowKind {
    Switch = 0,
    Choice = 1,
    Number = 2,
    Text = 3,
    Folder = 4,
    File = 5,
    Button = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub key: &'static str,
    pub kind: RowKind,
    pub label: &'static str,
    pub description: String,
    pub checked: bool,
    pub items: Vec<&'static str>,
    pub index: usize,
    pub text: String,
    pub placeholder: &'static str,
    /// A number's unit, a button's text.
    pub suffix: &'static str,
    pub enabled: bool,
    pub indent: bool,
    /// Why the last typed text was refused (the field stays marked until a value is kept).
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub title: &'static str,
    pub description: String,
    pub rows: Vec<RowView>,
}

/// Facts a row's text needs that `Settings` does not hold (the folders behind the Open buttons).
pub trait Context {
    /// The text under a button's label (`None` = its help).
    fn button_text(&self, key: &str) -> Option<String>;
}

/// The cards of `page` with the values of `settings`; `errors` are the refused texts by key.
pub fn cards(page: Page, settings: &Settings, errors: &BTreeMap<String, String>, ctx: &dyn Context) -> Vec<CardView> {
    catalog::cards(page)
        .iter()
        .map(|card| card_view(card, settings, errors, ctx))
        .collect()
}

fn card_view(card: &Card, settings: &Settings, errors: &BTreeMap<String, String>, ctx: &dyn Context) -> CardView {
    let mut description = card.description.to_owned();
    if card.fields.iter().any(|f| f.key == "export.name_template")
        && let Some(example) = catalog::template_example(&settings.export.name_template)
    {
        description = format!("{description} Now: \"{example}\".");
    }
    CardView {
        title: card.title,
        description,
        rows: card.fields.iter().map(|f| row(f, settings, errors, ctx)).collect(),
    }
}

/// One row.
pub fn row(field: &Field, settings: &Settings, errors: &BTreeMap<String, String>, ctx: &dyn Context) -> RowView {
    let value = (field.get)(settings);
    let mut description = field.help.to_owned();
    if matches!(field.kind, Kind::Button(_))
        && let Some(text) = ctx.button_text(field.key)
    {
        description = text;
    }
    if !field.live {
        description.push_str(" Applies at the next start.");
    }
    let (kind, items, placeholder, suffix) = match field.kind {
        Kind::Switch => (RowKind::Switch, Vec::new(), "", ""),
        Kind::Choice(items) => (RowKind::Choice, items.to_vec(), "", ""),
        Kind::Number { unit, .. } => (RowKind::Number, Vec::new(), "", unit),
        Kind::Text { placeholder } => (RowKind::Text, Vec::new(), placeholder, ""),
        Kind::Path { placeholder, folder } => (
            if folder { RowKind::Folder } else { RowKind::File },
            Vec::new(),
            placeholder,
            "",
        ),
        Kind::Button(text) => (RowKind::Button, Vec::new(), "", text),
    };
    let (checked, index, text) = match value {
        Value::Bool(b) => (b, 0, String::new()),
        Value::Index(i) => (false, i, String::new()),
        Value::Number(n) => (false, 0, n.to_string()),
        Value::Text(t) => (false, 0, t),
        Value::None => (false, 0, String::new()),
    };
    RowView {
        key: field.key,
        kind,
        label: field.label,
        description,
        checked,
        items,
        index,
        text,
        placeholder,
        suffix,
        enabled: (field.enabled)(settings),
        indent: field.indent,
        error: errors.get(field.key).cloned().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Paths;

    impl Context for Paths {
        fn button_text(&self, key: &str) -> Option<String> {
            (key == "open.logs").then(|| "/home/x/.local/state/rustcut/logs".to_owned())
        }
    }

    #[test]
    fn rows_show_values_notes_errors_and_folders() {
        let mut settings = Settings::default();
        settings.queue.workers = 2;
        let mut errors = BTreeMap::new();
        errors.insert("queue.when_done_countdown_secs".to_owned(), "Type a whole number.".to_owned());
        let queue = cards(Page::Queue, &settings, &errors, &Paths);
        let workers = queue.first().and_then(|c| c.rows.first()).unwrap();
        assert_eq!((workers.kind, workers.text.as_str(), workers.enabled), (RowKind::Number, "2", true));
        let tray = cards(Page::Desktop, &settings, &errors, &Paths)
            .iter()
            .flat_map(|c| c.rows.clone())
            .find(|r| r.key == "desktop.tray.enabled")
            .unwrap();
        assert!(tray.description.ends_with("Applies at the next start."), "{}", tray.description);
        assert!(!workers.description.contains("next start"));
        let countdown = queue.get(1).and_then(|c| c.rows.get(1)).unwrap();
        assert_eq!(countdown.error, "Type a whole number.");

        let advanced = cards(Page::Advanced, &settings, &errors, &Paths);
        let logs = advanced
            .iter()
            .flat_map(|c| &c.rows)
            .find(|r| r.key == "open.logs")
            .unwrap();
        assert_eq!((logs.kind, logs.suffix), (RowKind::Button, "Open"));
        assert_eq!(logs.description, "/home/x/.local/state/rustcut/logs");

        settings.rustview.enabled = false;
        let rustview = cards(Page::RustView, &settings, &errors, &Paths);
        let binary = rustview.first().and_then(|c| c.rows.get(1)).unwrap();
        assert_eq!((binary.kind, binary.enabled, binary.indent), (RowKind::File, false, true));
        let export = cards(Page::Export, &settings, &errors, &Paths);
        assert!(export.first().unwrap().description.ends_with("Now: \"Holiday - 01\"."));
    }
}
