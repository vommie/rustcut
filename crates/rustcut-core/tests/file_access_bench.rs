//! Measurements of RustCut's file access (user request 2026-10-24: as little drive work as possible). Ignored in the
//! normal run; `cargo test --release -p rustcut-core --test file_access_bench -- --ignored --nocapture` prints the
//! numbers recorded in the project notes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "measurement code"
)]

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use rustcut_core::naming::{scan_folder, scan_folder_now};

/// The scan as it was before 0.0.20: tokio's `read_dir`, which hops to the blocking pool every 32 entries.
async fn scan_tokio(folder: &Path) -> usize {
    let mut entries = tokio::fs::read_dir(folder).await.unwrap();
    let mut names = HashSet::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    names.len()
}

#[test]
#[ignore = "measurement, run by hand"]
fn folder_scan_of_20000_files() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..20_000 {
        std::fs::write(dir.path().join(format!("Clip - {i:05}.mkv")), b"").unwrap();
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let best = |f: &dyn Fn() -> usize| {
        (0..5)
            .map(|_| {
                let start = Instant::now();
                assert_eq!(f(), 20_000);
                start.elapsed()
            })
            .min()
            .unwrap()
    };
    let old = best(&|| rt.block_on(scan_tokio(dir.path())));
    let new = best(&|| rt.block_on(scan_folder(dir.path())).names.len());
    let direct = best(&|| scan_folder_now(dir.path()).names.len());
    println!("folder scan, 20000 names: tokio read_dir {old:?}, one blocking pass {new:?} (on the thread {direct:?})");
}
