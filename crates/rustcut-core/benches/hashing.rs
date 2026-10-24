//! Benchmark of the MD5 throughput (P6, M6.3): a 256 MiB file through the hash service as the app runs it (sidecar
//! writing off), so the number includes reading from the page cache, the 1 MiB chunks, the progress events and the
//! actor. Run with `cargo bench -p rustcut-core --bench hashing`.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "benchmark code")]

use std::io::Write as _;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rustcut_core::hashing::{HashCommand, spawn_hash_service};

const SIZE: usize = 256 * 1024 * 1024;

fn md5(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mkv");
    let mut file = std::fs::File::create(&path).unwrap();
    let block: Vec<u8> = (0..1024 * 1024).map(|i| u8::try_from(i % 251).unwrap()).collect();
    for _ in 0..SIZE / block.len() {
        file.write_all(&block).unwrap();
    }
    drop(file);
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("md5");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(u64::try_from(SIZE).unwrap()));
    group.bench_function("hash_service_256_mib", |b| {
        b.iter(|| {
            // A new service each time: a service remembers the hashes it computed for the session.
            runtime.block_on(async {
                let (service, task) = spawn_hash_service(runtime.handle());
                let md5 = service
                    .handle
                    .request(|reply| HashCommand::Hash {
                        path: path.clone(),
                        reply: Some(reply),
                    })
                    .await
                    .unwrap()
                    .unwrap();
                service
                    .handle
                    .request(|reply| HashCommand::Shutdown { reply })
                    .await
                    .unwrap();
                task.await.unwrap();
                md5
            })
        });
    });
    group.finish();
}

criterion_group!(benches, md5);
criterion_main!(benches);
