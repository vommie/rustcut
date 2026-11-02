//! Benchmarks of the queue and the store (P6, M6.3): the queue's snapshot (every job summarized, as the panel gets it)
//! and its scheduler with many jobs, and the SQLite store: inserting jobs, reading them back, appending and reading
//! log lines, in memory and in a file with WAL like the app. Run with `cargo bench -p rustcut-core --bench queue_store`.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "benchmark code")]

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use rustcut_core::edit::{FilterChain, Section, SectionList};
use rustcut_core::job::{JobId, JobMetadata, JobSpec, JobState, TargetSpec};
use rustcut_core::queue::{Model, summarize};
use rustcut_core::render::builtin_profiles;
use rustcut_core::settings::WhenDone;
use rustcut_core::store::{JobRecord, LogStream, SpecSlot, Store};
use rustcut_core::time::Timestamp;

fn spec(n: usize) -> JobSpec {
    let sections: SectionList = (0..3)
        .map(|i| Section::new(Timestamp::from_secs(i * 10), Timestamp::from_secs(i * 10 + 5)).unwrap())
        .collect::<Vec<_>>()
        .into();
    JobSpec {
        version: 1,
        source: PathBuf::from(format!("/media/videos/source {}.mkv", n % 40)),
        source_info: None,
        sections,
        filters: FilterChain::default(),
        final_multiple: 2,
        profile: builtin_profiles().into_iter().next().unwrap(),
        target: TargetSpec {
            folder: PathBuf::from("/media/out"),
            stem: format!("clip - {n:04}"),
            overwrite: false,
            naming: None,
        },
        metadata: JobMetadata::default(),
    }
}

/// `count` jobs as the queue holds them: most finished, some waiting, one rendering.
fn records(count: usize) -> Vec<JobRecord> {
    (0..count)
        .map(|n| {
            let spec = spec(n);
            let state = match n % 10 {
                0..=6 => JobState::Finished,
                7 if n == 7 => JobState::Rendering { pass: 1, passes: 1 },
                _ => JobState::Waiting,
            };
            JobRecord {
                id: JobId(i64::try_from(n).unwrap() + 1),
                position: i64::try_from(n).unwrap(),
                state,
                attempt: 1,
                created_at: 1_761_300_000_000,
                started_at: None,
                finished_at: None,
                source_path: spec.source.clone(),
                target_path: spec.target_path(),
                note: None,
                spec: SpecSlot::Decoded(Box::new(spec)),
                sync: None,
                relocation: None,
            }
        })
        .collect()
}

fn queue(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue");
    for count in [100, 1_000, 10_000] {
        let model = Model::new(records(count), 2, false, WhenDone::Nothing);
        group.throughput(Throughput::Elements(u64::try_from(count).unwrap()));
        group.bench_function(format!("snapshot_{count}_jobs"), |b| {
            b.iter(|| {
                let jobs: Vec<_> = black_box(&model).jobs().iter().map(summarize).collect();
                (jobs, model.processing())
            });
        });
        group.bench_function(format!("schedule_{count}_jobs"), |b| {
            b.iter(|| black_box(&model).schedule());
        });
    }
    group.finish();
}

fn insert_jobs(store: &mut Store, count: usize) {
    for n in 0..count {
        store.insert_job(&spec(n), &JobState::Waiting).unwrap();
    }
}

fn store(c: &mut Criterion) {
    let mut group = c.benchmark_group("store");
    group.sample_size(20);
    group.throughput(Throughput::Elements(1_000));
    group.bench_function("insert_1000_jobs_in_memory", |b| {
        b.iter_batched(
            || Store::open_in_memory().unwrap(),
            |mut store| insert_jobs(&mut store, 1_000),
            BatchSize::PerIteration,
        );
    });
    group.bench_function("insert_1000_jobs_file_wal", |b| {
        b.iter_batched(
            || {
                let dir = tempfile::tempdir().unwrap();
                let store = Store::open(&dir.path().join("queue.sqlite"), &dir.path().join("backups")).unwrap();
                (dir, store)
            },
            |(_dir, mut store)| insert_jobs(&mut store, 1_000),
            BatchSize::PerIteration,
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let mut filled = Store::open(&dir.path().join("queue.sqlite"), &dir.path().join("backups")).unwrap();
    insert_jobs(&mut filled, 1_000);
    group.bench_function("read_1000_jobs_file_wal", |b| {
        b.iter(|| black_box(&filled).jobs().unwrap());
    });
    let lines: Vec<String> = (0..1_000)
        .map(|i| {
            format!(
                "frame={i} fps=24.9 q=28.0 size={}kB time=00:00:{:02}.00 bitrate=1834.2kbits/s",
                i * 9,
                i % 60
            )
        })
        .collect();
    group.bench_function("append_1000_log_lines_file_wal", |b| {
        b.iter(|| {
            filled
                .append_log(JobId(1), LogStream::Ffmpeg, black_box(&lines))
                .unwrap();
        });
    });
    group.bench_function("read_2000_log_lines_file_wal", |b| {
        b.iter(|| black_box(&filled).log(JobId(1), 2_000).unwrap());
    });
    group.finish();
}

criterion_group!(benches, queue, store);
criterion_main!(benches);
