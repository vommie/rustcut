//! "Next in Folder": listing and ordering a folder of 1 000, 10 000 and 100 000 files by name and by date.
//!
//! The folders are made once on the real drive (`$RC_BENCH_DIR`, default `~/.cache/rustcut-bench`), with video and other
//! files, hidden ones and dates that differ. Real numbers need a quiet machine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    reason = "benchmark code"
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rustcut_core::folder_walk::dolphin::props::{DolphinPaths, XATTR};
use rustcut_core::folder_walk::{FileManagerMode, FolderCache, HiddenFiles, Request, list};

const SIZES: [usize; 3] = [1_000, 10_000, 100_000];
const WORDS: [&str; 6] = ["Urlaub", "Ärger", "GoPro", "clip", "Take", "zebra"];

fn root() -> PathBuf {
    std::env::var_os("RC_BENCH_DIR").map_or_else(
        || {
            directories::BaseDirs::new()
                .map(|b| b.cache_dir().join("rustcut-bench"))
                .unwrap_or_default()
        },
        PathBuf::from,
    )
}

/// The folder with `n` files (made when missing) and the name of a video in the middle.
fn folder(n: usize) -> (PathBuf, PathBuf) {
    let dir = root().join(format!("folder-{n}"));
    let done = dir.join(".complete");
    if !done.exists() {
        std::fs::create_dir_all(&dir).expect("folder");
        for i in 0..n {
            let word = WORDS.get(i % WORDS.len()).copied().unwrap_or("clip");
            let ext = match i % 10 {
                0 => "txt",
                1 => "jpg",
                2 => "mkv",
                _ => "mp4",
            };
            let hidden = if i % 50 == 0 { "." } else { "" };
            let path = dir.join(format!(
                "{hidden}{word} {:05} [take {}] - v1.{}.{ext}",
                (i * 7919) % n,
                i % 9,
                i % 13
            ));
            let file = std::fs::File::create(&path).expect("file");
            let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + ((i * 104_729) % 9_999_991) as u64);
            file.set_modified(when).expect("mtime");
        }
        std::fs::write(&done, b"").expect("marker");
    }
    let current = std::fs::read_dir(&dir)
        .expect("read")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mp4"))
        .nth(n / 3)
        .expect("a video");
    (dir, current)
}

fn dolphin_by_date(dir: &Path) -> DolphinPaths {
    let config = root().join("config");
    std::fs::create_dir_all(&config).expect("config");
    std::fs::write(config.join("dolphinrc"), "[General]\nGlobalViewProps=false\n").expect("rc");
    xattr::set(dir, XATTR, b"[Dolphin]\nSortRole=modificationtime\nSortOrder=1\n").expect("xattr");
    DolphinPaths {
        config_home: config,
        data_home: root().join("data"),
        home: root(),
        downloads: None,
    }
}

fn listing(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut group = c.benchmark_group("folder_walk");
    group.sample_size(10);
    for n in SIZES {
        let (dir, current) = folder(n);
        let by_name = Request {
            current: current.clone(),
            file_manager: FileManagerMode::Off,
            hidden: HiddenFiles::Skip,
            videos_only: true,
        };
        group.bench_with_input(BenchmarkId::new("by name", n), &by_name, |b, request| {
            b.iter(|| runtime.block_on(list(request, None, None)).expect("list"));
        });
        let cache = FolderCache::default();
        group.bench_with_input(
            BenchmarkId::new("by name, unchanged folder", n),
            &by_name,
            |b, request| {
                b.iter(|| runtime.block_on(list(request, None, Some(&cache))).expect("list"));
            },
        );
        let paths = dolphin_by_date(&dir);
        let by_date = Request {
            file_manager: FileManagerMode::Always,
            ..by_name
        };
        group.bench_with_input(BenchmarkId::new("by date", n), &by_date, |b, request| {
            b.iter(|| runtime.block_on(list(request, Some(&paths), None)).expect("list"));
        });
    }
    group.finish();
}

criterion_group!(benches, listing);
criterion_main!(benches);
