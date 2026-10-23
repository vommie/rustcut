//! Compiles the Slint UI (`ui/main.slint`) into Rust code that `src/lib.rs` includes.

fn main() -> Result<(), slint_build::CompileError> {
    println!("cargo:rerun-if-changed=ui/");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", config)
}
