//! ADR-10: the tag tree, the rating bar and their helpers are copies of RustView's components. This test compares every
//! copy with its original in the RustView checkout (`~/claude/rustview`, or `RUSTCUT_RUSTVIEW_DIR`) and fails with a diff
//! when they differ, so a change in RustView is noticed and pulled over. Without the checkout it is skipped (reported).
//!
//! Slint copies are compared as text after leaving out the copy's header comment, the import lines and the blocks marked
//! `// RustCut begin` ... `// RustCut end` (RustCut's only additions inside a copy). The Rust ports cannot be compared
//! as text (other types), so RustView's functions are pinned by a hash of their normalized text: when one changes, the
//! test names it and prints it, the port is reviewed, and the pin is updated here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, reason = "test code")]

use std::fmt::Write as _;
use std::hash::Hasher as _;
use std::path::{Path, PathBuf};

/// Whole files copied unchanged except their imports.
const WHOLE_FILES: &[&str] = &[
    "node_button.slint",
    "rating_bar.slint",
    "search_field.slint",
    "tag_search_filter.slint",
    "collapsible_panel.slint",
    "smooth_scroll_view.slint",
    "tag_history_item.slint",
    "tag_history_panel.slint",
    "resizer_bar.slint",
];

/// Components copied out of a RustView file: (RustCut file, RustView file, components).
const COMPONENTS: &[(&str, &str, &[&str])] = &[("tag_tree.slint", "tag_panel.slint", &["TagTreeItem", "TagTreeList"])];

/// RustView's Rust functions the ports follow: (file below `crates/rustview/src/ui/tags/`, function, pinned hash).
const PINNED_FUNCTIONS: &[(&str, &str, u64)] = &[
    ("tree.rs", "build_hierarchy", 0x6ac4_595b_b663_5e67),
    ("tree.rs", "build_branch", 0x7fe8_05c1_80ee_db06),
    ("tree.rs", "flatten", 0xdc7f_2b68_a87a_ed1d),
    ("tree.rs", "chunk_into_columns", 0x5e8d_87a3_7e11_3373),
    ("history/manager.rs", "record_tag_used", 0x6e1e_9907_51f8_feb0),
    ("history/manager.rs", "prune_invalid", 0xef2d_6a41_7bf3_3a8f),
    ("history/manager.rs", "build_hierarchy_tooltip", 0xbb1d_90d1_9912_7190),
    ("history/layout.rs", "estimate_stable_pill_width", 0x3c2d_2f7d_9bf7_3978),
    ("history/layout.rs", "pack_into_rows", 0xc388_d8f1_125e_2e3b),
    ("history_bridge.rs", "sync_ui", 0x1316_7034_6723_7af0),
];

/// RustView's constants the ports copy: (file below `crates/rustview/src/ui/tags/`, pinned hash of its `pub const` lines).
const PINNED_CONSTANTS: &[(&str, u64)] = &[
    ("mod.rs", 0xd647_caa6_eeae_9e79),
    ("history/layout.rs", 0x518e_6a3b_c506_f6f0),
];

fn rustview_dir() -> PathBuf {
    std::env::var_os("RUSTCUT_RUSTVIEW_DIR").map_or_else(
        || {
            let home = std::env::var_os("HOME").unwrap_or_default();
            Path::new(&home).join("claude/rustview")
        },
        PathBuf::from,
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()))
}

/// The text that has to match: no leading comment header, no imports, no RustCut blocks, no blank lines, no trailing
/// spaces.
fn normalized_slint(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_header = true;
    let mut in_rustcut = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if in_header && (trimmed.starts_with("//") || trimmed.is_empty()) {
            continue;
        }
        in_header = false;
        if trimmed.starts_with("// RustCut begin") {
            in_rustcut = true;
            continue;
        }
        if trimmed.starts_with("// RustCut end") {
            in_rustcut = false;
            continue;
        }
        if in_rustcut || trimmed.is_empty() || trimmed.starts_with("import ") {
            continue;
        }
        out.push(line.trim_end());
    }
    out.join("\n")
}

/// `export component NAME ...` up to its closing brace at the start of a line.
fn component<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("export component {name} "))?;
    let end = text.get(start..)?.find("\n}")? + start + 2;
    text.get(start..end)
}

fn diff(original: &str, copy: &str, what: &str) -> String {
    let mut text = format!("--- RustView {what}\n+++ RustCut {what}\n");
    for hunk in similar::TextDiff::from_lines(original, copy)
        .unified_diff()
        .context_radius(2)
        .iter_hunks()
    {
        let _ = write!(text, "{hunk}");
    }
    text
}

/// The body of `fn name` (signature through the matching closing brace); string literals are skipped while counting.
fn function<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text
        .match_indices(&format!("fn {name}"))
        .map(|(at, _)| at)
        .find(|&at| {
            text.get(at + 3 + name.len()..)
                .is_some_and(|rest| rest.starts_with('(') || rest.starts_with('<'))
        })?;
    let open = text.get(start..)?.find('{')? + start;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, c) in text.get(open..)?.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(start..=open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// FNV-1a over the text with every run of whitespace as one space (formatting changes do not count).
fn fingerprint(text: &str) -> u64 {
    let mut hasher = fnv::FnvHasher::default();
    hasher.write(text.split_whitespace().collect::<Vec<_>>().join(" ").as_bytes());
    hasher.finish()
}

#[test]
fn copies_match_rustview() {
    let rustview = rustview_dir();
    let rv_ui = rustview.join("crates/rustview/ui");
    let rv_tags = rustview.join("crates/rustview/src/ui/tags");
    if !rv_ui.is_dir() {
        eprintln!(
            "SKIPPED: no RustView checkout at {} (set RUSTCUT_RUSTVIEW_DIR); the copies were not compared",
            rustview.display()
        );
        return;
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut problems = String::new();

    for file in WHOLE_FILES {
        let copy = read(&ui.join(file));
        assert!(
            copy.starts_with("// Copied from RustView"),
            "{file}: the copy has no header naming its RustView source"
        );
        let (original, copy) = (normalized_slint(&read(&rv_ui.join(file))), normalized_slint(&copy));
        if original != copy {
            problems.push_str(&diff(&original, &copy, file));
        }
    }

    for (file, source, names) in COMPONENTS {
        let copy_text = read(&ui.join(file));
        let original_text = read(&rv_ui.join(source));
        assert!(
            copy_text.starts_with("// Copied from RustView"),
            "{file}: no source header"
        );
        for name in *names {
            let what = format!("{source}::{name}");
            match (component(&original_text, name), component(&copy_text, name)) {
                (Some(original), Some(copy)) => {
                    let (original, copy) = (normalized_slint(original), normalized_slint(copy));
                    if original != copy {
                        problems.push_str(&diff(&original, &copy, &what));
                    }
                }
                (None, _) => {
                    let _ = writeln!(problems, "{what}: RustView has no such component any more");
                }
                (_, None) => {
                    let _ = writeln!(problems, "{what}: missing in {file}");
                }
            }
        }
    }

    for (file, name, pinned) in PINNED_FUNCTIONS {
        let text = read(&rv_tags.join(file));
        match function(&text, name) {
            Some(body) => {
                let now = fingerprint(body);
                if now != *pinned {
                    let _ = writeln!(
                        problems,
                        "RustView's {file}::{name} changed since it was ported (pinned {pinned:#018x}, now {now:#018x}): \
                         review the port in src/ui/tags/ and update the pin.\n{body}\n"
                    );
                }
            }
            None => {
                let _ = writeln!(problems, "RustView's {file} has no fn {name} any more");
            }
        }
    }

    for (file, pinned) in PINNED_CONSTANTS {
        let text = read(&rv_tags.join(file));
        let constants: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("pub const "))
            .collect();
        let now = fingerprint(&constants.join("\n"));
        if now != *pinned {
            let _ = writeln!(
                problems,
                "RustView's constants in {file} changed (pinned {pinned:#018x}, now {now:#018x}): compare them with \
                 src/ui/tags/ and update the pin.\n{}\n",
                constants.join("\n")
            );
        }
    }

    assert!(problems.is_empty(), "the copies differ from RustView:\n{problems}");
}

#[test]
fn normalizing_leaves_out_headers_imports_and_rustcut_blocks() {
    let copy = "// Copied from RustView\n\nimport { Theme } from \"styles.slint\";\nexport component A {\n    x: 1;\n    \
                // RustCut begin: extra\n    y: 2;\n    // RustCut end\n}\n";
    let original = "import { Theme } from \"styles.slint\";\n\nexport component A {\n    x: 1;   \n}\n";
    assert_eq!(normalized_slint(copy), normalized_slint(original));
    assert_eq!(
        component(copy, "A").map(normalized_slint).as_deref(),
        Some("export component A {\n    x: 1;\n}")
    );
    let rust = "impl X {\n    pub fn a(v: &str) -> String {\n        format!(\"{{{v}\")\n    }\n    fn ab() {}\n}";
    assert_eq!(
        function(rust, "a"),
        Some("fn a(v: &str) -> String {\n        format!(\"{{{v}\")\n    }")
    );
    assert_eq!(function(rust, "ab"), Some("fn ab() {}"));
    assert_eq!(fingerprint("a  b\n c"), fingerprint("a b c"));
}
