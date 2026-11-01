//! The migrations of the folder tree (P11, user decision Q-FP2): the list of named target folders becomes the tree, and
//! the export card's index into the list becomes an entry id. Both steps give the folder at index `i` the id `i + 1`
//! (D-FPa), so `config.toml` and `ui-state.toml` agree whichever is migrated first.

use crate::folders::FolderId;

/// The id the folder at `index` of the old list gets.
fn id_of(index: usize) -> Option<i64> {
    i64::try_from(index).ok()?.checked_add(1)
}

/// Settings version 7 -> 8: `export.target_folders = [{label, path}]` becomes `export.folders.entries` (same order, no
/// group, no star; an entry without a path is dropped, its id stays unused).
pub(super) fn migrate_folder_tree(table: &mut toml::Table) -> Result<(), String> {
    let Some(export) = table.get_mut("export") else {
        return Ok(());
    };
    let export = export
        .as_table_mut()
        .ok_or_else(|| "export is not a table".to_owned())?;
    let Some(old) = export.remove("target_folders") else {
        return Ok(());
    };
    let old = old
        .as_array()
        .ok_or_else(|| "export.target_folders is not a list".to_owned())?;
    let entries: Vec<toml::Value> = old
        .iter()
        .enumerate()
        .filter_map(|(i, folder)| {
            let folder = folder.as_table()?;
            let path = folder.get("path")?.as_str()?;
            let name = folder
                .get("label")
                .and_then(toml::Value::as_str)
                .filter(|l| !l.trim().is_empty())
                .map_or_else(
                    || crate::folders::default_name(std::path::Path::new(path)),
                    str::to_owned,
                );
            let mut entry = toml::Table::new();
            entry.insert("id".into(), toml::Value::Integer(id_of(i)?));
            entry.insert("name".into(), toml::Value::String(name));
            entry.insert("path".into(), toml::Value::String(path.to_owned()));
            Some(toml::Value::Table(entry))
        })
        .collect();
    let mut folders = toml::Table::new();
    if !entries.is_empty() {
        folders.insert("entries".into(), toml::Value::Array(entries));
    }
    export.insert("folders".into(), toml::Value::Table(folders));
    Ok(())
}

/// Version 8 -> 9 (P12 3.2): an entry's `picture` (an icon or a stored picture, nothing = the letter) becomes `look`: the
/// choice beside what it keeps, so changing the choice no longer loses a picture.
pub(super) fn migrate_picture_look(table: &mut toml::Table) -> Result<(), String> {
    fn convert(entry: &mut toml::Table) {
        let Some(old) = entry.remove("picture") else {
            return;
        };
        let (kind, key) = match old.as_table() {
            Some(t) if t.contains_key("icon") => ("icon", "icon"),
            Some(t) if t.contains_key("stored") => ("own", "stored"),
            // Older versions read a value they did not know as no picture.
            _ => return,
        };
        let Some(value) = old.get(key).and_then(toml::Value::as_str) else {
            return;
        };
        let mut look = toml::Table::new();
        look.insert("kind".into(), toml::Value::String(kind.to_owned()));
        let field = if kind == "icon" { "icon" } else { "image" };
        look.insert(field.into(), toml::Value::String(value.to_owned()));
        entry.insert("look".into(), toml::Value::Table(look));
    }
    fn walk(entries: &mut toml::Value) {
        for entry in entries.as_array_mut().into_iter().flatten() {
            let Some(entry) = entry.as_table_mut() else { continue };
            convert(entry);
            if let Some(children) = entry.get_mut("children") {
                walk(children);
            }
        }
    }
    let Some(folders) = table
        .get_mut("export")
        .and_then(toml::Value::as_table_mut)
        .and_then(|export| export.get_mut("folders"))
        .and_then(toml::Value::as_table_mut)
    else {
        return Ok(());
    };
    if let Some(source) = folders.get_mut("source").and_then(toml::Value::as_table_mut) {
        convert(source);
    }
    if let Some(entries) = folders.get_mut("entries") {
        walk(entries);
    }
    Ok(())
}

/// UI state version 1 -> 2: `export.target_folder` (an index into the old list, none = the source's folder) becomes
/// `export.target_id` (the entry's id; 0 = "Next to the source").
pub(super) fn migrate_target_id(table: &mut toml::Table) -> Result<(), String> {
    let Some(export) = table.get_mut("export") else {
        return Ok(());
    };
    let export = export
        .as_table_mut()
        .ok_or_else(|| "export is not a table".to_owned())?;
    let Some(old) = export.remove("target_folder") else {
        return Ok(());
    };
    let id = old
        .as_integer()
        .and_then(|i| usize::try_from(i).ok())
        .and_then(id_of)
        .unwrap_or(i64::from(FolderId::SOURCE.0));
    export.insert("target_id".into(), toml::Value::Integer(id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::folders::FolderId;
    use crate::settings::{LoadOutcome, Settings, UiState, load};

    #[test]
    fn the_folder_list_of_0_0_76_becomes_the_tree() {
        // Settings version 7 -> 8 (P11): the folders keep their order and labels, ids 1..n, backed up first.
        let dir = tempfile::tempdir().unwrap();
        let old = "version = 7\n[export]\nname_template = \"{{ name }}\"\n\n[[export.target_folders]]\nlabel = \
                   \"Clips\"\npath = \"/v/clips\"\n\n[[export.target_folders]]\nlabel = \"\"\npath = \"/v/best\"\n\n\
                   [[export.target_folders]]\nlabel = \"no path\"\n\n[[export.target_folders]]\nlabel = \"Last\"\n\
                   path = \"/v/last\"\n";
        std::fs::write(dir.path().join("config.toml"), old).unwrap();
        let outcome = load::<Settings>(dir.path(), &dir.path().join("b")).unwrap();
        let LoadOutcome::Migrated {
            value,
            from: 7,
            backup,
            notes,
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), old, "backed up first");
        assert!(backup.to_string_lossy().contains("folder-tree"));
        let tree = &value.export.folders;
        let shown: Vec<(u32, &str, &Path)> = tree
            .entries
            .iter()
            .map(|n| (n.id.0, n.name.as_str(), n.path.as_deref().unwrap()))
            .collect();
        assert_eq!(
            shown,
            [
                (1, "Clips", Path::new("/v/clips")),
                (2, "best", Path::new("/v/best")),
                (4, "Last", Path::new("/v/last"))
            ]
        );
        assert!(tree.favorites().is_empty() && tree.entries.iter().all(|n| n.children.is_empty()));
        assert_eq!(value.export.name_template, "{{ name }}");
        // Without a list: an empty tree.
        let mut table: toml::Table = toml::from_str("[export]\nname_template = \"x\"\n").unwrap();
        super::migrate_folder_tree(&mut table).unwrap();
        assert!(!table.to_string().contains("folders"), "{table}");
    }

    #[test]
    fn the_chosen_folder_index_becomes_its_id() {
        // UI state version 1 -> 2 (P11): index i -> id i + 1 (the config step's rule), none -> the source.
        for (old, id) in [
            ("version = 1\n[export]\ntarget_folder = 3\ncounter = 7\n", FolderId(4)),
            ("version = 1\n[export]\ncounter = 7\n", FolderId::SOURCE),
            (
                "version = 1\n[export]\ntarget_folder = -2\ncounter = 7\n",
                FolderId::SOURCE,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("ui-state.toml"), old).unwrap();
            let outcome = load::<UiState>(dir.path(), &dir.path().join("b")).unwrap();
            let LoadOutcome::Migrated {
                value, from: 1, backup, ..
            } = outcome
            else {
                panic!("{outcome:?}")
            };
            assert_eq!(std::fs::read_to_string(&backup).unwrap(), old, "backed up first");
            assert_eq!((value.export.target_id, value.export.counter), (id, 7), "{old}");
        }
        let fresh: UiState =
            toml::from_str("version = 2\n[export]\ntarget_id = 5\nrecent_targets = [5, 0]\nlist_collapsed = [3]\n")
                .unwrap();
        assert_eq!(fresh.export.target_id, FolderId(5));
        assert_eq!(fresh.export.list_collapsed, [FolderId(3)]);
        assert_eq!(fresh.export.recent_targets, [FolderId(5), FolderId::SOURCE]);
        let odd: UiState = toml::from_str("version = 2\n[export]\ntarget_id = \"x\"\n").unwrap();
        assert_eq!(odd.export.target_id, FolderId::SOURCE, "unreadable = the source");
    }
}
