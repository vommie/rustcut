//! The render engine against the real ffmpeg and PyCutRv's test media (`~/claude/pycutrv-testenv/golden/source/`, or
//! `RUSTCUT_GOLDEN_MEDIA`): several sections with two audio tracks, a rotated source with a crop, an odd size, a source
//! whose audio ends early, a hostile file name, two-pass deshake, cancel, pause, failures that must leave an existing
//! target alone, and the encoder capability check. ffmpeg (with libx264, libsvtav1, vid.stab) and the media are
//! required; the tests fail with what is missing.
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

use rustcut_core::edit::{Crop, DeshakeParams, Filter, FilterChain, Rotation, Section, SectionList};
use rustcut_core::job::{JobId, JobMetadata, JobSpec, TargetSpec};
use rustcut_core::media::{ProbeOptions, probe};
use rustcut_core::render::{
    Availability, BitDepth, Container, Control, HardwareContext, JobLog, PassError, PassKind, RateControl, RenderEnv,
    RenderError, RenderPriority, RenderProfile, RenderUpdate, VideoEncoder, WorkDirs, builtin_profiles,
    detect_capabilities, render_job,
};
use rustcut_core::time::Timestamp;
use tokio::sync::watch;

fn golden(name: &str) -> PathBuf {
    let dir = std::env::var_os("RUSTCUT_GOLDEN_MEDIA").map_or_else(
        || Path::new(&std::env::var_os("HOME").expect("HOME is set")).join("claude/pycutrv-testenv/golden/source"),
        PathBuf::from,
    );
    let path = dir.join(name);
    assert!(
        path.is_file(),
        "the test medium is missing: {} (set RUSTCUT_GOLDEN_MEDIA to the folder with PyCutRv's golden sources)",
        path.display()
    );
    path
}

/// A fast x264 profile (ultrafast, CRF 30, 8-bit) with AAC audio in `container`.
fn fast_profile(container: Container) -> RenderProfile {
    let mut p = builtin_profiles()
        .into_iter()
        .find(|p| p.id == "x264-fast-share")
        .unwrap();
    p.video.encoder = VideoEncoder::X264;
    p.video.preset = Some("ultrafast".into());
    p.video.rate_control = RateControl::Quality { value: 30 };
    p.video.bit_depth = BitDepth::Eight;
    p.video.tune = None;
    p.video.profile = None;
    p.container = container;
    p
}

struct Fixture {
    dir: tempfile::TempDir,
    env: RenderEnv,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("out")).unwrap();
        let env = RenderEnv {
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
        Self { dir, env }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn spec(&self, source: &str, sections: &[(i64, i64)], stem: &str) -> JobSpec {
        let list: SectionList = sections
            .iter()
            .map(|&(a, b)| Section::new(Timestamp::from_millis(a), Timestamp::from_millis(b)).unwrap())
            .collect::<Vec<_>>()
            .into();
        JobSpec {
            version: 1,
            source: golden(source),
            source_info: None,
            sections: list,
            filters: FilterChain::default(),
            profile: fast_profile(Container::Mkv),
            target: TargetSpec {
                folder: self.out(),
                stem: stem.into(),
                overwrite: false,
                naming: None,
            },
            metadata: JobMetadata::default(),
        }
    }

    /// Everything left in the output folder and the work folders besides `keep`.
    fn leftovers(&self, keep: &[&Path]) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for dir in [self.out(), self.env.dirs.deshake.clone(), self.env.dirs.passlog.clone()] {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                found.extend(
                    entries
                        .map(|e| e.unwrap().path())
                        .filter(|p| !keep.contains(&p.as_path())),
                );
            }
        }
        found
    }
}

type Updates = Arc<Mutex<Vec<RenderUpdate>>>;

async fn render(
    fx: &Fixture,
    spec: &JobSpec,
    control: watch::Receiver<Control>,
    updates: Updates,
) -> (Result<rustcut_core::render::RenderedFile, RenderError>, JobLog) {
    let mut log = JobLog::new();
    let result = render_job(JobId(7), spec, &fx.env, control, &mut log, move |u| {
        updates.lock().unwrap().push(u);
    })
    .await;
    (result, log)
}

async fn render_simply(
    fx: &Fixture,
    spec: &JobSpec,
) -> (Result<rustcut_core::render::RenderedFile, RenderError>, JobLog) {
    let (_tx, rx) = watch::channel(Control::Run);
    render(fx, spec, rx, Updates::default()).await
}

fn millis(t: Option<Timestamp>) -> i64 {
    t.unwrap().as_micros() / 1000
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sections_keep_both_audio_tracks_and_their_languages() {
    let fx = Fixture::new();
    let spec = fx.spec("multi_audio_2tracks.mkv", &[(500, 2_500), (5_000, 6_000)], "two tracks");
    let updates = Updates::default();
    let (_tx, rx) = watch::channel(Control::Run);
    let (result, log) = render(&fx, &spec, rx, Arc::clone(&updates)).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    assert_eq!(done.path, fx.out().join("two tracks.mkv"));
    assert!(done.bytes > 0);
    let info = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    let languages: Vec<Option<String>> = info.audio_streams().map(|(s, _)| s.language.clone()).collect();
    assert_eq!(languages, [Some("eng".to_owned()), Some("deu".to_owned())]);
    assert!((millis(info.duration()) - 3_000).abs() < 200, "{:?}", info.duration());
    assert_eq!(
        fx.leftovers(&[&done.path]),
        Vec::<PathBuf>::new(),
        "no temp file is left"
    );
    let updates = updates.lock().unwrap();
    assert!(matches!(
        updates.first(),
        Some(RenderUpdate::Pass {
            pass: 1,
            passes: 1,
            kind: PassKind::Encode
        })
    ));
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::Progress(p) if p.done > Timestamp::ZERO))
    );
    assert!(updates.contains(&RenderUpdate::Verifying));
    assert!(log.to_text().contains("[Render] Verified"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rotated_source_is_cropped_in_its_shown_orientation() {
    let fx = Fixture::new();
    // Stored 640x360 with a display rotation: shown (and filtered) as 360x640.
    let mut spec = fx.spec("rotated_90_metadata.mp4", &[(1_000, 3_000)], "rotated");
    spec.filters.set(Filter::Crop(Crop::new(100, 20, 41, 0)));
    spec.filters.set_enabled(rustcut_core::edit::FilterKind::Crop, true);
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    let info = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    let (_, video) = info.primary_video().unwrap();
    // 360 - 20 = 340 wide, 640 - 141 = 499 high, evened to 498.
    assert_eq!(video.oriented_size(), Some((340, 498)));
    assert_eq!(video.rotation, 0, "the output carries no rotation of its own");
    assert!(!info.has_audio());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn odd_sizes_audio_that_ends_early_and_hostile_names() {
    let fx = Fixture::new();
    let mut spec = fx.spec("odd_size_853x479_yuv444.mp4", &[(0, 2_000)], "odd");
    spec.filters.set(Filter::Rotate {
        rotation: Rotation::Clockwise,
    });
    spec.filters.set_enabled(rustcut_core::edit::FilterKind::Rotate, true);
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    let info = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    assert_eq!(info.primary_video().unwrap().1.oriented_size(), Some((478, 852)));

    // Sections after the end of the audio: concat pads them with silence, the video stays complete.
    let spec = fx.spec(
        "audio_ends_early_3s.mp4",
        &[(1_000, 2_000), (10_000, 12_000)],
        "silent tail",
    );
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    let info = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    assert!((millis(info.duration()) - 3_000).abs() < 300, "{:?}", info.duration());
    assert_eq!(info.audio_streams().count(), 1);

    let mut spec = fx.spec(
        "Ärger über Größe [test] (1).mp4",
        &[(1_000, 2_000)],
        "-a: 'b' [c], d; e=f",
    );
    spec.profile = fast_profile(Container::Mp4);
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    assert_eq!(done.path, fx.out().join("-a: 'b' [c], d; e=f.mp4"));
    assert!(probe(&done.path, &ProbeOptions::default()).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deshake_runs_two_passes_and_removes_its_transform_file() {
    let fx = Fixture::new();
    let mut spec = fx.spec("shaky_h264_640x360.mp4", &[(2_000, 4_000)], "steady");
    spec.filters.set(Filter::Deshake(DeshakeParams::default()));
    spec.filters.set_enabled(rustcut_core::edit::FilterKind::Deshake, true);
    let updates = Updates::default();
    let (_tx, rx) = watch::channel(Control::Run);
    let (result, log) = render(&fx, &spec, rx, Arc::clone(&updates)).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    let passes: Vec<(u8, PassKind)> = updates
        .lock()
        .unwrap()
        .iter()
        .filter_map(|u| match u {
            RenderUpdate::Pass { pass, kind, .. } => Some((*pass, *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(passes, [(1, PassKind::DeshakeAnalysis), (2, PassKind::Encode)]);
    assert!(log.to_text().contains("vidstabdetect=result="));
    assert_eq!(
        fx.leftovers(&[&done.path]),
        Vec::<PathBuf>::new(),
        "the .trf file is removed"
    );
    assert!(fx.env.dirs.deshake.is_dir(), "the work folder stays");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_two_pass_encode_uses_and_removes_its_statistics() {
    let fx = Fixture::new();
    let mut spec = fx.spec("shaky_h264_640x360.mp4", &[(0, 2_000)], "two pass");
    spec.profile.video.rate_control = RateControl::TwoPass { kbps: 800 };
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    assert!(log.to_text().contains("-pass 2"));
    assert_eq!(fx.leftovers(&[&done.path]), Vec::<PathBuf>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_and_failure_never_touch_an_existing_target() {
    let fx = Fixture::new();
    // Three minutes at a slow preset: long enough to cancel in the middle.
    let mut spec = fx.spec("long_h264_320x180.mp4", &[(0, 180_000)], "kept");
    spec.profile.video.preset = Some("veryslow".into());
    spec.target.overwrite = true;
    let target = spec.target_path();
    std::fs::write(&target, "the old file").unwrap();

    let (tx, rx) = watch::channel(Control::Run);
    let updates = Updates::default();
    let watcher = {
        let updates = Arc::clone(&updates);
        tokio::spawn(async move {
            // Pause at the first progress, check that the output stops growing, resume, then cancel.
            while !updates
                .lock()
                .unwrap()
                .iter()
                .any(|u| matches!(u, RenderUpdate::Progress(_)))
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tx.send_replace(Control::Pause);
            tokio::time::sleep(Duration::from_millis(600)).await;
            let progress = || {
                updates
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|u| matches!(u, RenderUpdate::Progress(_)))
                    .count()
            };
            let before = progress();
            tokio::time::sleep(Duration::from_millis(1_200)).await;
            let after = progress();
            tx.send_replace(Control::Run);
            tokio::time::sleep(Duration::from_millis(300)).await;
            tx.send_replace(Control::Cancel);
            (before, after, tx)
        })
    };
    let (result, log) = render(&fx, &spec, rx, Arc::clone(&updates)).await;
    let (before, after, _tx) = watcher.await.unwrap();
    let error = result.unwrap_err();
    assert!(error.is_canceled(), "{error}\n{}", log.to_text());
    assert_eq!(before, after, "no progress while paused");
    let paused: Vec<RenderUpdate> = updates
        .lock()
        .unwrap()
        .iter()
        .filter(|u| matches!(u, RenderUpdate::Paused(_)))
        .copied()
        .collect();
    assert_eq!(paused, [RenderUpdate::Paused(true), RenderUpdate::Paused(false)]);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "the old file");
    assert_eq!(fx.leftovers(&[&target]), Vec::<PathBuf>::new(), "the temp file is gone");

    // An encoder that refuses its settings (baseline H.264 has no 10-bit): the job fails with ffmpeg's reason.
    let mut spec = fx.spec("shaky_h264_640x360.mp4", &[(0, 1_000)], "kept");
    spec.target.overwrite = true;
    spec.profile.video.bit_depth = BitDepth::Ten;
    spec.profile.video.profile = Some("baseline".into());
    let (result, log) = render_simply(&fx, &spec).await;
    match result {
        Err(RenderError::Pass {
            kind: PassKind::Encode,
            error: PassError::Failed { message, .. },
        }) => assert!(!message.is_empty(), "{}", log.to_text()),
        other => panic!("{other:?}\n{}", log.to_text()),
    }
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "the old file");
    assert_eq!(fx.leftovers(&[&target]), Vec::<PathBuf>::new());

    // Without "overwrite" an existing target stops the job before anything runs.
    spec.target.overwrite = false;
    spec.profile = fast_profile(Container::Mkv);
    let (result, log) = render_simply(&fx, &spec).await;
    assert_eq!(result.unwrap_err(), RenderError::TargetExists(target.clone()));
    assert!(!log.to_text().contains("ffmpeg -hide_banner"));
}

/// A clip that changes from 30 to 10 fps after two seconds (like a screen recorder writes it).
fn variable_frame_rate_clip(dir: &Path) -> PathBuf {
    let path = dir.join("vfr.mkv");
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=160x120:r=30:d=4"])
        .args([
            "-vf",
            "select='lt(n\\,60)+not(mod(n\\,3))'",
            "-fps_mode",
            "vfr",
            "-c:v",
            "ffv1",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg is required (sudo pacman -S ffmpeg)");
    assert!(status.success());
    path
}

fn frame_count(path: &Path) -> u64 {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe is required (sudo pacman -S ffmpeg)");
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_variable_frame_rate_keeps_its_frames_in_mp4() {
    let fx = Fixture::new();
    let source = variable_frame_rate_clip(fx.dir.path());
    // 1 s at 30 fps plus 1 s at 10 fps: 40 frames (a constant 30 fps output, as older ffmpeg builds wrote MP4 by default,
    // would have 60).
    let mut spec = fx.spec("multi_audio_2tracks.mkv", &[(1_000, 3_000)], "vfr");
    spec.source = source;
    spec.profile = fast_profile(Container::Mp4);
    let (result, log) = render_simply(&fx, &spec).await;
    let done = result.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    assert_eq!(frame_count(&done.path), 40);
    let info = probe(&done.path, &ProbeOptions::default()).await.unwrap();
    assert!((millis(info.duration()) - 2_000).abs() < 150, "{:?}", info.duration());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unusable_sources_and_folders_fail_early() {
    let fx = Fixture::new();
    let spec = fx.spec("multi_audio_2tracks.mkv", &[(0, 1_000)], "x");
    // A target folder's path taken by a file fails early; a missing folder is made on the fly (P9.7).
    let mut taken = spec.clone();
    taken.target.folder = fx.dir.path().join("a file, not a folder");
    std::fs::write(&taken.target.folder, b"x").unwrap();
    assert!(matches!(
        render_simply(&fx, &taken).await.0,
        Err(RenderError::TargetFolder(folder, _)) if folder == taken.target.folder
    ));
    let mut fresh = spec.clone();
    fresh.target.folder = fx.dir.path().join("new").join("deep folder");
    let (made, log) = render_simply(&fx, &fresh).await;
    made.unwrap_or_else(|e| panic!("{e}\n{}", log.to_text()));
    assert!(fresh.target.folder.is_dir(), "the folder was made");
    let mut broken = spec.clone();
    broken.source = fx.dir.path().join("does not exist.mp4");
    assert!(matches!(
        render_simply(&fx, &broken).await.0,
        Err(RenderError::Source(_))
    ));
    let mut past = spec;
    past.sections = vec![Section::new(Timestamp::from_secs(100), Timestamp::from_secs(120)).unwrap()].into();
    assert!(matches!(render_simply(&fx, &past).await.0, Err(RenderError::Plan(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_capability_check_finds_the_software_encoders() {
    let caps = detect_capabilities(None)
        .await
        .expect("ffmpeg is required (sudo pacman -S ffmpeg)");
    for encoder in [
        VideoEncoder::SvtAv1,
        VideoEncoder::X265,
        VideoEncoder::X264,
        VideoEncoder::Aom,
    ] {
        assert!(caps.works(encoder), "{encoder:?}: {}", caps.summary());
    }
    assert!(caps.vidstab, "vid.stab is required (sudo pacman -S vid.stab)");
    assert_eq!(caps.encoders.len(), VideoEncoder::ALL.len());
    // Hardware encoders: listed by this ffmpeg build, offered only when the test encode passed. In a VM without a GPU
    // they all fail, and none of that is an error of the check.
    for status in &caps.encoders {
        let backend = status.encoder.spec().backend;
        if backend != rustcut_core::render::EncoderBackend::Software && status.availability == Availability::Works {
            eprintln!("hardware encoder works here: {:?}", status.encoder);
        }
    }
    assert_eq!(
        caps.hardware.vaapi_device.is_some(),
        caps.encoders.iter().any(|s| {
            s.encoder.spec().backend == rustcut_core::render::EncoderBackend::Vaapi
                && s.availability == Availability::Works
        })
    );
    eprintln!("{}", caps.summary());
}
