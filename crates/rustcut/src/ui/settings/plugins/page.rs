//! A plugin's page: RustCut's card about the plugin (switch, limits, facts, buttons), then the plugin's own cards
//! with their options and tables, every value with what the checks found about it.

use rustcut_core::plugins::{Field, FieldKind, Issue, Level, Manifest, PathSelect, PluginInfo, Source, values};
use rustcut_core::settings::{PLUGIN_MEMORY_MB, PLUGIN_TIME_LIMIT_MS, PluginEntry, Settings};
use serde_json::{Map, Value};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use super::view::{self, Choices};
use super::{PluginKey, SettingsBridge};
use crate::{
    IconId, PluginBlockView, PluginCardView, PluginCellView, PluginColumnView, PluginTableRowView, PluginsUi,
    SettingRowView, SettingsWindow,
};

/// A row of the page as Slint shows it (`SettingRowView`'s kinds: 0 switch, 1 choice, 2 number, 3 text, 4 folder, 5
/// file, 6 button).
fn row(key: String, kind: i32, label: &str, help: &str) -> SettingRowView {
    SettingRowView {
        key: key.into(),
        kind,
        label: label.into(),
        description: help.into(),
        enabled: true,
        items: ModelRc::default(),
        item_icons: ModelRc::default(),
        ..SettingRowView::default()
    }
}

fn texts(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.iter().map(|i| SharedString::from(i.as_str())).collect::<Vec<_>>(),
    ))
}

fn block(row: SettingRowView) -> PluginBlockView {
    PluginBlockView {
        table: false,
        row,
        ..PluginBlockView::default()
    }
}

fn button(id: &str, field: &str, label: &str, help: &str, icon: IconId, hold: bool) -> PluginBlockView {
    let mut r = row(PluginKey::text(id, field), 6, label, help);
    r.icon = icon;
    r.hold = hold;
    block(r)
}

/// A number as the field shows it.
pub(super) fn number_text(value: &Value, decimals: u8) -> String {
    let n = value.as_f64().unwrap_or_default();
    format!("{n:.prec$}", prec = usize::from(decimals))
}

/// What the checks found about one field (and row), joined; the worst level (0 none, 1 warning, 2 error).
fn found(issues: &[Issue], field: &str, row: Option<u32>) -> (String, i32) {
    let hits: Vec<&Issue> = issues.iter().filter(|i| i.field == field && i.row == row).collect();
    let level = hits
        .iter()
        .map(|i| if i.level == Level::Error { 2 } else { 1 })
        .max()
        .unwrap_or(0);
    let text = hits.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("\n");
    (text, level)
}

impl SettingsBridge {
    pub(in crate::ui::settings) fn publish_plugin_page(&self, window: &SettingsWindow, id: &str, settings: &Settings) {
        // A drag of a table's rows keeps the rows it holds; the page shows the new state when it ends.
        if self.plugin_pages.drag.borrow().is_some() {
            return;
        }
        let ui = window.global::<PluginsUi<'_>>();
        let Some(info) = self.plugin_info(id) else {
            ui.set_cards(ModelRc::default());
            return;
        };
        if !self.plugin_pages.issues.borrow().contains_key(id) {
            self.plugin_pages.issues.borrow_mut().insert(id.to_owned(), Vec::new());
            self.request_plugin_check(id);
        }
        let expanded = self.plugin_pages.expanded.borrow().clone();
        ui.set_current(view::item(&info, settings, &expanded, self.plugin_icon(&info)));
        let entry = settings.plugins.entry(id);
        let mut cards = vec![Self::host_card(&info, &entry)];
        if let Some(manifest) = info.manifest.as_deref() {
            let values = values::effective(manifest, &entry.values);
            let choices = self.plugin_choices(settings);
            let issues = self.plugin_pages.issues.borrow().get(id).cloned().unwrap_or_default();
            cards.extend(manifest.cards.iter().map(|card| {
                PluginCardView {
                    title: card.title.as_str().into(),
                    description: card.description.as_str().into(),
                    blocks: ModelRc::new(VecModel::from(
                        card.fields
                            .iter()
                            .map(|field| field_block(id, field, &values, &choices, &issues))
                            .collect::<Vec<_>>(),
                    )),
                }
            }));
        }
        ui.set_cards(ModelRc::new(VecModel::from(cards)));
    }

    /// The choices of target folder and tag fields: the folder tree and RustView's tags.
    pub(super) fn plugin_choices(&self, settings: &Settings) -> Choices {
        let tags = self.hooks.borrow().tag_paths.as_ref().and_then(|f| f());
        Choices::new(&settings.export.folders, tags)
    }

    /// RustCut's card: the plugin's switch, its limits, its facts and what can be done with it.
    fn host_card(info: &PluginInfo, entry: &PluginEntry) -> PluginCardView {
        let id = info.id.as_str();
        let facts = view::details(info)
            .into_iter()
            .filter(|(label, _)| label != "Description")
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut enabled = row(
            PluginKey::text(id, "#enabled"),
            0,
            "Run this plugin",
            "Off: it gets no events; its settings stay.",
        );
        enabled.checked = entry.enabled;
        let mut time = row(
            PluginKey::text(id, "#time"),
            2,
            "Time limit",
            "How long one reaction may take before RustCut stops it and counts a failure (3 in a row pause the plugin).",
        );
        time.text = entry.time_limit_ms.to_string().into();
        time.suffix = "ms".into();
        let mut memory = row(
            PluginKey::text(id, "#memory"),
            2,
            "Memory limit",
            "How much memory the plugin may use; more stops its call and counts a failure.",
        );
        memory.text = entry.memory_mb.to_string().into();
        memory.suffix = "MB".into();
        let dev = info.source == Source::Development;
        let blocks = vec![
            block(enabled),
            block(time),
            block(memory),
            button(
                id,
                "#reload",
                "Reload",
                "Reads the plugin again (also resumes it after a pause).",
                IconId::Reload,
                false,
            ),
            button(
                id,
                "#export",
                "Save settings",
                "Saves this plugin's settings to a file (for another computer, or as a copy).",
                IconId::SettingsExport,
                false,
            ),
            button(
                id,
                "#import",
                "Read settings",
                "Reads this plugin's settings from a file saved before.",
                IconId::SettingsImport,
                false,
            ),
            button(
                id,
                "#remove",
                if dev { "Disconnect" } else { "Remove" },
                if dev {
                    "Disconnects the development folder; its files stay."
                } else {
                    "Removes the plugin; its settings stay for a reinstall."
                },
                IconId::JobRemove,
                true,
            ),
        ];
        PluginCardView {
            title: "Plugin".into(),
            description: facts.into(),
            blocks: ModelRc::new(VecModel::from(blocks)),
        }
    }
}

/// One field of a plugin card: an option row, or a table.
fn field_block(
    id: &str,
    field: &Field,
    values: &Map<String, Value>,
    choices: &Choices,
    issues: &[Issue],
) -> PluginBlockView {
    let value = values.get(&field.key).cloned().unwrap_or(Value::Null);
    let key = PluginKey::text(id, &field.key);
    let enabled = values::enabled(field, values);
    if let FieldKind::Table {
        columns,
        add_label,
        empty,
        reorder_drag,
        reorder_buttons,
    } = &field.kind
    {
        let rows = value.as_array().cloned().unwrap_or_default();
        let row_views: Vec<PluginTableRowView> = rows
            .iter()
            .enumerate()
            .map(|(r, row_value)| {
                let row_index = u32::try_from(r).ok();
                let level = row_value.as_object().cloned().unwrap_or_default();
                let cells: Vec<PluginCellView> = columns
                    .iter()
                    .map(|column| {
                        let cell_value = level.get(&column.key).cloned().unwrap_or(Value::Null);
                        let mut cell = cell(column, &cell_value, choices);
                        cell.enabled = enabled && values::enabled(column, &level);
                        cell.issue = found(issues, &format!("{}.{}", field.key, column.key), row_index)
                            .0
                            .into();
                        cell
                    })
                    .collect();
                let (issue, issue_level) = found(issues, &field.key, row_index);
                PluginTableRowView {
                    cells: ModelRc::new(VecModel::from(cells)),
                    issue: issue.into(),
                    issue_level,
                }
            })
            .collect();
        let column_views: Vec<PluginColumnView> = columns
            .iter()
            .map(|c| {
                let (fixed, weight) = column_width(&c.kind);
                PluginColumnView {
                    label: c.label.as_str().into(),
                    help: c.help.as_str().into(),
                    kind: cell_kind(&c.kind),
                    fixed,
                    weight,
                    marked: false,
                }
            })
            .collect();
        let mut column_views = column_views;
        for (c, view) in column_views.iter_mut().enumerate() {
            let key = columns
                .get(c)
                .map(|col| format!("{}.{}", field.key, col.key))
                .unwrap_or_default();
            view.marked = issues.iter().any(|i| i.field == key && i.row.is_some());
        }
        let row_marked = issues.iter().any(|i| i.field == field.key && i.row.is_some());
        let fixed_total = column_views.iter().map(|c| c.fixed).sum();
        let weight_total = column_views.iter().map(|c| c.weight).sum();
        return PluginBlockView {
            table: true,
            key: key.into(),
            label: field.label.as_str().into(),
            help: field.help.as_str().into(),
            columns: ModelRc::new(VecModel::from(column_views)),
            rows: ModelRc::new(VecModel::from(row_views)),
            add_label: add_label.as_str().into(),
            empty: empty.as_str().into(),
            fixed_total,
            weight_total,
            row_marked,
            reorder_drag: *reorder_drag,
            reorder_buttons: *reorder_buttons,
            issue: found(issues, &field.key, None).0.into(),
            ..PluginBlockView::default()
        };
    }
    let mut r = row(key, 0, &field.label, &field.help);
    r.enabled = enabled;
    match &field.kind {
        FieldKind::Switch { .. } => r.checked = value.as_bool().unwrap_or_default(),
        FieldKind::Number { decimals, unit, .. } => {
            r.kind = 2;
            r.text = number_text(&value, *decimals).into();
            r.suffix = unit.as_str().into();
        }
        FieldKind::Choice { options, .. } => {
            let items: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
            r.kind = 1;
            r.index = i32::try_from(
                options
                    .iter()
                    .position(|o| value.as_str() == Some(o.value.as_str()))
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            r.segmented = crate::util::choice::segmented(&items);
            r.items = texts(&items);
        }
        FieldKind::Text { placeholder, .. } => {
            r.kind = 3;
            r.text = value.as_str().unwrap_or_default().into();
            r.placeholder = placeholder.as_str().into();
        }
        FieldKind::Path {
            select, placeholder, ..
        } => {
            r.kind = if *select == PathSelect::Folder { 4 } else { 5 };
            r.text = value.as_str().unwrap_or_default().into();
            r.placeholder = placeholder.as_str().into();
        }
        kind @ (FieldKind::Target | FieldKind::Tag | FieldKind::Rating { .. }) => {
            let (items, index) = choices.items(kind, &value);
            r.kind = 1;
            r.index = i32::try_from(index).unwrap_or(0);
            r.searchable = !matches!(kind, FieldKind::Rating { .. });
            r.items = texts(&items);
        }
        FieldKind::Table { .. } => {}
    }
    let (issue, level) = found(issues, &field.key, None);
    if level > 0 {
        r.status = 3;
        r.status_text = issue.into();
    }
    block(r)
}

/// A column's fixed width in logical pixels, or 0 and its share of the free width: folders and texts get the most.
fn column_width(kind: &FieldKind) -> (f32, f32) {
    match kind {
        FieldKind::Switch { .. } => (72.0, 0.0),
        FieldKind::Rating { .. } => (150.0, 0.0),
        FieldKind::Path { .. } => (0.0, 3.0),
        FieldKind::Text { .. } | FieldKind::Target | FieldKind::Tag => (0.0, 2.0),
        FieldKind::Number { .. } | FieldKind::Table { .. } => (0.0, 1.0),
        FieldKind::Choice { options, .. } => (0.0, if options.len() <= 3 { 1.2 } else { 1.6 }),
    }
}

/// The cell kind of a column (0 text, 1 folder path, 2 switch, 3 choice, 4 target folder, 5 tag, 6 rating).
fn cell_kind(kind: &FieldKind) -> i32 {
    match kind {
        FieldKind::Text { .. } | FieldKind::Number { .. } | FieldKind::Table { .. } => 0,
        FieldKind::Path { .. } => 1,
        FieldKind::Switch { .. } => 2,
        FieldKind::Choice { .. } => 3,
        FieldKind::Target => 4,
        FieldKind::Tag => 5,
        FieldKind::Rating { .. } => 6,
    }
}

fn cell(column: &Field, value: &Value, choices: &Choices) -> PluginCellView {
    let mut cell = PluginCellView {
        kind: cell_kind(&column.kind),
        enabled: true,
        stars: -1,
        ..PluginCellView::default()
    };
    match &column.kind {
        FieldKind::Text { placeholder, .. } | FieldKind::Path { placeholder, .. } => {
            cell.text = value.as_str().unwrap_or_default().into();
            cell.placeholder = placeholder.as_str().into();
        }
        FieldKind::Number { decimals, .. } => cell.text = number_text(value, *decimals).into(),
        FieldKind::Switch { .. } => cell.checked = value.as_bool().unwrap_or_default(),
        FieldKind::Choice { options, .. } => {
            let items: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
            cell.index = i32::try_from(
                options
                    .iter()
                    .position(|o| value.as_str() == Some(o.value.as_str()))
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            cell.items = texts(&items);
        }
        kind @ (FieldKind::Target | FieldKind::Tag) => {
            let (items, index) = choices.items(kind, value);
            cell.index = i32::try_from(index).unwrap_or(0);
            cell.items = texts(&items);
        }
        FieldKind::Rating { .. } => cell.stars = value.as_i64().and_then(|n| i32::try_from(n).ok()).unwrap_or(-1),
        FieldKind::Table { .. } => {}
    }
    cell
}

/// The limits' ranges, for the edits.
pub(super) const TIME_RANGE: std::ops::RangeInclusive<u32> = PLUGIN_TIME_LIMIT_MS;
pub(super) const MEMORY_RANGE: std::ops::RangeInclusive<u32> = PLUGIN_MEMORY_MB;

/// The manifest of a plugin, if it could be read.
pub(super) fn manifest(info: &PluginInfo) -> Option<&Manifest> {
    info.manifest.as_deref()
}
