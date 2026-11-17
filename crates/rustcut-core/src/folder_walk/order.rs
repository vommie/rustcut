//! Dolphin's file order for one folder (`KFileItemModel::lessThan` and `sortRoleCompare`): hidden files last when
//! asked, then the sort role, then the name, then the path; descending turns everything but "hidden last" around.

use std::cmp::Ordering;
use std::path::PathBuf;

use super::natural::NameOrder;

/// What a folder is sorted by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortRole {
    /// The file name (Dolphin's `text`).
    Name,
    /// The file size.
    Size,
    /// The modification time.
    Modified,
    /// The creation (birth) time.
    Created,
    /// The access time.
    Accessed,
    /// The type's description ("MPEG-4 video").
    Type,
    /// A role RustCut cannot rebuild (rating, tags, length, ...): Dolphin's id, sorted by name instead.
    Unsupported(String),
}

impl SortRole {
    /// Dolphin's role id (`text`, `size`, `modificationtime`, ...; the legacy `name`/`date` too).
    #[must_use]
    pub fn from_dolphin(role: &str) -> Self {
        match role.trim() {
            "" | "text" | "name" => Self::Name,
            "size" => Self::Size,
            "modificationtime" | "date" => Self::Modified,
            "creationtime" => Self::Created,
            "accesstime" => Self::Accessed,
            "type" => Self::Type,
            other => Self::Unsupported(other.to_owned()),
        }
    }

    /// Dolphin's id of this role.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Name => "text",
            Self::Size => "size",
            Self::Modified => "modificationtime",
            Self::Created => "creationtime",
            Self::Accessed => "accesstime",
            Self::Type => "type",
            Self::Unsupported(id) => id,
        }
    }
}

/// How a folder's files are ordered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    /// The role.
    pub role: SortRole,
    /// Largest, newest or Z first.
    pub descending: bool,
    /// Hidden files after the others (Dolphin's "Hidden files last"), whatever the direction.
    pub hidden_last: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            role: SortRole::Name,
            descending: false,
            hidden_last: false,
        }
    }
}

/// One file of the folder with what the order needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// The full path.
    pub path: PathBuf,
    /// The file name as shown.
    pub name: String,
    /// The name starts with a dot.
    pub hidden: bool,
    /// Bytes.
    pub size: u64,
    /// Seconds since the epoch; -1 when unknown (Dolphin's default).
    pub modified: i64,
    /// Seconds since the epoch; -1 when unknown.
    pub created: i64,
    /// Seconds since the epoch; -1 when unknown.
    pub accessed: i64,
    /// The type's description; empty when unknown (sorts last ascending).
    pub type_name: String,
}

/// Sorts `entries` in Dolphin's order.
pub fn sort(entries: &mut [Entry], spec: &SortSpec, names: &NameOrder) {
    entries.sort_by(|a, b| compare(a, b, spec, names));
}

/// Dolphin's `lessThan` as an ordering.
#[must_use]
pub fn compare(a: &Entry, b: &Entry, spec: &SortSpec, names: &NameOrder) -> Ordering {
    if spec.hidden_last && a.hidden != b.hidden {
        return a.hidden.cmp(&b.hidden);
    }
    let result = role_compare(a, b, &spec.role, names)
        .then_with(|| names.compare(&a.name, &b.name))
        .then_with(|| a.path.cmp(&b.path));
    if spec.descending { result.reverse() } else { result }
}

fn role_compare(a: &Entry, b: &Entry, role: &SortRole, names: &NameOrder) -> Ordering {
    match role {
        SortRole::Name | SortRole::Unsupported(_) => Ordering::Equal,
        SortRole::Size => a.size.cmp(&b.size),
        SortRole::Modified => a.modified.cmp(&b.modified),
        SortRole::Created => a.created.cmp(&b.created),
        SortRole::Accessed => a.accessed.cmp(&b.accessed),
        SortRole::Type => match (a.type_name.is_empty(), b.type_name.is_empty()) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => names.compare(&a.type_name, &b.type_name),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder_walk::natural::SortingChoice;

    fn entry(name: &str, size: u64, modified: i64, hidden: bool) -> Entry {
        Entry {
            path: PathBuf::from("/v").join(name),
            name: name.to_owned(),
            hidden,
            size,
            modified,
            created: -1,
            accessed: -1,
            type_name: String::new(),
        }
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn date_descending_breaks_ties_by_name_turned_around_too() {
        let order = NameOrder::new(SortingChoice::Natural, "C");
        let mut list = vec![
            entry("b", 1, 10, false),
            entry("a", 1, 10, false),
            entry("c", 1, 20, false),
        ];
        let spec = SortSpec {
            role: SortRole::Modified,
            descending: true,
            hidden_last: false,
        };
        sort(&mut list, &spec, &order);
        assert_eq!(names(&list), ["c", "b", "a"]);
    }

    #[test]
    fn hidden_last_is_never_turned_around() {
        let order = NameOrder::new(SortingChoice::Natural, "C");
        let mut list = vec![
            entry(".z", 5, 0, true),
            entry("a", 1, 0, false),
            entry("b", 9, 0, false),
        ];
        let spec = SortSpec {
            role: SortRole::Size,
            descending: true,
            hidden_last: true,
        };
        sort(&mut list, &spec, &order);
        assert_eq!(names(&list), ["b", "a", ".z"]);
    }

    #[test]
    fn type_without_a_name_comes_last_and_roles_round_trip() {
        let order = NameOrder::new(SortingChoice::Natural, "C");
        let mut a = entry("a", 0, 0, false);
        let mut b = entry("b", 0, 0, false);
        b.type_name = "MPEG-4 video".into();
        a.type_name = String::new();
        let mut list = vec![a, b];
        sort(
            &mut list,
            &SortSpec {
                role: SortRole::Type,
                ..SortSpec::default()
            },
            &order,
        );
        assert_eq!(names(&list), ["b", "a"]);
        for id in [
            "text",
            "size",
            "modificationtime",
            "creationtime",
            "accesstime",
            "type",
            "rating",
        ] {
            assert_eq!(SortRole::from_dolphin(id).id(), id);
        }
        assert_eq!(SortRole::from_dolphin("date"), SortRole::Modified);
    }
}
