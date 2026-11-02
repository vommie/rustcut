use std::path::PathBuf;

use rustcut_core::edit::{Crop, Filter, FilterChain, Section, Size};
use rustcut_core::job::{JobId, JobMetadata, SourceSummary, TargetSpec};
use rustcut_core::render::builtin_profiles;
use rustcut_core::time::FrameRate;

use super::*;

/// 2026-11-02 12:00 UTC.
const NOW: i64 = 1_793_620_800_000;

fn summary(state: JobState) -> JobSummary {
    JobSummary {
        id: JobId(12),
        state,
        attempt: 1,
        name: "Holiday 2".into(),
        target: PathBuf::from("/videos/done/2026/Holiday 2.mkv"),
        source: PathBuf::from("/videos/raw/GX010234.MP4"),
        profile: "AV1 balanced".into(),
        sections: 3,
        duration: Timestamp::from_secs(84),
        note: None,
        created_at: NOW - 60_000,
        started_at: Some(NOW - 300_000),
        finished_at: Some(NOW - 48_000),
        sync: SyncState::Synced,
        relocation: None,
    }
}

fn spec() -> JobSpec {
    let s = |a, b| Section::new(Timestamp::from_secs(a), Timestamp::from_secs(b)).unwrap();
    let mut filters = FilterChain::default();
    filters.set(Filter::Crop(Crop::new(140, 0, 140, 0)));
    filters.set_enabled(FilterKind::Crop, true);
    JobSpec {
        version: 1,
        source: PathBuf::from("/videos/raw/GX010234.MP4"),
        source_info: Some(SourceSummary {
            size: Size::new(1920, 1080),
            frame_rate: FrameRate::new(25, 1),
            duration: Timestamp::from_secs(600),
            audio_tracks: 1,
        }),
        sections: vec![s(190, 220), s(300, 330), s(400, 424)].into(),
        filters,
        final_multiple: 2,
        profile: builtin_profiles().remove(0),
        target: TargetSpec {
            folder: PathBuf::from("/videos/done/2026"),
            stem: "Holiday 2".into(),
            overwrite: false,
            naming: None,
        },
        metadata: JobMetadata {
            tags: vec![3, 1, 2, 99],
            rating: Some(4),
            source_hash: None,
            without_rustview: false,
        },
    }
}

fn names(id: u32) -> Option<String> {
    match id {
        1 => Some("beach".into()),
        2 => Some("Family".into()),
        3 => Some("Drone".into()),
        _ => None,
    }
}

fn card(row: &JobSummary, progress: Option<&JobProgress>, size: Option<u64>, spec: Option<&JobSpec>) -> Facts {
    facts(&Input {
        row,
        progress,
        size,
        spec,
        tag_name: &names,
        in_line: Some(3),
        now_ms: NOW,
    })
}

#[test]
fn a_finished_job_shows_its_size_and_render_time() {
    let spec = spec();
    let f = card(&summary(JobState::Finished), None, Some(432_013_312), Some(&spec));
    assert_eq!((f.name.as_str(), f.state.as_str()), ("Holiday 2.mkv", "Finished"));
    assert_eq!(f.rating, Some(4));
    assert_eq!(f.tags, ["beach", "Drone", "Family"], "sorted, unknown ids left out");
    assert_eq!(f.hint, "");
    assert!(f.progress.is_none() && f.error.is_none());
    let texts: Vec<(&str, &str)> = f.rows.iter().map(|r| (r.text.as_str(), r.right.as_str())).collect();
    assert_eq!(texts[0], ("/videos/done/2026/Holiday 2.mkv", ""));
    assert_eq!(texts[1], ("/videos/raw/GX010234.MP4", ""));
    assert_eq!(texts[2], ("1:24 · 3 sections", "1920×800"), "the crop's output size");
    assert_eq!(texts[3].1, "412 MiB");
    assert!(texts[4].0.starts_with("Rendered in 4:12"), "{:?}", texts[4]);
    assert_eq!(f.rows[5].text, "In RustView");
    assert_eq!(f.rows[5].icon, Some(Icon::CardRustviewWritten));
    assert!(f.rows[2].filters && f.rows[0].path);
    assert_eq!(f.filters, [FilterKind::Crop]);
    assert!(f.footer.starts_with("#12 · created "), "{}", f.footer);
    assert!(!f.footer.contains("attempt"), "the first attempt is not named");
}

#[test]
fn a_render_shows_its_progress_and_estimate() {
    let row = summary(JobState::Rendering { pass: 2, passes: 2 });
    let p = JobProgress {
        id: row.id,
        pass: 2,
        passes: 2,
        done: Timestamp::from_secs(60),
        total: Timestamp::from_secs(84),
        fps: 48.0,
        speed: 1.9,
        size_bytes: 120_000_000,
        eta_secs: Some(130),
    };
    let f = card(&row, Some(&p), None, None);
    let progress = f.progress.unwrap();
    assert_eq!(progress.left, "Pass 2/2 · 48 fps · 1.9×");
    assert_eq!(
        progress.right, "~2:10 left",
        "the estimate stands in the output row only"
    );
    assert!(progress.fraction > 0.8 && progress.fraction < 0.9);
    assert_eq!(f.hint, "", "no hint while the spec is read");
    assert!(
        !f.rows.iter().any(|r| r.icon == Some(Icon::CardTime)),
        "no time row: the first start may be days old"
    );
    assert_eq!(
        f.rows.iter().filter(|r| r.right.starts_with('~')).count(),
        1,
        "the size estimate"
    );
    let slow = JobProgress { fps: 0.4, ..p };
    assert!(
        card(&row, Some(&slow), None, None)
            .progress
            .unwrap()
            .left
            .contains("0.4 fps")
    );
}

#[test]
fn a_failure_names_its_reason_and_a_move_its_new_place() {
    let mut row = summary(JobState::Failed {
        reason: "Encoder libsvtav1: invalid parameter".into(),
    });
    row.attempt = 2;
    row.note = Some("Retried".into());
    row.relocation = Some(PathBuf::from("/archive/Holiday 2.mkv"));
    row.sync = SyncState::Failed { error: "gone".into() };
    let mut spec = spec();
    spec.metadata.rating = None;
    spec.metadata.tags.clear();
    let f = card(&row, None, None, Some(&spec));
    assert_eq!(f.error.as_deref(), Some("Encoder libsvtav1: invalid parameter"));
    assert_eq!(f.hint, "No rating, no tags");
    let moving = &f.rows[1];
    assert_eq!(
        (moving.text.as_str(), moving.tone),
        ("/archive/Holiday 2.mkv", Tone::Accent)
    );
    assert!(f.rows.iter().any(|r| r.text.starts_with("Failed after 4:12")));
    assert_eq!(f.rows.last().unwrap().tone, Tone::Error, "RustView failed");
    assert!(f.footer.ends_with(" · attempt 2"));
    assert_eq!(f.note.as_deref(), Some("Retried"));
}

#[test]
fn a_waiting_job_names_its_place_in_line() {
    let mut row = summary(JobState::Waiting);
    (row.started_at, row.finished_at, row.sections) = (None, None, 1);
    let mut spec = spec();
    spec.metadata.without_rustview = true;
    spec.source_info = None;
    let f = card(&row, None, None, Some(&spec));
    assert!(f.rows.iter().any(|r| r.text == "Waiting · #3 in line"));
    assert_eq!(f.rows[2].text, "1:24 · 1 section");
    assert_eq!(f.rows[2].right, "", "no source size, no output size");
    assert_eq!(f.rows.last().unwrap().text, "Saved without RustView");
    let rows = [
        summary(JobState::Finished),
        JobSummary {
            id: JobId(3),
            ..row.clone()
        },
        JobSummary { id: JobId(4), ..row },
    ];
    assert_eq!(in_line(&rows, JobId(4)), Some(2));
    assert_eq!(in_line(&rows, JobId(12)), None);
}

#[test]
fn tags_without_names_are_counted() {
    let spec = spec();
    let f = facts(&Input {
        row: &summary(JobState::Finished),
        progress: None,
        size: None,
        spec: Some(&spec),
        tag_name: &|_| None,
        in_line: None,
        now_ms: NOW,
    });
    assert!(f.tags.is_empty());
    assert_eq!(f.hint, "4 tags", "RustView's tree is not there");
}

#[test]
fn times_of_another_day_carry_the_date() {
    assert_eq!(local_time(NOW, NOW).len(), 5);
    assert_eq!(local_time(NOW - 3 * 86_400_000, NOW).len(), 16);
}
