//! Changing a plugin's settings: options, table cells and rows, RustCut's own options of the plugin, and the checks
//! that run after every change (the plugin's own `check` plus RustCut's: tags and folders that no longer exist,
//! source folders missing on disk). Every change goes through the settings writer.

use std::rc::Weak;

use rustcut_core::plugins::{Field, FieldKind, Issue, Level, PathSelect, PluginCommand, values};
use serde_json::{Map, Value};

use super::page::{MEMORY_RANGE, TIME_RANGE, manifest};
use super::view::{self, Choices};
use super::{PluginKey, SettingsBridge};
use crate::PluginsUi;
use crate::ui::settings::field::Edit;

fn with<R: Default>(this: &Weak<SettingsBridge>, f: impl FnOnce(&SettingsBridge) -> R) -> R {
    this.upgrade().map(|b| f(&b)).unwrap_or_default()
}

fn index(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

pub(super) fn connect(this: &Weak<SettingsBridge>, ui: &PluginsUi<'_>) {
    let t = this.clone();
    ui.on_cell_text(move |key, r, c, text| {
        with(&t, |b| {
            b.plugin_cell(&key, r, c, CellEdit::Text(text.to_string())).is_ok()
        })
    });
    let t = this.clone();
    ui.on_cell_toggle(move |key, r, c| {
        with(&t, |b| b.plugin_cell(&key, r, c, CellEdit::Toggle).is_ok());
    });
    let t = this.clone();
    ui.on_cell_choose(move |key, r, c, i| {
        with(&t, |b| b.plugin_cell(&key, r, c, CellEdit::Choose(i)).is_ok());
    });
    let t = this.clone();
    ui.on_cell_rate(move |key, r, c, stars| {
        with(&t, |b| b.plugin_cell(&key, r, c, CellEdit::Rate(stars)).is_ok());
    });
    let t = this.clone();
    ui.on_cell_pick(move |key, r, c| {
        with(&t, |b| {
            if let (Some(key), Some(r), Some(c)) = (PluginKey::parse(&key), index(r), index(c)) {
                b.plugin_pick(key, Some((r, c)));
            }
        });
    });
    let t = this.clone();
    ui.on_row_add(move |key| with(&t, |b| b.plugin_rows(&key, RowEdit::Add)));
    let t = this.clone();
    ui.on_row_remove(move |key, r| with(&t, |b| b.plugin_rows(&key, RowEdit::Remove(r))));
    let t = this.clone();
    ui.on_row_move(move |key, r, delta| with(&t, |b| b.plugin_rows(&key, RowEdit::Move(r, delta))));
}

enum CellEdit {
    Text(String),
    Toggle,
    Choose(i32),
    Rate(i32),
}

enum RowEdit {
    Add,
    Remove(i32),
    Move(i32, i32),
    MoveTo(usize, usize),
}

/// The new value of `field` after `edit`, or why it is refused.
fn edited(field: &Field, old: &Value, edit: Edit, choices: &Choices) -> Result<Value, String> {
    match (&field.kind, edit) {
        (FieldKind::Switch { .. }, Edit::Toggle) => Ok(Value::Bool(!old.as_bool().unwrap_or_default())),
        (
            FieldKind::Number {
                min,
                max,
                step,
                decimals,
                ..
            },
            edit,
        ) => {
            let number = match edit {
                Edit::Type(text) => text
                    .trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .map_err(|err| format!("{} must be a number between {min} and {max} ({err})", field.label))?,
                Edit::Step(steps) => old.as_f64().unwrap_or_default() + f64::from(steps) * step,
                _ => return Err(String::new()),
            };
            if !(min..=max).contains(&&number) {
                return Err(format!("{} must lie between {min} and {max}", field.label));
            }
            let _ = decimals;
            Ok(values::sanitize(field, Some(&Value::from(number))))
        }
        (FieldKind::Choice { options, .. }, Edit::Choose(i)) => options
            .get(i)
            .map(|o| Value::String(o.value.clone()))
            .ok_or_else(String::new),
        (FieldKind::Text { .. } | FieldKind::Path { .. }, Edit::Type(text)) => Ok(Value::String(text)),
        (kind @ (FieldKind::Target | FieldKind::Tag | FieldKind::Rating { .. }), Edit::Choose(i)) => {
            choices.value_at(kind, i).ok_or_else(String::new)
        }
        _ => Err(String::new()),
    }
}

impl SettingsBridge {
    /// An option of a plugin page (`SettingsUi`'s callbacks with a `plugin:` key).
    pub(in crate::ui::settings) fn plugin_edit(&self, key: &PluginKey, edit: Edit) -> Result<(), String> {
        if let Some(host) = key.field.strip_prefix('#') {
            return self.plugin_host_edit(&key.id, host, edit);
        }
        let Some(info) = self.plugin_info(&key.id) else {
            return Ok(());
        };
        let Some(manifest) = manifest(&info) else { return Ok(()) };
        let Some(field) = manifest.fields().find(|f| f.key == key.field).cloned() else {
            return Ok(());
        };
        let settings = self.writer.current();
        let stored = settings.plugins.entry(&key.id).values;
        let current = values::effective(manifest, &stored);
        let old = current.get(&field.key).cloned().unwrap_or(Value::Null);
        let value = edited(&field, &old, edit, &self.plugin_choices(&settings));
        match value {
            Ok(value) => {
                self.store_value(&key.id, &field.key, value);
                Ok(())
            }
            Err(reason) if reason.is_empty() => Ok(()),
            Err(reason) => {
                crate::ui::settings::handlers::post_refused(
                    &PluginKey::text(&key.id, &key.field),
                    &reason,
                    std::time::Duration::ZERO,
                );
                Err(reason)
            }
        }
    }

    /// RustCut's own options of a plugin: its switch and its limits.
    fn plugin_host_edit(&self, id: &str, field: &str, edit: Edit) -> Result<(), String> {
        let entry = self.writer.current().plugins.entry(id);
        let step = |value: u32, range: &std::ops::RangeInclusive<u32>, by: i64, edit: Edit| -> Result<u32, String> {
            let next = match edit {
                Edit::Type(text) => text
                    .trim()
                    .parse::<i64>()
                    .map_err(|err| format!("A whole number between {} and {} ({err})", range.start(), range.end()))?,
                Edit::Step(steps) => i64::from(value) + i64::from(steps) * by,
                _ => return Ok(value),
            };
            let next = u32::try_from(next.max(0)).unwrap_or(u32::MAX);
            if range.contains(&next) {
                Ok(next)
            } else {
                Err(format!("Between {} and {}", range.start(), range.end()))
            }
        };
        let id = id.to_owned();
        match field {
            "enabled" => self.update(|s| {
                let entry = s.plugins.entries.entry(id).or_default();
                entry.enabled = !entry.enabled;
            }),
            "time" => {
                let ms = step(entry.time_limit_ms, &TIME_RANGE, 100, edit)?;
                self.update(|s| s.plugins.entries.entry(id).or_default().time_limit_ms = ms);
            }
            "memory" => {
                let mb = step(entry.memory_mb, &MEMORY_RANGE, 16, edit)?;
                self.update(|s| s.plugins.entries.entry(id).or_default().memory_mb = mb);
            }
            _ => {}
        }
        Ok(())
    }

    /// The buttons of RustCut's card on a plugin page.
    pub(in crate::ui::settings) fn plugin_action(&self, key: &PluginKey) {
        match key.field.as_str() {
            "#reload" => self.plugin_reload(&key.id),
            "#export" => self.plugin_export(&key.id),
            "#import" => self.plugin_import(&key.id),
            "#remove" => self.plugin_remove(&key.id),
            _ => {}
        }
    }

    /// Every setting of a plugin back to its default (the page header's held button); its switch and limits stay.
    pub(in crate::ui::settings) fn reset_plugin(&self, id: &str) {
        let id = id.to_owned();
        tracing::info!("[Plugins] {id}: settings back to their defaults");
        self.update(|s| s.plugins.entries.entry(id).or_default().values = Map::new());
    }

    /// The browse button of a path option (`cell` = a table's row and column).
    pub(in crate::ui::settings) fn plugin_pick(&self, key: PluginKey, cell: Option<(usize, usize)>) {
        let Some(info) = self.plugin_info(&key.id) else { return };
        let Some(field) = manifest(&info)
            .and_then(|m| m.fields().find(|f| f.key == key.field))
            .cloned()
        else {
            return;
        };
        let target = match (&field.kind, cell) {
            (FieldKind::Table { columns, .. }, Some((_, c))) => columns.get(c).cloned(),
            (_, None) => Some(field.clone()),
            _ => None,
        };
        let Some(target) = target else { return };
        let FieldKind::Path { select, .. } = target.kind else {
            return;
        };
        let mut dialog = rfd::AsyncFileDialog::new().set_title(format!("Choose: {}", target.label));
        if let Some(window) = self.window.borrow().as_ref() {
            use slint::ComponentHandle as _;
            dialog = dialog.set_parent(&window.window().window_handle());
        }
        let picked = crate::ui::settings::handlers::pick_path(dialog, select == PathSelect::Folder);
        let weak = Weak::clone(&self.me);
        let spawned = slint::spawn_local(async move {
            let Some(path) = picked.await else { return };
            let Some(b) = weak.upgrade() else { return };
            let text = path.display().to_string();
            let full = PluginKey::text(&key.id, &key.field);
            let _ = match cell {
                Some((r, c)) => b.plugin_cell(
                    &full,
                    i32::try_from(r).unwrap_or(-1),
                    i32::try_from(c).unwrap_or(-1),
                    CellEdit::Text(text),
                ),
                None => b.plugin_edit(&key, Edit::Type(text)),
            };
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Plugins] The event loop is gone; no file dialog");
        }
    }

    fn plugin_cell(&self, key: &str, r: i32, c: i32, edit: CellEdit) -> Result<(), String> {
        let (Some(key), Some(r), Some(c)) = (PluginKey::parse(key), index(r), index(c)) else {
            return Ok(());
        };
        let Some(info) = self.plugin_info(&key.id) else {
            return Ok(());
        };
        let Some(manifest) = manifest(&info) else { return Ok(()) };
        let Some(field) = manifest.fields().find(|f| f.key == key.field).cloned() else {
            return Ok(());
        };
        let FieldKind::Table { columns, .. } = &field.kind else {
            return Ok(());
        };
        let Some(column) = columns.get(c) else { return Ok(()) };
        let settings = self.writer.current();
        let current = values::effective(manifest, &settings.plugins.entry(&key.id).values);
        let mut rows = current
            .get(&field.key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(row) = rows.get_mut(r).and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let old = row.get(&column.key).cloned().unwrap_or(Value::Null);
        let edit = match edit {
            CellEdit::Text(text) => Edit::Type(text),
            CellEdit::Toggle => Edit::Toggle,
            CellEdit::Choose(i) => Edit::Choose(index(i).unwrap_or(0)),
            // A click on the chosen rating clears it.
            CellEdit::Rate(stars) => {
                let value = if old.as_i64() == Some(i64::from(stars)) {
                    Value::Null
                } else {
                    Value::from(stars.clamp(0, 5))
                };
                row.insert(column.key.clone(), value);
                self.store_value(&key.id, &field.key, Value::Array(rows));
                return Ok(());
            }
        };
        match edited(column, &old, edit, &self.plugin_choices(&settings)) {
            Ok(value) => {
                row.insert(column.key.clone(), value);
                self.store_value(&key.id, &field.key, Value::Array(rows));
                Ok(())
            }
            Err(reason) if reason.is_empty() => Ok(()),
            Err(reason) => {
                crate::ui::notices::post(crate::ui::notices::Notice::new(
                    crate::ui::notices::Kind::ValueInvalid,
                    reason.clone(),
                ));
                Err(reason)
            }
        }
    }

    fn plugin_rows(&self, key: &str, edit: RowEdit) {
        let Some(key) = PluginKey::parse(key) else { return };
        let Some(info) = self.plugin_info(&key.id) else { return };
        let Some(manifest) = manifest(&info) else { return };
        let Some(field) = manifest.fields().find(|f| f.key == key.field).cloned() else {
            return;
        };
        let FieldKind::Table { columns, .. } = &field.kind else {
            return;
        };
        let current = values::effective(manifest, &self.writer.current().plugins.entry(&key.id).values);
        let mut rows = current
            .get(&field.key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match edit {
            RowEdit::Add => rows.push(values::new_row(columns)),
            RowEdit::Remove(r) => {
                if let Some(r) = index(r).filter(|r| *r < rows.len()) {
                    rows.remove(r);
                }
            }
            RowEdit::MoveTo(from, to) => {
                if from < rows.len() && to < rows.len() {
                    let row = rows.remove(from);
                    rows.insert(to, row);
                }
            }
            RowEdit::Move(r, delta) => {
                let from = index(r);
                let to = from.and_then(|f| if delta < 0 { f.checked_sub(1) } else { Some(f + 1) });
                if let (Some(from), Some(to)) = (from, to)
                    && from < rows.len()
                    && to < rows.len()
                {
                    rows.swap(from, to);
                }
            }
        }
        self.store_value(&key.id, &field.key, Value::Array(rows));
    }

    /// A table's rows as the page shows them.
    pub(super) fn plugin_table_rows(&self, key: &PluginKey) -> Vec<Value> {
        let Some(info) = self.plugin_info(&key.id) else {
            return Vec::new();
        };
        let Some(manifest) = manifest(&info) else {
            return Vec::new();
        };
        let current = values::effective(manifest, &self.writer.current().plugins.entry(&key.id).values);
        current
            .get(&key.field)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    /// A dropped row of a table.
    pub(super) fn plugin_rows_move_to(&self, key: &str, from: usize, to: usize) {
        self.plugin_rows(key, RowEdit::MoveTo(from, to));
    }

    /// Writes one value of a plugin (the other stored values, also unknown ones, stay) and checks again.
    fn store_value(&self, id: &str, key: &str, value: Value) {
        let (id_owned, key_owned) = (id.to_owned(), key.to_owned());
        self.update(|s| {
            s.plugins
                .entries
                .entry(id_owned)
                .or_default()
                .values
                .insert(key_owned, value);
        });
        tracing::info!("[Plugins] {id}: setting {key} changed");
        self.request_plugin_check(id);
    }

    /// Checks a plugin's values: its own `check`, RustCut's references and folders on disk; the page shows the
    /// findings when both answered.
    pub(in crate::ui::settings) fn request_plugin_check(&self, id: &str) {
        let Some(plugins) = self.plugins() else { return };
        let Some(info) = self.plugin_info(id) else { return };
        let Some(manifest) = info.manifest else { return };
        let settings = self.writer.current();
        let stored = settings.plugins.entry(id).values;
        let current = values::effective(&manifest, &stored);
        let fields: Vec<&Field> = manifest.fields().collect();
        let mut issues = view::missing_refs(&fields, &current, &self.plugin_choices(&settings));
        let paths = folder_paths(&fields, &current);
        let serial = self.plugin_pages.check_serial.get() + 1;
        self.plugin_pages.check_serial.set(serial);
        let handle = plugins.service().handle.clone();
        let id_owned = id.to_owned();
        let asking = self.runtime.spawn(async move {
            let own = handle
                .request(|reply| PluginCommand::Check {
                    id: id_owned,
                    values: stored,
                    reply,
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            let missing = tokio::task::spawn_blocking(move || missing_folders(paths))
                .await
                .unwrap_or_default();
            (own, missing)
        });
        let weak = Weak::clone(&self.me);
        let id = id.to_owned();
        let spawned = slint::spawn_local(async move {
            let Ok((own, missing)) = asking.await else { return };
            let Some(b) = weak.upgrade() else { return };
            if b.plugin_pages.check_serial.get() != serial {
                return;
            }
            issues.extend(own);
            issues.extend(missing);
            b.plugin_pages.issues.borrow_mut().insert(id, issues);
            b.publish();
        });
        if let Err(e) = spawned {
            tracing::debug!(error = %e, "[Plugins] The event loop is gone");
        }
    }
}

/// The folder paths of a plugin's values, with where they are (field, row, the column's key for tables).
fn folder_paths(fields: &[&Field], values: &Map<String, Value>) -> Vec<(String, Option<u32>, String, String)> {
    let home = directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_default();
    let expand = |text: &str| match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", home.display()),
        _ => text.to_owned(),
    };
    let mut out = Vec::new();
    for field in fields {
        match &field.kind {
            FieldKind::Path {
                select: PathSelect::Folder,
                ..
            } => {
                if let Some(text) = values
                    .get(&field.key)
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    out.push((field.key.clone(), None, field.label.clone(), expand(text)));
                }
            }
            FieldKind::Table { columns, .. } => {
                let rows = values
                    .get(&field.key)
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for (r, row) in rows.iter().enumerate() {
                    for column in columns {
                        let FieldKind::Path {
                            select: PathSelect::Folder,
                            ..
                        } = column.kind
                        else {
                            continue;
                        };
                        if let Some(text) = row
                            .get(&column.key)
                            .and_then(Value::as_str)
                            .filter(|t| !t.trim().is_empty())
                        {
                            out.push((
                                format!("{}.{}", field.key, column.key),
                                u32::try_from(r).ok(),
                                column.label.clone(),
                                expand(text),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a text looks like a pattern (a regular expression or a glob) rather than a folder: then it is not checked.
fn looks_like_pattern(text: &str) -> bool {
    text.chars().any(|c| "^$*+?()[]{}|\\".contains(c))
}

/// The folders that do not exist on disk (runtime thread), as findings.
fn missing_folders(paths: Vec<(String, Option<u32>, String, String)>) -> Vec<Issue> {
    paths
        .into_iter()
        .filter(|(_, _, _, path)| !looks_like_pattern(path) && !std::path::Path::new(path).is_dir())
        .map(|(field, row, label, path)| Issue {
            field,
            row,
            level: Level::Warning,
            message: format!("{label}: the folder {path} does not exist (yet), so it matches nothing until it does."),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_not_checked_as_folders() {
        assert!(looks_like_pattern("^/mnt/usb[0-9]+/"));
        assert!(looks_like_pattern("/videos/*"));
        assert!(!looks_like_pattern("/home/me/Videos/Holiday 2026"));
    }

    #[test]
    fn numbers_are_checked_and_stepped() {
        let field: Field = toml::from_str(
            "key = \"n\"\nlabel = \"N\"\nkind = \"number\"\ndefault = 5.0\nmin = 1.0\nmax = 10.0\nstep = 2.0\n",
        )
        .unwrap();
        let choices = Choices::default();
        assert_eq!(
            edited(&field, &Value::from(5), Edit::Step(2), &choices),
            Ok(Value::from(9))
        );
        assert!(edited(&field, &Value::from(5), Edit::Type("11".into()), &choices).is_err());
        assert_eq!(
            edited(&field, &Value::from(5), Edit::Type("2,0".into()), &choices),
            Ok(Value::from(2))
        );
    }
}
