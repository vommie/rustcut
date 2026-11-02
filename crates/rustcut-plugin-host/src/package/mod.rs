//! Plugin files (`.rcplugin`): a tar archive compressed with zstd, every file at the top level:
//! `plugin.toml`, `plugin.wasm`, and optionally `icon.svg`, `README.md`, `CHANGELOG.md`, `LICENSE`.
//!
//! Installing reads the whole archive into memory with size limits (nothing is unpacked by path, so no entry can
//! write outside the plugin's folder), writes it into a staging folder and swaps that folder in only after the caller
//! checked it (the component compiles). The previous version stays until the swap succeeded.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::manifest::{MANIFEST_FILE, Manifest, ManifestError, WASM_FILE};

/// The file name extension of plugin files.
pub const EXTENSION: &str = "rcplugin";

/// The files a plugin file may hold, besides the two required ones.
const OPTIONAL_FILES: [&str; 4] = ["icon.svg", "README.md", "CHANGELOG.md", "LICENSE"];

/// The largest component accepted.
const MAX_WASM: u64 = 64 << 20;

/// The largest other file accepted.
const MAX_OTHER: u64 = 1 << 20;

/// Why a plugin file cannot be read or installed.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("cannot read the plugin file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the plugin file holds \"{0}\", which a plugin file may not hold")]
    Unexpected(String),
    #[error("the plugin file holds \"{0}\" twice")]
    Twice(String),
    #[error("\"{0}\" in the plugin file is too large")]
    TooLarge(String),
    #[error("the plugin file has no {0}")]
    Missing(&'static str),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
}

/// A plugin file's content, read and checked.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// Reads and checks a plugin file.
    ///
    /// # Errors
    /// The file is no tar.zst archive, holds unexpected or oversized entries, or its manifest is invalid.
    pub fn read(file: &Path) -> Result<Self, PackageError> {
        let decoder = zstd::Decoder::new(std::fs::File::open(file)?)?;
        let mut archive = tar::Archive::new(decoder);
        let mut files = BTreeMap::new();
        for entry in archive.entries()? {
            let entry = entry?;
            let name = entry.path()?.to_string_lossy().trim_start_matches("./").to_owned();
            if entry.header().entry_type().is_dir() && name.is_empty() {
                continue;
            }
            let allowed = name == MANIFEST_FILE || name == WASM_FILE || OPTIONAL_FILES.contains(&name.as_str());
            if !allowed || !entry.header().entry_type().is_file() {
                return Err(PackageError::Unexpected(name));
            }
            let limit = if name == WASM_FILE { MAX_WASM } else { MAX_OTHER };
            if entry.size() > limit {
                return Err(PackageError::TooLarge(name));
            }
            let mut bytes = Vec::new();
            entry.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
                return Err(PackageError::TooLarge(name));
            }
            if files.insert(name.clone(), bytes).is_some() {
                return Err(PackageError::Twice(name));
            }
        }
        let manifest_bytes = files.get(MANIFEST_FILE).ok_or(PackageError::Missing(MANIFEST_FILE))?;
        let manifest = Manifest::parse(&String::from_utf8_lossy(manifest_bytes))?;
        if manifest.wasm != Path::new(WASM_FILE) {
            return Err(PackageError::Manifest(ManifestError::Invalid(format!(
                "an installed plugin's wasm must be \"{WASM_FILE}\""
            ))));
        }
        if !files.contains_key(WASM_FILE) {
            return Err(PackageError::Missing(WASM_FILE));
        }
        Ok(Self { manifest, files })
    }

    /// Writes the files into a staging folder below `plugins_dir`; [`Staged::commit`] puts it in place.
    ///
    /// # Errors
    /// The folder cannot be written.
    pub fn stage(&self, plugins_dir: &Path) -> Result<Staged, PackageError> {
        std::fs::create_dir_all(plugins_dir)?;
        let dir = plugins_dir.join(format!(".staging-{}", self.manifest.id));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir(&dir)?;
        for (name, bytes) in &self.files {
            std::fs::write(dir.join(name), bytes)?;
        }
        Ok(Staged {
            dir,
            target: plugins_dir.join(&self.manifest.id),
            manifest: self.manifest.clone(),
        })
    }
}

/// A plugin written into its staging folder, not yet in place.
#[derive(Debug)]
pub struct Staged {
    pub dir: PathBuf,
    target: PathBuf,
    pub manifest: Manifest,
}

impl Staged {
    /// Swaps the staging folder in; an installed version is removed only after the new one is in place.
    ///
    /// # Errors
    /// A rename or the removal fails; the previous version is put back when the swap itself failed.
    pub fn commit(self) -> Result<PathBuf, PackageError> {
        let old = self.target.with_file_name(format!(".old-{}", self.manifest.id));
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        let had_old = self.target.exists();
        if had_old {
            std::fs::rename(&self.target, &old)?;
        }
        if let Err(err) = std::fs::rename(&self.dir, &self.target) {
            if had_old {
                std::fs::rename(&old, &self.target)?;
            }
            return Err(err.into());
        }
        if had_old {
            std::fs::remove_dir_all(&old)?;
        }
        Ok(self.target)
    }

    /// Removes the staging folder.
    ///
    /// # Errors
    /// The folder cannot be removed.
    pub fn discard(self) -> Result<(), PackageError> {
        std::fs::remove_dir_all(&self.dir)?;
        Ok(())
    }
}

/// Packs a plugin file: the manifest (its `wasm` set to `plugin.wasm`), the component and the optional files that
/// exist in `extras_dir`. The archive is reproducible: sorted, every time stamp 0, mode 0644.
///
/// # Errors
/// A file cannot be read or the archive cannot be written.
pub fn pack(manifest: &Manifest, wasm: &Path, extras_dir: &Path, out: &Path) -> Result<(), PackageError> {
    let mut manifest = manifest.clone();
    manifest.wasm = PathBuf::from(WASM_FILE);
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(MANIFEST_FILE.to_owned(), manifest.to_toml()?.into_bytes());
    files.insert(WASM_FILE.to_owned(), std::fs::read(wasm)?);
    for name in OPTIONAL_FILES {
        let path = extras_dir.join(name);
        if path.is_file() {
            files.insert(name.to_owned(), std::fs::read(path)?);
        }
    }
    let encoder = zstd::Encoder::new(std::fs::File::create(out)?, 19)?;
    let mut builder = tar::Builder::new(encoder);
    builder.mode(tar::HeaderMode::Deterministic);
    for (name, bytes) in &files {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        builder.append_data(&mut header, name, bytes.as_slice())?;
    }
    builder.into_inner()?.finish()?;
    Ok(())
}

/// Removes an installed plugin's folder.
///
/// # Errors
/// The folder cannot be removed.
pub fn remove(plugins_dir: &Path, id: &str) -> Result<(), PackageError> {
    let dir = plugins_dir.join(id);
    if crate::manifest::valid_id(id) && dir.is_dir() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}
