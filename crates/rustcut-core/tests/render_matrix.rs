//! The render integration matrix (P6, M6.2): the render engine with the real ffmpeg over PyCutRv's test media, with
//! every built-in profile, every software encoder, the rate controls, every container with the audio codecs the profile
//! check lets through, every filter and every test video (cases in `render_matrix/cases.rs`). Each render is verified by
//! the engine and then probed: the video codec, the size the filter chain predicts, the audio tracks and their codec.
//!
//! Gated: it takes minutes, so it is ignored unless `rc-testenv test matrix [--full]` runs it. Variables:
//! `RUSTCUT_GOLDEN_MEDIA` the media folder (rc-testenv: the environment's `source/`), `RUSTCUT_MATRIX=full` the full
//! cross product (every profile on every video, every filter with every profile; hours), `RUSTCUT_MATRIX_JOBS`
//! renders at once (default 2), `RUSTCUT_MATRIX_FILTER` only cases whose group or name contains it,
//! `RUSTCUT_MATRIX_REPORT` a file for the Markdown table of results.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

#[path = "render_matrix/cases.rs"]
mod cases;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cases::{Case, Expect};
use rustcut_core::edit::{Section, SectionList, Size};
use rustcut_core::job::{JobId, JobMetadata, JobSpec, TargetSpec};
use rustcut_core::media::{MediaInfo, ProbeOptions, probe};
use rustcut_core::render::{
    AudioCodec, CodecFamily, Control, EncoderBackend, HardwareContext, JobLog, RenderEnv, RenderPriority, WorkDirs,
    detect_capabilities, render_job,
};
use rustcut_core::time::Timestamp;
use tokio::sync::{Semaphore, watch};

fn media_dir() -> PathBuf {
    std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || Path::new(&std::env::var_os("HOME").expect("HOME is set")).join("claude/pycutrv-testenv/golden/source"),
        PathBuf::from,
    )
}

/// What one case gave.
struct Row {
    case: Case,
    secs: f64,
    outcome: Result<String, String>,
}

fn family_codec(family: CodecFamily) -> &'static str {
    match family {
        CodecFamily::Av1 => "av1",
        CodecFamily::Hevc => "hevc",
        CodecFamily::H264 => "h264",
    }
}

fn audio_codec_name(codec: AudioCodec, source: &MediaInfo) -> Option<String> {
    Some(match codec {
        AudioCodec::Opus => "opus".into(),
        AudioCodec::Aac => "aac".into(),
        AudioCodec::Mp3 => "mp3".into(),
        AudioCodec::Vorbis => "vorbis".into(),
        AudioCodec::Flac => "flac".into(),
        AudioCodec::Copy => source.audio_streams().next()?.0.codec.clone()?,
    })
}

/// Compares the output with what the case asked for; `Ok` is the report's description of the file.
fn check_output(case: &Case, source: &MediaInfo, output: &MediaInfo, bytes: u64) -> Result<String, String> {
    let mut problems = Vec::new();
    let (stream, video) = output.primary_video().ok_or("the output has no video")?;
    let codec = stream.codec.clone().unwrap_or_default();
    let wanted = family_codec(case.profile.video.encoder.spec().family);
    if codec != wanted {
        problems.push(format!("video codec {codec}, wanted {wanted}"));
    }
    let size = video.oriented_size().ok_or("the output has no size")?;
    let (_, source_video) = source.primary_video().ok_or("the source has no video")?;
    let (w, h) = source_video.oriented_size().ok_or("the source has no size")?;
    // The render evens the chain's size (the encoders need even sizes), rounding down like the render's guard.
    match case.filters.output_size(Size::new(w, h)) {
        Ok(expected) if (expected.width & !1, expected.height & !1) != size => problems.push(format!(
            "size {}x{}, the chain predicts {}x{} (evened)",
            size.0,
            size.1,
            expected.width & !1,
            expected.height & !1
        )),
        Ok(_) => {}
        Err(error) => problems.push(format!("the chain predicts no size: {error}")),
    }
    let tracks = output.audio_streams().count();
    let source_tracks = source.audio_streams().count();
    if tracks != source_tracks {
        problems.push(format!("{tracks} audio tracks, the source has {source_tracks}"));
    }
    if let Some(wanted) = (source_tracks > 0)
        .then(|| audio_codec_name(case.profile.audio.codec, source))
        .flatten()
    {
        for (stream, _) in output.audio_streams() {
            let got = stream.codec.clone().unwrap_or_default();
            if got != wanted {
                problems.push(format!("audio codec {got}, wanted {wanted}"));
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let duration = output.duration().map_or(0.0, Timestamp::as_secs_f64);
    Ok(format!(
        "{codec} {}x{}, {tracks} audio, {duration:.2} s, {} kB",
        size.0,
        size.1,
        bytes / 1_000
    ))
}

async fn run_case(case: Case, media: PathBuf, sources: Arc<HashMap<String, Option<MediaInfo>>>) -> Row {
    let started = Instant::now();
    let outcome = render_case(&case, &media, &sources).await;
    let outcome = match (case.expect, outcome) {
        (Expect::Renders | Expect::Either, Ok(text)) => Ok(text),
        (Expect::Fails, Ok(text)) => Err(format!("rendered, but must fail: {text}")),
        (Expect::Fails | Expect::Either, Err(error)) => Ok(format!("failed as expected: {error}")),
        (Expect::Renders, Err(error)) => Err(error),
    };
    Row {
        case,
        secs: started.elapsed().as_secs_f64(),
        outcome,
    }
}

async fn render_case(
    case: &Case,
    media: &Path,
    sources: &HashMap<String, Option<MediaInfo>>,
) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let env = RenderEnv {
        ffmpeg: PathBuf::from("ffmpeg"),
        probe: ProbeOptions::default(),
        dirs: WorkDirs {
            deshake: dir.path().join("data/deshake"),
            passlog: dir.path().join("data/passlog"),
        },
        hardware: HardwareContext::default(),
        priority: RenderPriority::default(),
        stall_timeout: Some(Duration::from_secs(300)),
    };
    let sections: SectionList = case
        .sections
        .iter()
        .map(|&(a, b)| Section::new(Timestamp::from_millis(a), Timestamp::from_millis(b)).unwrap())
        .collect::<Vec<_>>()
        .into();
    let spec = JobSpec {
        version: 1,
        source: media.join(case.source),
        source_info: None,
        sections,
        filters: case.filters.clone(),
        profile: case.profile.clone(),
        target: TargetSpec {
            folder: out,
            stem: "matrix".into(),
            overwrite: false,
            naming: None,
        },
        metadata: JobMetadata::default(),
    };
    let (_control, rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let done = render_job(JobId(1), &spec, &env, rx, &mut log, |_| {})
        .await
        .map_err(|error| {
            let cause = log.last_error().map(|line| format!(" ({line})")).unwrap_or_default();
            format!("{error}{cause}")
        })?;
    let source = sources
        .get(case.source)
        .and_then(Option::as_ref)
        .ok_or("the source could not be probed, but it rendered")?;
    let output = probe(&done.path, &ProbeOptions::default())
        .await
        .map_err(|error| format!("the output cannot be probed: {error}"))?;
    check_output(case, source, &output, done.bytes)
}

fn report(rows: &[Row], refused: &[String], hardware: &[String], full: bool, total: Duration) -> String {
    let failed = rows.iter().filter(|r| r.outcome.is_err()).count();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Render matrix ({}, {} cases, {} failed, {:.0} s)\n",
        if full { "full" } else { "standard" },
        rows.len(),
        failed,
        total.as_secs_f64()
    );
    let _ = writeln!(
        text,
        "| # | Group | Case | Source | Profile | Filters | Result | Time |"
    );
    let _ = writeln!(text, "|---|---|---|---|---|---|---|---|");
    for (i, row) in rows.iter().enumerate() {
        let result = match &row.outcome {
            Ok(text) => format!("ok: {text}"),
            Err(error) => format!("**FAILED**: {error}"),
        };
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {:.1} s |",
            i + 1,
            row.case.group,
            row.case.name.replace('|', "/"),
            row.case.source,
            row.case.profile.name,
            cases::filter_names(&row.case.filters),
            result.replace('|', "/"),
            row.secs
        );
    }
    if !refused.is_empty() {
        let _ = writeln!(text, "\nRefused by the profile check (not rendered):\n");
        for line in refused {
            let _ = writeln!(text, "- {line}");
        }
    }
    let _ = writeln!(
        text,
        "\nHardware encoders: {}",
        if hardware.is_empty() {
            "none works here".into()
        } else {
            hardware.join(", ")
        }
    );
    text
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "the render matrix takes minutes: rc-testenv test matrix [--full]"]
async fn the_render_matrix() {
    let media = media_dir();
    assert!(
        media.is_dir(),
        "the test media are missing: {} (RUSTCUT_GOLDEN_MEDIA)",
        media.display()
    );
    let full = std::env::var("RUSTCUT_MATRIX").is_ok_and(|v| v == "full");
    let jobs: usize = std::env::var("RUSTCUT_MATRIX_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
        .max(1);
    let only = std::env::var("RUSTCUT_MATRIX_FILTER").ok();
    let caps = detect_capabilities(None)
        .await
        .expect("ffmpeg is required (sudo pacman -S ffmpeg)");
    let hardware: Vec<String> = caps
        .working()
        .filter(|e| e.spec().backend != EncoderBackend::Software)
        .map(|e| e.spec().display_name.to_owned())
        .collect();
    let (mut all, refused) = cases::all(full);
    if let Some(only) = &only {
        all.retain(|c| c.group.contains(only.as_str()) || c.name.contains(only.as_str()));
    }
    for case in &all {
        let encoder = case.profile.video.encoder;
        assert!(
            caps.works(encoder),
            "{encoder:?} does not work here: {}",
            caps.summary()
        );
    }

    let mut sources = HashMap::new();
    for case in &all {
        if !sources.contains_key(case.source) {
            let info = probe(&media.join(case.source), &ProbeOptions::default()).await.ok();
            sources.insert(case.source.to_owned(), info);
        }
    }
    let sources = Arc::new(sources);

    eprintln!(
        "[matrix] {} cases, {jobs} at once, media {}",
        all.len(),
        media.display()
    );
    let started = Instant::now();
    let permits = Arc::new(Semaphore::new(jobs));
    let mut tasks = Vec::new();
    for (i, case) in all.into_iter().enumerate() {
        let permits = Arc::clone(&permits);
        let media = media.clone();
        let sources = Arc::clone(&sources);
        tasks.push(tokio::spawn(async move {
            let _permit = permits.acquire_owned().await.unwrap();
            let row = run_case(case, media, sources).await;
            eprintln!(
                "[matrix] {:>3} {} {} / {} ({:.1} s)",
                i + 1,
                if row.outcome.is_ok() { "ok    " } else { "FAILED" },
                row.case.group,
                row.case.name,
                row.secs
            );
            row
        }));
    }
    let mut rows = Vec::new();
    for task in tasks {
        rows.push(task.await.unwrap());
    }
    let text = report(&rows, &refused, &hardware, full, started.elapsed());
    eprintln!("\n{text}");
    if let Some(path) = std::env::var_os("RUSTCUT_MATRIX_REPORT") {
        std::fs::write(&path, &text).unwrap();
        eprintln!("[matrix] report: {}", PathBuf::from(path).display());
    }
    let failed: Vec<String> = rows
        .iter()
        .filter_map(|r| {
            r.outcome
                .as_ref()
                .err()
                .map(|e| format!("{} / {}: {e}", r.case.group, r.case.name))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{} of {} cases failed:\n{}",
        failed.len(),
        rows.len(),
        failed.join("\n")
    );
}
