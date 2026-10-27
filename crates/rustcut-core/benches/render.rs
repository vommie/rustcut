//! Benchmarks of the pure render code (P6, M6.3): the filter chain to lavfi text (preview and render), the command
//! plan of a job, ffmpeg's `-progress` parser and the decoding of ffprobe's JSON. Run with
//! `cargo bench -p rustcut-core --bench render`.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "benchmark code")]

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rustcut_core::edit::{
    Crop, DeinterlaceRate, Deinterlacer, DeshakeParams, DeshakePass, FieldParity, Filter, FilterChain, LavfiTarget,
    OrientationParams, ResizeAlgorithm, Section, SectionList, Size,
};
use rustcut_core::job::{JobId, JobMetadata, JobSpec, TargetSpec};
use rustcut_core::media::{MediaInfo, decode};
use rustcut_core::render::{HardwareContext, PlanInput, ProgressParser, WorkDirs, builtin_profiles, plan};
use rustcut_core::time::Timestamp;

/// ffprobe's description of `multi_audio_2tracks.mkv` (two audio tracks) and of `hd_h264_1080p.mp4`.
const MULTI_AUDIO_JSON: &[u8] = include_bytes!("fixtures/multi_audio_2tracks.json");
const HD_JSON: &[u8] = include_bytes!("fixtures/hd_h264_1080p.json");

/// Every filter at once, as a user may set them.
fn full_chain() -> FilterChain {
    let mut chain = FilterChain::default();
    for filter in [
        Filter::Deinterlace {
            deinterlacer: Deinterlacer::Bwdif,
            rate: DeinterlaceRate::Frame,
            field_order: FieldParity::Auto,
            only_interlaced: false,
        },
        Filter::Crop(Crop::new(40, 10, 40, 10)),
        Filter::Deshake(DeshakeParams::default()),
        Filter::Orientation(OrientationParams {
            turns: 1,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
        Filter::Resize {
            width: Some(640),
            height: None,
            algorithm: ResizeAlgorithm::Lanczos,
        },
    ] {
        let kind = filter.kind();
        chain.set(filter);
        chain.set_enabled(kind, true);
    }
    chain
}

/// `sections` sections spread over the first 7 s of the 8 s fixture (sections past the end would be dropped).
fn spec(sections: usize) -> JobSpec {
    let step = 7_000 / i64::try_from(sections).unwrap();
    let list: SectionList = (0..sections)
        .map(|i| {
            let start = i64::try_from(i).unwrap() * step;
            Section::new(
                Timestamp::from_millis(start),
                Timestamp::from_millis(start + step * 2 / 3),
            )
            .unwrap()
        })
        .collect::<Vec<_>>()
        .into();
    JobSpec {
        version: 1,
        source: PathBuf::from("/media/videos/multi_audio_2tracks.mkv"),
        source_info: None,
        sections: list,
        filters: full_chain(),
        final_multiple: 2,
        profile: builtin_profiles().into_iter().next().unwrap(),
        target: TargetSpec {
            folder: PathBuf::from("/media/out"),
            stem: "clip - 01".into(),
            overwrite: false,
            naming: None,
        },
        metadata: JobMetadata::default(),
    }
}

fn lavfi(c: &mut Criterion) {
    let chain = full_chain();
    let source = Size::new(1920, 1080);
    let mut group = c.benchmark_group("lavfi");
    group.bench_function("render_every_filter", |b| {
        b.iter(|| {
            black_box(&chain).to_lavfi(LavfiTarget::Render {
                source,
                deshake: DeshakePass::Transform {
                    input: Path::new("/data/deshake/job-1.trf"),
                },
                multiple: 2,
            })
        });
    });
    group.bench_function("preview_every_filter", |b| {
        b.iter(|| black_box(&chain).preview_filter(90));
    });
    group.finish();
}

fn command(c: &mut Criterion) {
    let source: MediaInfo = decode(MULTI_AUDIO_JSON).unwrap();
    let dirs = WorkDirs {
        deshake: PathBuf::from("/data/deshake"),
        passlog: PathBuf::from("/data/passlog"),
    };
    let hardware = HardwareContext::default();
    let mut group = c.benchmark_group("command_plan");
    for sections in [1, 10, 100] {
        let job = spec(sections);
        let planned = plan(PlanInput {
            job: JobId(1),
            spec: &job,
            source: &source,
            dirs: &dirs,
            hardware: &hardware,
        });
        assert!(planned.is_ok(), "{sections} sections: {planned:?}");
        group.bench_function(format!("{sections}_sections_every_filter"), |b| {
            b.iter(|| {
                plan(PlanInput {
                    job: JobId(1),
                    spec: black_box(&job),
                    source: &source,
                    dirs: &dirs,
                    hardware: &hardware,
                })
            });
        });
    }
    group.finish();
}

/// `blocks` blocks of `ffmpeg -progress pipe:1`, as ffmpeg writes them.
fn progress_stream(blocks: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(blocks * 12);
    for i in 0..blocks {
        let us = i * 40_000;
        lines.extend([
            format!("frame={i}"),
            "fps=24.87".to_owned(),
            "stream_0_0_q=28.0".to_owned(),
            "bitrate=1834.2kbits/s".to_owned(),
            format!("total_size={}", i * 9_170),
            format!("out_time_us={us}"),
            format!("out_time_ms={us}"),
            format!("out_time=00:00:{:02}.{:06}", us / 1_000_000 % 60, us % 1_000_000),
            "dup_frames=0".to_owned(),
            "drop_frames=0".to_owned(),
            "speed=1.02x".to_owned(),
            "progress=continue".to_owned(),
        ]);
    }
    lines
}

fn progress(c: &mut Criterion) {
    let lines = progress_stream(10_000);
    let mut group = c.benchmark_group("progress");
    group.throughput(Throughput::Elements(u64::try_from(lines.len()).unwrap()));
    group.bench_function("parse_10000_blocks", |b| {
        b.iter(|| {
            let mut parser = ProgressParser::new();
            let mut samples = 0usize;
            for line in &lines {
                if parser.push_line(black_box(line)).is_some() {
                    samples += 1;
                }
            }
            samples
        });
    });
    group.finish();
}

fn probe_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("ffprobe_decode");
    group.bench_function("multi_audio_2tracks", |b| {
        b.iter(|| decode(black_box(MULTI_AUDIO_JSON)));
    });
    group.bench_function("hd_h264_1080p", |b| b.iter(|| decode(black_box(HD_JSON))));
    group.finish();
}

criterion_group!(benches, lavfi, command, progress, probe_json);
criterion_main!(benches);
