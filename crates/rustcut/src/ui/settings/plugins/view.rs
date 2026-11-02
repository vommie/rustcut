//! What the plugin pages show, without Slint state: a plugin's row in the list, the choices of target folder and tag
//! fields, and RustCut's own checks of a plugin's values (tags and target folders that no longer exist).

use std::collections::HashSet;

use rustcut_core::folders::{FolderNode, FolderTree, SOURCE_NAME};
use rustcut_core::plugins::{Field, FieldKind, Issue, Level, PluginInfo, PluginState, Source};
use rustcut_core::settings::Settings;
use serde_json::{Map, Value};
use slint::{ModelRc, VecModel};

use crate::{PluginDetailView, PluginItemView};

/// The picture of a plugin without its own.
pub const PUZZLE_SVG: &str = include_str!("../../../../assets/icons/tabler/outline/puzzle.svg");

/// A Tabler SVG in the theme's primary color.
pub fn svg_image(svg: &str) -> slint::Image {
    let tinted = svg.replace("currentColor", "#89b4fa");
    slint::Image::load_from_svg_data(tinted.as_bytes()).unwrap_or_default()
}

/// The state's words and kind (0 runs, 1 off, 2 warning, 3 error).
pub fn state(state: &PluginState) -> (String, i32) {
    match state {
        PluginState::Ready => ("Runs".to_owned(), 0),
        PluginState::Off => ("Off".to_owned(), 1),
        PluginState::AllOff => ("Off: plugins are off".to_owned(), 1),
        PluginState::SafeMode => ("Off for this start (--no-plugins)".to_owned(), 1),
        PluginState::Paused(why) => (format!("Paused: {why}"), 2),
        PluginState::Shadowed => ("A development folder runs instead".to_owned(), 2),
        PluginState::Broken(why) => (format!("Cannot run: {why}"), 3),
        PluginState::Incompatible(api) => (format!("Needs plugin API {api}"), 3),
    }
}

/// The plugin's facts for its details and its page's first card.
pub fn details(info: &PluginInfo) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut push = |label: &str, value: String| {
        if !value.trim().is_empty() {
            rows.push((label.to_owned(), value));
        }
    };
    if let Some(m) = &info.manifest {
        push("Description", m.description.clone());
        push("Version", m.version.to_string());
        push("Authors", m.authors.join(", "));
        push("License", m.license.clone());
        push("Repository", m.repository.clone());
        push("Plugin API", m.api.clone());
    }
    let source = match info.source {
        Source::Installed => "Installed",
        Source::Development => "Development folder",
    };
    push("Folder", format!("{} ({source})", info.dir.display()));
    let stats = &info.stats;
    push(
        "Since the start",
        format!(
            "{} calls, {} on average, {} the longest, {} errors",
            stats.calls,
            millis(stats.average()),
            millis(stats.longest),
            stats.errors
        ),
    );
    if let Some((at, error)) = &stats.last_error {
        let at: chrono::DateTime<chrono::Local> = (*at).into();
        push("Last error", format!("{} at {}", error, at.format("%H:%M:%S")));
    }
    push("State", state(&info.state).0);
    rows
}

fn millis(time: std::time::Duration) -> String {
    format!("{:.1} ms", time.as_secs_f64() * 1000.0)
}

/// A plugin's row of the list.
pub fn item(info: &PluginInfo, settings: &Settings, expanded: &HashSet<String>, icon: slint::Image) -> PluginItemView {
    let (state_text, state_kind) = state(&info.state);
    let details: Vec<PluginDetailView> = details(info)
        .into_iter()
        .map(|(label, value)| PluginDetailView {
            label: label.into(),
            value: value.into(),
        })
        .collect();
    PluginItemView {
        id: info.id.as_str().into(),
        name: info.name().into(),
        version: info
            .manifest
            .as_ref()
            .map(|m| m.version.to_string())
            .unwrap_or_default()
            .into(),
        description: info
            .manifest
            .as_ref()
            .map(|m| m.description.clone())
            .unwrap_or_default()
            .into(),
        icon,
        state: state_text.into(),
        state_kind,
        enabled: settings.plugins.entry(&info.id).enabled,
        can_toggle: info.manifest.is_some(),
        development: info.source == Source::Development,
        update: info
            .update
            .as_ref()
            .map(|(v, _)| format!("Version {v} is available"))
            .unwrap_or_default()
            .into(),
        expanded: expanded.contains(&info.id),
        details: ModelRc::new(VecModel::from(details)),
        can_remove: true,
        can_move_up: false,
        can_move_down: false,
    }
}

/// The values a target folder or a tag field can take, with their labels; `None` = not set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    /// Target folders: "Next to the source", then every folder as "Group › Name".
    pub folders: Vec<(u32, String)>,
    /// Tags with their paths; `None` while RustView's tag tree did not arrive.
    pub tags: Option<Vec<(u32, String)>>,
}

impl Choices {
    pub fn new(tree: &FolderTree, tags: Option<Vec<(u32, Vec<String>)>>) -> Self {
        let mut folders = vec![(0, SOURCE_NAME.to_owned())];
        fn walk(nodes: &[FolderNode], prefix: &str, out: &mut Vec<(u32, String)>) {
            for node in nodes {
                let label = if prefix.is_empty() {
                    node.name.clone()
                } else {
                    format!("{prefix} › {}", node.name)
                };
                if node.path.is_some() {
                    out.push((node.id.0, label.clone()));
                }
                walk(&node.children, &label, out);
            }
        }
        walk(&tree.entries, "", &mut folders);
        let tags = tags.map(|list| list.into_iter().map(|(id, path)| (id, path.join(" › "))).collect());
        Self { folders, tags }
    }

    /// The labels of a target or tag field, "Not set" first, plus the index of `value`; a value that no longer
    /// exists is listed too, so it stays visible until the user picks another.
    pub fn items(&self, kind: &FieldKind, value: &Value) -> (Vec<String>, usize) {
        let id = value.as_u64().and_then(|n| u32::try_from(n).ok());
        let (list, missing): (&[(u32, String)], String) = match kind {
            FieldKind::Target => (
                &self.folders,
                format!("Folder {} (no longer exists)", id.unwrap_or_default()),
            ),
            FieldKind::Tag => match &self.tags {
                Some(tags) => (tags, format!("Tag {} (no longer in RustView)", id.unwrap_or_default())),
                None => (
                    &[],
                    format!("Tag {} (RustView not reached yet)", id.unwrap_or_default()),
                ),
            },
            FieldKind::Rating { .. } => {
                let mut items = vec!["Not set".to_owned()];
                items.extend((0..=5).map(|n| format!("{n} {}", if n == 1 { "star" } else { "stars" })));
                let index = id.and_then(|n| usize::try_from(n).ok()).map_or(0, |n| n + 1);
                return (items, index.min(6));
            }
            _ => return (Vec::new(), 0),
        };
        let mut items = vec!["Not set".to_owned()];
        items.extend(list.iter().map(|(_, label)| label.clone()));
        let index = match id {
            None => 0,
            Some(id) => list.iter().position(|(known, _)| *known == id).map_or_else(
                || {
                    items.push(missing);
                    items.len() - 1
                },
                |at| at + 1,
            ),
        };
        (items, index)
    }

    /// The value of item `index` of a target or tag field (`items` order); `None` = keep the old value.
    pub fn value_at(&self, kind: &FieldKind, index: usize) -> Option<Value> {
        if index == 0 {
            return Some(Value::Null);
        }
        let list = match kind {
            FieldKind::Target => &self.folders,
            FieldKind::Tag => self.tags.as_ref()?,
            FieldKind::Rating { .. } => return u8::try_from(index - 1).ok().filter(|n| *n <= 5).map(Value::from),
            _ => return None,
        };
        list.get(index - 1).map(|(id, _)| Value::from(*id))
    }

    /// Whether a target or tag value names something that exists (`true` while the tags are unknown).
    fn known(&self, kind: &FieldKind, id: u32) -> bool {
        match kind {
            FieldKind::Target => self.folders.iter().any(|(known, _)| *known == id),
            FieldKind::Tag => self
                .tags
                .as_ref()
                .is_none_or(|tags| tags.iter().any(|(known, _)| *known == id)),
            _ => true,
        }
    }
}

/// RustCut's own findings: tags and target folders the values name that no longer exist (an edge case every plugin
/// has: a rule outlives the tag or folder it names).
pub fn missing_refs(fields: &[&Field], values: &Map<String, Value>, choices: &Choices) -> Vec<Issue> {
    let mut issues = Vec::new();
    let check = |field: &Field, value: Option<&Value>, row: Option<u32>, issues: &mut Vec<Issue>| {
        let Some(id) = value.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) else {
            return;
        };
        if choices.known(&field.kind, id) {
            return;
        }
        let message = match field.kind {
            FieldKind::Target => format!(
                "{}: the target folder no longer exists in Settings > Target Folders; choose another one.",
                field.label
            ),
            _ => format!(
                "{}: the tag no longer exists in RustView's tag tree; choose another one.",
                field.label
            ),
        };
        issues.push(Issue {
            field: field.key.clone(),
            row,
            level: Level::Warning,
            message,
        });
    };
    for field in fields {
        match &field.kind {
            FieldKind::Table { columns, .. } => {
                let rows = values
                    .get(&field.key)
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for (index, row) in rows.iter().enumerate() {
                    for column in columns {
                        let row_index = u32::try_from(index).ok();
                        let mut found = Vec::new();
                        check(column, row.get(&column.key), row_index, &mut found);
                        for mut issue in found {
                            issue.field = format!("{}.{}", field.key, column.key);
                            issues.push(issue);
                        }
                    }
                }
            }
            _ => check(field, values.get(&field.key), None, &mut issues),
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use rustcut_core::folders::{FolderId, FolderTree};
    use serde_json::json;

    use super::*;

    fn tree() -> FolderTree {
        let mut tree = FolderTree::default();
        let _ = tree.add_folder(
            std::path::Path::new("/videos/holiday"),
            rustcut_core::folders::Spot::End,
        );
        tree
    }

    #[test]
    fn missing_tags_and_folders_are_named_per_row() {
        let manifest = rustcut_core::plugins::Manifest::parse(
            "id = \"x1\"\nname = \"X\"\nversion = \"1.0.0\"\napi = \"1.0\"\n[[cards]]\ntitle = \"R\"\n\
             [[cards.fields]]\nkey = \"rules\"\nlabel = \"Rules\"\nkind = \"table\"\n\
             [[cards.fields.columns]]\nkey = \"tag\"\nlabel = \"Tag\"\nkind = \"tag\"\n\
             [[cards.fields.columns]]\nkey = \"target\"\nlabel = \"Target\"\nkind = \"target\"\n",
        )
        .unwrap();
        let folder = tree().folders().first().map(|n| n.id).unwrap_or(FolderId(1));
        let choices = Choices::new(&tree(), Some(vec![(7, vec!["Nature".into(), "Water".into()])]));
        let values = json!({"rules": [{"tag": 7, "target": folder.0}, {"tag": 8, "target": 99}]});
        let fields: Vec<&Field> = manifest.fields().collect();
        let issues = missing_refs(&fields, values.as_object().unwrap(), &choices);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues.iter().all(|i| i.row == Some(1)));
        // Unknown tags count as known while no tag tree arrived.
        let blind = Choices::new(&tree(), None);
        assert_eq!(missing_refs(&fields, values.as_object().unwrap(), &blind).len(), 1);
    }

    #[test]
    fn a_vanished_value_stays_listed() {
        let choices = Choices::new(&tree(), Some(vec![(7, vec!["A".into()])]));
        let (items, index) = choices.items(&FieldKind::Tag, &json!(8));
        assert_eq!(items.last().map(String::as_str), Some("Tag 8 (no longer in RustView)"));
        assert_eq!(index, items.len() - 1);
        assert_eq!(
            choices.items(&FieldKind::Tag, &json!(7)),
            (vec!["Not set".into(), "A".into()], 1)
        );
        assert_eq!(choices.value_at(&FieldKind::Tag, 1), Some(json!(7)));
        assert_eq!(choices.value_at(&FieldKind::Tag, 0), Some(Value::Null));
        let (items, index) = choices.items(&FieldKind::Target, &Value::Null);
        assert_eq!((items[1].as_str(), index), ("Next to the source", 0));
    }
}
