//! The encoding system against the real ffmpeg (M2.5): the check of a profile by the encoder (every built-in passes,
//! ignored codec parameters and refused settings are reported), and the probe render around a playhead with deshake and
//! a crop, written into the probe folder and verified like a job, with a size estimate from its progress. ffmpeg (with
//! libx264, libx265, libsvtav1, vid.stab) and PyCutRv's test media are required; the tests fail with what is missing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustcut_core::edit::{Crop, DeshakeParams, Filter, FilterChain, FilterKind};
use rustcut_core::media::{ProbeOptions, probe};
use rustcut_core::render::{
    CodecParam, Control, HardwareContext, JobLog, PROBE_JOB, ProbeInput, ProfileField, RenderEnv, RenderPriority,
    RenderProfile, RenderUpdate, Severity, VideoEncoder, WorkDirs, builtin_profiles, check_profile, clear_probe_dir,
    estimate_size, probe_dir, probe_env, probe_job, render_job,
};
use rustcut_core::time::Timestamp;
use tokio::sync::watch;

fn golden(name: &str) -> PathBuf {
    let dir = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    let path = dir.join(name);
    assert!(
        path.is_file(),
        "the test medium is missing: {} (set RUSTCUT_GOLDEN_MEDIA or place the PyCutRv test media next to the repository)",
        path.display()
    );
    path
}

fn profile(id: &str) -> RenderProfile {
    builtin_profiles().into_iter().find(|p| p.id == id).unwrap()
}

async fn check(p: &RenderProfile) -> Vec<rustcut_core::render::ProfileIssue> {
    let dir = tempfile::tempdir().unwrap();
    check_profile(Path::new("ffmpeg"), p, &HardwareContext::default(), dir.path())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_built_in_profile_passes_the_check() {
    for p in builtin_profiles() {
        let issues = check(&p).await;
        assert_eq!(issues, vec![], "{}: not even a warning", p.id);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn ignored_codec_parameters_are_errors() {
    for (id, encoder) in [
        ("x264-fast-share", VideoEncoder::X264),
        ("x265-quality", VideoEncoder::X265),
        ("av1-balanced", VideoEncoder::SvtAv1),
    ] {
        let mut p = profile(id);
        assert_eq!(p.video.encoder, encoder);
        p.video.preset = encoder.spec().speeds.map(|s| s[0].to_owned());
        p.video.codec_params.push(CodecParam::new("no-such-key", "1"));
        let issues = check(&p).await;
        assert!(
            issues
                .iter()
                .any(|i| i.field == ProfileField::CodecParams && i.severity == Severity::Error),
            "{id}: {issues:?}"
        );
        // A known key passes.
        p.video.codec_params = vec![CodecParam::new(
            if encoder == VideoEncoder::SvtAv1 {
                "enable-overlays"
            } else {
                "ref"
            },
            "1",
        )];
        let issues = check(&p).await;
        assert!(
            issues.iter().all(|i| i.field != ProfileField::CodecParams),
            "{id}: {issues:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_setting_names_the_cause() {
    // Static validation cannot know x265's limits; x265 refuses to open with this one.
    let mut p = profile("x265-quality");
    p.video.preset = Some("veryfast".into());
    p.video.codec_params = vec![CodecParam::new("ctu", "7")];
    let issues = check(&p).await;
    let errors: Vec<_> = issues.iter().filter(|i| i.severity == Severity::Error).collect();
    assert_eq!(errors.len(), 1, "{issues:?}");
    assert_eq!(errors[0].field, ProfileField::Encoder);
    assert_eq!(
        errors[0].message,
        "H.265 / HEVC (x265) refuses these settings: x265: max cu size must be 16, 32, or 64"
    );

    // A level x264 does not know passes the static check (it looks like a level) but is ignored by x264.
    let mut p = profile("x264-compatible");
    p.video.level = Some("9.9".into());
    assert!(p.is_usable());
    let issues = check(&p).await;
    assert!(
        issues
            .iter()
            .any(|i| i.field == ProfileField::Level && i.message.contains("level = 9.9")),
        "{issues:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_probe_render_with_deshake_and_crop() {
    let dir = tempfile::tempdir().unwrap();
    let folder = probe_dir(dir.path());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("probe.mkv"), "an older probe of another container").unwrap();
    clear_probe_dir(&folder).await;

    let source = golden("shaky_h264_640x360.mp4");
    let info = probe(&source, &ProbeOptions::default()).await.unwrap();
    let duration = info.duration().unwrap();
    let mut filters = FilterChain::default();
    filters.set(Filter::Crop(Crop::new(20, 10, 20, 10)));
    filters.set_enabled(FilterKind::Crop, true);
    filters.set(Filter::Deshake(DeshakeParams::default()));
    filters.set_enabled(FilterKind::Deshake, true);
    let mut p = profile("x264-fast-share");
    p.video.preset = Some("medium".into());
    let job = probe_job(&ProbeInput {
        source: &source,
        duration,
        playhead: Timestamp::from_secs(3),
        seconds: 4,
        filters: &filters,
        final_multiple: 2,
        profile: &p,
        exact: false,
        job_duration: duration,
        folder: &folder,
    })
    .unwrap();
    assert_eq!(job.window.start(), Timestamp::from_secs(2));
    assert_eq!(
        job.spec.profile.video.preset.as_deref(),
        Some("veryfast"),
        "the fast preset"
    );

    let jobs_env = RenderEnv {
        ffmpeg: PathBuf::from("ffmpeg"),
        probe: ProbeOptions::default(),
        dirs: WorkDirs {
            deshake: dir.path().join("data/deshake"),
            passlog: dir.path().join("data/passlog"),
        },
        hardware: HardwareContext::default(),
        priority: RenderPriority::default(),
        stall_timeout: Some(Duration::from_secs(120)),
    };
    let env = probe_env(&jobs_env, &folder);
    let updates = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&updates);
    let (_control, rx) = watch::channel(Control::Run);
    let mut log = JobLog::new();
    let done = render_job(PROBE_JOB, &job.spec, &env, rx, &mut log, move |u| {
        seen.lock().unwrap().push(u);
    })
    .await
    .unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));

    assert_eq!(done.path, folder.join("probe.mp4"));
    let out = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    let length = out.duration().unwrap().as_secs_f64();
    assert!((length - 4.0).abs() < 0.3, "{length}");
    let (_, video) = out.primary_video().unwrap();
    assert_eq!((video.width, video.height), (Some(620), Some(320)), "cropped");
    let leftovers: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        leftovers,
        ["probe.mp4"],
        "the transform file is gone, the older probe too"
    );
    assert!(!jobs_env.dirs.deshake.exists(), "the jobs' work folders are not used");

    let passes = updates
        .lock()
        .unwrap()
        .iter()
        .filter(|u| matches!(u, RenderUpdate::Pass { .. }))
        .count();
    assert_eq!(passes, 2, "deshake analysis and encode");
    let estimates: Vec<u64> = updates
        .lock()
        .unwrap()
        .iter()
        .filter_map(|u| match u {
            RenderUpdate::Progress(p) => estimate_size(p),
            _ => None,
        })
        .collect();
    if let Some(last) = estimates.last() {
        let ratio = *last as f64 / done.bytes as f64;
        assert!((0.5..2.0).contains(&ratio), "estimate {last} for {} bytes", done.bytes);
    }
}
