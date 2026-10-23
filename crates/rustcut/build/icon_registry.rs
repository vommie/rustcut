//! Build-time icon registry generator.
//!
//! Copied from RustView (`crates/rustview/build/icon_registry.rs`); changed: the generated `to_slint` targets
//! `crate::IconId` (RustCut re-exports the generated Slint code at the crate root) and the hint for missing files.
//!
//! Reads `assets/icons/icons.toml` (the single source of truth for all icons), validates it against the SVG
//! files on disk and generates the Slint enum/lookup plus the matching Rust enum into `OUT_DIR`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_RELATIVE: &str = "assets/icons/icons.toml";
const ICONS_RELATIVE: &str = "assets/icons";

/// One validated manifest entry.
struct IconEntry {
    id: String,
    /// Path relative to `assets/` (also used as the include-relative `@image-url` path in the generated Slint file).
    source: String,
    monochrome: bool,
    doc: String,
    aliases: Vec<String>,
}

/// Generates `icon_registry.slint` and `icon_registry.rs` in `out_dir` and returns the include root for Slint.
pub fn generate(manifest_dir: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let icons_root = manifest_dir.join(ICONS_RELATIVE);
    let manifest_path = manifest_dir.join(MANIFEST_RELATIVE);
    let text =
        fs::read_to_string(&manifest_path).map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let document: toml::Table = toml::from_str(&text).map_err(|e| format!("invalid {MANIFEST_RELATIVE}: {e}"))?;

    let tabler_version = tabler_version(&document)?;
    let entries = parse_entries(&document)?;
    validate_entries(&entries, manifest_dir)?;

    warn_about_unused_files(&entries, &icons_root);
    write_output(out_dir, "icon_registry.slint", &render_slint(&entries))?;
    write_output(
        out_dir,
        "icon_registry.rs",
        &render_rust(&entries, &tabler_version, manifest_dir),
    )?;
    println!("cargo:rerun-if-changed={MANIFEST_RELATIVE}");
    println!("cargo:rerun-if-changed={ICONS_RELATIVE}");
    Ok(manifest_dir.join("assets"))
}

fn write_output(out_dir: &Path, name: &str, content: &str) -> Result<(), String> {
    fs::write(out_dir.join(name), content).map_err(|e| format!("cannot write {name}: {e}"))
}

fn tabler_version(document: &toml::Table) -> Result<String, String> {
    document
        .get("library")
        .and_then(|v| v.get("tabler_version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "[library] tabler_version is missing".to_owned())
}

fn parse_entries(document: &toml::Table) -> Result<Vec<IconEntry>, String> {
    let icons = document
        .get("icons")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "[icons] table is missing".to_owned())?;
    // `toml::Table` is ordered by key, so the generated output is deterministic.
    icons.iter().map(|(id, value)| parse_entry(id, value)).collect()
}

fn parse_entry(id: &str, value: &toml::Value) -> Result<IconEntry, String> {
    let table = value
        .as_table()
        .ok_or_else(|| format!("icon '{id}': entry must be an inline table"))?;
    let text = |key: &str| table.get(key).and_then(|v| v.as_str());

    let source = match (text("tabler"), text("custom")) {
        (Some(name), None) => {
            let style = text("style").unwrap_or("outline");
            if style != "outline" && style != "filled" {
                return Err(format!("icon '{id}': style must be 'outline' or 'filled'"));
            }
            format!("icons/tabler/{style}/{name}.svg")
        }
        (None, Some(file)) => format!("icons/custom/{file}"),
        _ => return Err(format!("icon '{id}': set exactly one of 'tabler' or 'custom'")),
    };
    let aliases = table
        .get("aliases")
        .and_then(|v| v.as_array())
        .map(|list| list.iter().filter_map(|a| a.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    Ok(IconEntry {
        id: id.to_owned(),
        source,
        monochrome: !table.get("color").and_then(toml::Value::as_bool).unwrap_or(false),
        doc: text("doc").unwrap_or_default().to_owned(),
        aliases,
    })
}

fn validate_entries(entries: &[IconEntry], manifest_dir: &Path) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for entry in entries {
        if !is_valid_id(&entry.id) {
            return Err(format!("icon id '{}' must be lower snake_case", entry.id));
        }
        for name in std::iter::once(&entry.id).chain(entry.aliases.iter()) {
            if !names.insert(name.clone()) {
                return Err(format!("icon name/alias '{name}' is used more than once"));
            }
        }
        let path = manifest_dir.join("assets").join(&entry.source);
        if !path.is_file() {
            return Err(format!(
                "icon '{}': missing file {} (copy it with `icon copy tabler <name> <dir>`)",
                entry.id,
                path.display()
            ));
        }
    }
    Ok(())
}

/// Emits a Cargo warning for SVG files below `assets/icons` that no manifest entry references.
fn warn_about_unused_files(entries: &[IconEntry], icons_root: &Path) {
    let used: BTreeSet<PathBuf> = entries
        .iter()
        .map(|e| icons_root.join(e.source.trim_start_matches("icons/")))
        .collect();
    let mut stack = vec![icons_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = fs::read_dir(&dir) else { continue };
        for path in read_dir.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "svg") && !used.contains(&path) {
                println!(
                    "cargo:warning=[Icons] unused icon file {} (not in icons.toml)",
                    path.display()
                );
            }
        }
    }
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `rating_0` -> `Rating0`
fn pascal_case(id: &str) -> String {
    id.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map_or_else(String::new, |c| c.to_ascii_uppercase().to_string() + chars.as_str())
        })
        .collect()
}

/// `rating_0` -> `rating-0`
fn kebab_case(id: &str) -> String {
    id.replace('_', "-")
}

fn render_slint(entries: &[IconEntry]) -> String {
    let mut out =
        String::from("// @generated by build/icon_registry.rs from assets/icons/icons.toml - DO NOT EDIT.\n\n");
    out.push_str("/// Every icon known to the application. `none` renders nothing.\nexport enum IconId {\n    none,\n");
    for entry in entries {
        let _ = writeln!(out, "    {},", kebab_case(&entry.id));
    }
    out.push_str("}\n\nexport global IconRegistry {\n");
    out.push_str("    /// Vector source of an icon (embedded into the binary).\n");
    out.push_str("    pure public function source(id: IconId) -> image {\n");
    for entry in entries {
        let _ = writeln!(
            out,
            "        if id == IconId.{} {{ return @image-url(\"{}\"); }}",
            kebab_case(&entry.id),
            entry.source
        );
    }
    out.push_str("        return @image-url(\"\");\n    }\n\n");
    out.push_str("    /// Whether the icon is monochrome and therefore tinted with the theme color.\n");
    out.push_str("    pure public function is-tinted(id: IconId) -> bool {\n");
    let colored: Vec<String> = entries
        .iter()
        .filter(|e| !e.monochrome)
        .map(|e| format!("id == IconId.{}", kebab_case(&e.id)))
        .collect();
    if colored.is_empty() {
        out.push_str("        return id != IconId.none;\n");
    } else {
        let _ = writeln!(out, "        if {} {{ return false; }}", colored.join(" || "));
        out.push_str("        return id != IconId.none;\n");
    }
    out.push_str("    }\n}\n");
    out
}

fn render_rust(entries: &[IconEntry], tabler_version: &str, manifest_dir: &Path) -> String {
    let mut out =
        String::from("// @generated by build/icon_registry.rs from assets/icons/icons.toml - DO NOT EDIT.\n\n");
    let _ = writeln!(
        out,
        "/// Version of the bundled Tabler icon set.\npub const TABLER_VERSION: &str = {tabler_version:?};\n"
    );
    out.push_str(&render_rust_enum(entries));
    out.push_str(&render_rust_specs(entries, manifest_dir));
    out.push_str(&render_rust_impl(entries));
    out
}

fn render_rust_enum(entries: &[IconEntry]) -> String {
    let mut out = String::from(
        "/// Every icon known to the application (generated from the manifest, mirrors the Slint `IconId`).\n",
    );
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]\npub enum Icon {\n");
    for entry in entries {
        if !entry.doc.is_empty() {
            let _ = writeln!(out, "    /// {}", entry.doc);
        }
        let _ = writeln!(out, "    #[serde(rename = {:?})]", entry.id);
        let _ = writeln!(out, "    {},", pascal_case(&entry.id));
    }
    out.push_str("}\n\n");
    out
}

fn render_rust_specs(entries: &[IconEntry], manifest_dir: &Path) -> String {
    let mut out = format!("static SPECS: [IconSpec; {}] = [\n", entries.len());
    for entry in entries {
        let aliases: Vec<String> = entry.aliases.iter().map(|a| format!("{a:?}")).collect();
        let svg_path = manifest_dir.join("assets").join(&entry.source);
        let _ = writeln!(
            out,
            "    IconSpec {{ id: {:?}, source: {:?}, monochrome: {}, doc: {:?}, aliases: &[{}], svg: include_str!({:?}) }},",
            entry.id,
            entry.source,
            entry.monochrome,
            entry.doc,
            aliases.join(", "),
            svg_path.to_string_lossy()
        );
    }
    out.push_str("];\n\n");
    out
}

fn render_rust_impl(entries: &[IconEntry]) -> String {
    let variants: Vec<String> = entries
        .iter()
        .map(|e| format!("Icon::{}", pascal_case(&e.id)))
        .collect();
    let mut out = String::from("impl Icon {\n");
    let _ = writeln!(
        out,
        "    /// All icons in manifest (alphabetical) order.\n    pub const ALL: [Icon; {}] = [{}];\n",
        entries.len(),
        variants.join(", ")
    );
    out.push_str("    /// Resolves an icon by id or alias (case-insensitive, `-` and `_` are equivalent).\n");
    out.push_str("    pub fn from_name(name: &str) -> Option<Icon> {\n");
    out.push_str("        match name.trim().to_ascii_lowercase().replace('-', \"_\").as_str() {\n");
    let mut lookup: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries {
        let variant = format!("Icon::{}", pascal_case(&entry.id));
        lookup.insert(entry.id.clone(), variant.clone());
        for alias in &entry.aliases {
            lookup.insert(alias.replace('-', "_"), variant.clone());
        }
    }
    // One arm per icon, with the id and all aliases as alternatives.
    let mut by_variant: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, variant) in &lookup {
        by_variant.entry(variant.as_str()).or_default().push(name.as_str());
    }
    for (variant, names) in &by_variant {
        let patterns: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
        let _ = writeln!(out, "            {} => Some({variant}),", patterns.join(" | "));
    }
    out.push_str("            _ => None,\n        }\n    }\n\n");
    out.push_str("    /// Converts to the Slint enum used by `AppIcon`.\n");
    out.push_str("    pub fn to_slint(self) -> crate::IconId {\n        match self {\n");
    for entry in entries {
        let _ = writeln!(
            out,
            "            Icon::{0} => crate::IconId::{0},",
            pascal_case(&entry.id)
        );
    }
    out.push_str("        }\n    }\n}\n");
    out
}
