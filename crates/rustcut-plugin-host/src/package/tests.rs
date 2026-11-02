use super::*;

const MANIFEST: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"1.0.0\"\napi = \"1.0\"\n";

fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
    let encoder = zstd::Encoder::new(std::fs::File::create(path).unwrap(), 3).unwrap();
    let mut builder = tar::Builder::new(encoder);
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // Raw name bytes: `set_path` refuses "..", which is what this test needs to write.
        let raw = &mut header.as_gnu_mut().unwrap().name;
        raw[..name.len()].copy_from_slice(name.as_bytes());
        header.set_cksum();
        builder.append(&header, *bytes).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap();
}

#[test]
fn pack_read_stage_and_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("README.md"), "hello").unwrap();
    std::fs::write(src.join("plugin.wasm"), b"\0asm").unwrap();
    let mut manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.wasm = PathBuf::from("../target/demo.wasm");
    let file = tmp.path().join("demo-1.0.0.rcplugin");
    pack(&manifest, &src.join("plugin.wasm"), &src, &file).unwrap();

    let package = Package::read(&file).unwrap();
    assert_eq!(package.manifest.wasm, PathBuf::from(WASM_FILE));
    let plugins = tmp.path().join("plugins");
    let dir = package.stage(&plugins).unwrap().commit().unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "hello");

    // An update replaces the folder and leaves nothing behind.
    std::fs::write(dir.join("stale.txt"), "x").unwrap();
    let dir = package.stage(&plugins).unwrap().commit().unwrap();
    assert!(!dir.join("stale.txt").exists());
    let names: Vec<_> = std::fs::read_dir(&plugins)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("demo")]);

    remove(&plugins, "demo").unwrap();
    assert!(!dir.exists());
}

#[test]
fn packing_twice_gives_the_same_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("plugin.wasm"), b"\0asm").unwrap();
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    pack(&manifest, &tmp.path().join("plugin.wasm"), tmp.path(), &a).unwrap();
    pack(&manifest, &tmp.path().join("plugin.wasm"), tmp.path(), &b).unwrap();
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

#[test]
fn foreign_paths_and_missing_files_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("bad.rcplugin");
    write_archive(
        &file,
        &[
            ("plugin.toml", MANIFEST.as_bytes()),
            ("../evil.sh", b"x"),
            ("plugin.wasm", b"x"),
        ],
    );
    assert!(matches!(Package::read(&file), Err(PackageError::Unexpected(name)) if name == "../evil.sh"));
    write_archive(&file, &[("plugin.toml", MANIFEST.as_bytes())]);
    assert!(matches!(
        Package::read(&file),
        Err(PackageError::Missing("plugin.wasm"))
    ));
    write_archive(&file, &[("plugin.wasm", b"x")]);
    assert!(matches!(
        Package::read(&file),
        Err(PackageError::Missing("plugin.toml"))
    ));
    std::fs::write(&file, b"not an archive").unwrap();
    assert!(Package::read(&file).is_err());
}
