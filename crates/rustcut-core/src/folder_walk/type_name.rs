//! The type descriptions Dolphin sorts by "Type" (`KFileItem::mimeComment()`): the shared MIME database's type for the
//! file name, then that type's `<comment>` in the user's language.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn database() -> &'static xdg_mime::SharedMimeInfo {
    static DB: OnceLock<xdg_mime::SharedMimeInfo> = OnceLock::new();
    DB.get_or_init(xdg_mime::SharedMimeInfo::new)
}

/// The description of `path`'s type ("MPEG-4 video"), empty when unknown.
#[must_use]
pub fn of(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return String::new();
    };
    let Some(mime) = database().get_mime_types_from_file_name(name).into_iter().next() else {
        return String::new();
    };
    comment(mime.essence_str(), &languages()).unwrap_or_default()
}

/// Sets every entry's type description, looked up once per extension (the MIME database's globs are extensions for
/// every video type; a folder of 100 000 files has a handful).
pub fn fill(entries: &mut [super::order::Entry], names: &super::natural::NameOrder) {
    let languages = languages();
    let mut known: std::collections::HashMap<String, super::natural::NameKey> = std::collections::HashMap::new();
    for entry in entries {
        let ext = entry
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let key = known.entry(ext).or_insert_with(|| {
            let name = entry.path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let comment = database()
                .get_mime_types_from_file_name(name)
                .into_iter()
                .next()
                .and_then(|mime| comment(mime.essence_str(), &languages))
                .unwrap_or_default();
            names.key(&comment)
        });
        entry.type_name = key.clone();
    }
}

/// The user's languages, most wanted first (`de_DE`, `de`).
fn languages() -> Vec<String> {
    let mut out = Vec::new();
    let first = std::env::var("LANGUAGE").ok().filter(|v| !v.is_empty());
    let mut wanted: Vec<String> = first
        .map(|v| v.split(':').map(str::to_owned).collect())
        .unwrap_or_default();
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            wanted.push(value);
            break;
        }
    }
    for value in wanted {
        let tag = value.split(['.', '@']).next().unwrap_or_default().to_owned();
        if tag.is_empty() || tag == "C" || tag == "POSIX" {
            continue;
        }
        let short = tag.split('_').next().unwrap_or_default().to_owned();
        out.push(tag);
        out.push(short);
    }
    out
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.data_dir().to_path_buf());
    }
    let system = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty());
    let system = system.unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    dirs
}

/// The comment of `essence` (`video/mp4`) in the first of `languages` that has one, else the untranslated one.
fn comment(essence: &str, languages: &[String]) -> Option<String> {
    let file = data_dirs()
        .into_iter()
        .map(|d| d.join("mime").join(format!("{essence}.xml")))
        .find(|f| f.is_file())?;
    let text = std::fs::read_to_string(file).ok()?;
    comment_in(&text, languages)
}

/// The best `<comment>` of a type's XML.
#[must_use]
pub fn comment_in(xml: &str, languages: &[String]) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let comments: Vec<(Option<&str>, &str)> = doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("comment"))
        .map(|n| {
            (
                n.attribute(("http://www.w3.org/XML/1998/namespace", "lang")),
                n.text().unwrap_or_default(),
            )
        })
        .collect();
    languages
        .iter()
        .find_map(|lang| {
            comments
                .iter()
                .find(|(l, _)| *l == Some(lang.as_str()))
                .map(|(_, t)| (*t).to_owned())
        })
        .or_else(|| comments.iter().find(|(l, _)| l.is_none()).map(|(_, t)| (*t).to_owned()))
}

#[cfg(test)]
mod tests {
    use super::comment_in;

    #[test]
    fn picks_the_language_then_the_plain_comment() {
        let xml = r#"<?xml version="1.0"?><mime-type xmlns="http://www.freedesktop.org/standards/shared-mime-info" type="video/mp4"><comment>MPEG-4 video</comment><comment xml:lang="de">MPEG-4-Video</comment></mime-type>"#;
        assert_eq!(
            comment_in(xml, &["de_DE".into(), "de".into()]).as_deref(),
            Some("MPEG-4-Video")
        );
        assert_eq!(comment_in(xml, &["fr".into()]).as_deref(), Some("MPEG-4 video"));
    }
}
