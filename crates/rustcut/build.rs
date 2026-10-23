//! Compiles the Slint UI (`ui/main.slint`) into Rust code that `src/lib.rs` includes, after generating the icon registry
//! (`assets/icons/icons.toml` -> Slint `IconId` + Rust `Icon`, see `build/icon_registry.rs`).

use std::env;
use std::path::PathBuf;

#[path = "build/icon_registry.rs"]
mod icon_registry;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=ui/");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    // The generated `icon_registry.slint` lives in OUT_DIR and references the SVGs relative to `assets/`.
    let assets_root = icon_registry::generate(&manifest_dir, &out_dir).map_err(|e| format!("[Icons] {e}"))?;
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_include_paths(vec![out_dir, assets_root])
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        // Slint's MCP server finds elements by id only with debug info in the generated code (feature `slint-mcp`).
        .with_debug_info(env::var_os("CARGO_FEATURE_SLINT_MCP").is_some());
    slint_build::compile_with_config("ui/main.slint", config)?;
    Ok(())
}
