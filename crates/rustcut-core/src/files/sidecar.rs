//! A moved file's md5 sidecar goes with it (md5-sidecar rule 6, shared with RustKnown and RustTransferFrom).

use std::path::Path;

/// Moves `from`'s sidecar to `to` after the file itself moved: renamed on the same drive, written anew with its time on
/// another; an invalid sidecar and an orphan at the new place go to the trash. A failure leaves an orphan behind and is
/// only logged (the file's move stands). Blocking.
pub fn carry_sidecar(from: &Path, to: &Path) {
    let mut trash = |path: &Path| trash::delete(path).map_err(std::io::Error::other);
    match md5_sidecar::move_along(from, to, &mut trash) {
        Ok(carried) => tracing::debug!(?carried, to = %to.display(), "[Move] Sidecar handled"),
        Err(e) => tracing::warn!(error = %e, from = %from.display(), "[Move] The sidecar did not follow its file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidecar_follows_a_moved_file() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.mkv"), dir.path().join("b.mkv"));
        std::fs::write(&from, b"video").unwrap();
        std::fs::write(dir.path().join(".a.mkv.md5"), "0123456789abcdef0123456789abcdef").unwrap();
        std::fs::rename(&from, &to).unwrap();
        carry_sidecar(&from, &to);
        assert!(!dir.path().join(".a.mkv.md5").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".b.mkv.md5")).unwrap(),
            "0123456789abcdef0123456789abcdef"
        );
    }
}
