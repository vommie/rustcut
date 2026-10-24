//! Which settings of a file this version does not know (the dialog about a file from a newer RustCut, M6.5d, user
//! decision Q-N10): every key of the file that the value read from it does not write back, as a dotted path. Empty
//! lists and tables are left out (this version skips writing those too), arrays count as one value, the file's
//! `version` is no setting.

use serde::Serialize;

/// The dotted keys of `text` that `read` (the value this version read from it) does not have.
pub fn unknown_keys<T: Serialize>(text: &str, read: &T) -> Vec<String> {
    let Ok(file) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Ok(toml::Value::Table(known)) = toml::Value::try_from(read) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(&file, &known, "", &mut out);
    // The format version belongs to the file, not to the settings.
    out.retain(|k| k != "version");
    out
}

fn walk(file: &toml::Table, known: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in file {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match (value, known.get(key)) {
            (toml::Value::Table(inner), Some(toml::Value::Table(ours))) => walk(inner, ours, &path, out),
            (_, Some(_)) => {}
            (toml::Value::Table(t), None) if t.is_empty() => {}
            (toml::Value::Array(a), None) if a.is_empty() => {}
            (_, None) => out.push(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn keys_of_a_newer_file_are_found() {
        let text =
            "version = 9\n[general]\nsingle_instance = false\nturbo = true\n[future]\nx = 1\n[player]\nmuted = []\n";
        let read: Settings = toml::from_str(text).unwrap_or_default();
        let keys = unknown_keys(text, &read);
        assert_eq!(keys, ["future", "general.turbo"]);
        assert!(unknown_keys("not = [toml", &read).is_empty());
    }
}
