//! What the Job panel says about a job (user decision Q-Q1), without Slint: sections of label/value rows built from the
//! queue's row (state, times, RustView sync) and the job's spec (source, output, sections, filters, render settings,
//! tags and rating). Long values are elided by the panel; the full text is the row's tooltip.

use std::fmt::Write as _;
use std::path::Path;

use rustcut_core::edit::{DeinterlaceRate, Filter, FilterChain};
use rustcut_core::job::{JobSpec, JobState};
use rustcut_core::queue::JobSummary;
use rustcut_core::render::{RateControl, RenderProfile};
use rustcut_core::rustview::SyncState;
use rustcut_core::time::Timestamp;

use crate::ui::queue::{clock, title};

/// How a value is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRow {
    pub label: String,
    pub value: String,
    pub tooltip: String,
    pub tone: Tone,
}

impl DetailRow {
    fn new(label: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: label.to_owned(),
            tooltip: value.clone(),
            value,
            tone: Tone::Normal,
        }
    }

    fn muted(label: &str, value: impl Into<String>) -> Self {
        Self {
            tone: Tone::Muted,
            ..Self::new(label, value)
        }
    }

    fn tip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailSection {
    /// Stable id (`ui-state.toml` keeps the folded ones).
    pub id: &'static str,
    pub title: String,
    pub rows: Vec<DetailRow>,
}

/// The ids of every section, in order.
pub const SECTION_IDS: [&str; 7] = ["job", "source", "output", "sections", "filters", "render", "tags"];

/// The facts the queue's row and the panel's bridge add to the spec.
#[derive(Clone, Default)]
pub struct Extras<'a> {
    /// Measured size of the finished output.
    pub size: Option<u64>,
    /// RustView's tag names by id (from the Tag & Rate panel's tree).
    pub tag_name: Option<&'a dyn Fn(u32) -> Option<String>>,
}

/// Every section of the Job panel for `row` and its `spec` (`None` while it is read, or when this version cannot read
/// it).
pub fn details(row: &JobSummary, spec: Option<&JobSpec>, extras: &Extras<'_>) -> Vec<DetailSection> {
    let without_rustview = spec.is_some_and(|s| s.metadata.without_rustview);
    let mut sections = vec![job_section(row, without_rustview)];
    let Some(spec) = spec else {
        return sections;
    };
    sections.push(source_section(spec));
    sections.push(output_section(spec, extras.size));
    sections.push(sections_section(spec));
    sections.push(filters_section(&spec.filters));
    sections.push(render_section(&spec.profile));
    sections.push(tags_section(spec, extras.tag_name));
    sections
}

fn job_section(row: &JobSummary, without_rustview: bool) -> DetailSection {
    let mut rows = vec![DetailRow::new("State", title(row.state.kind()))];
    if let JobState::Failed { reason } = &row.state {
        rows.push(DetailRow {
            tone: Tone::Error,
            ..DetailRow::new("Reason", reason.as_str())
        });
    }
    if let Some(note) = &row.note {
        rows.push(DetailRow::muted("Note", note.as_str()));
    }
    rows.push(DetailRow::new(
        "Number",
        format!("{} (attempt {})", row.id, row.attempt),
    ));
    rows.push(DetailRow::new("Saved", local_time(row.created_at)));
    if let Some(at) = row.started_at {
        rows.push(DetailRow::new("Started", local_time(at)));
    }
    if let Some(at) = row.finished_at {
        let mut value = local_time(at);
        if let Some(started) = row.started_at
            && at >= started
        {
            let _ = write!(value, " (took {})", clock(Timestamp::from_millis(at - started)));
        }
        rows.push(DetailRow::new("Ended", value));
    }
    let (sync, tone) = match &row.sync {
        // The Tag & Rate panel's "Active" switch was off (user decision Q-P8b).
        _ if without_rustview => ("Saved without RustView".to_owned(), Tone::Muted),
        SyncState::NotSynced => ("Not written".to_owned(), Tone::Muted),
        SyncState::Pending => ("Waiting to be written".to_owned(), Tone::Muted),
        SyncState::Synced => ("Written".to_owned(), Tone::Normal),
        SyncState::Failed { error } => (format!("Failed: {error}"), Tone::Error),
        SyncState::CleanedUp => ("Removed again".to_owned(), Tone::Muted),
    };
    rows.push(DetailRow {
        tone,
        ..DetailRow::new("RustView", sync)
    });
    DetailSection {
        id: "job",
        title: "Job".into(),
        rows,
    }
}

fn source_section(spec: &JobSpec) -> DetailSection {
    let mut rows = path_rows(&spec.source);
    if let Some(info) = &spec.source_info {
        let mut video = format!("{}x{}", info.size.width, info.size.height);
        if let Some(rate) = info.frame_rate {
            video.push_str(&format!(" · {:.3} fps", rate.as_f64()).replace(".000 fps", " fps"));
        }
        let _ = write!(video, " · {}", clock(info.duration));
        rows.push(DetailRow::new("Video", video));
        rows.push(DetailRow::new(
            "Audio",
            match info.audio_tracks {
                0 => "No audio".to_owned(),
                1 => "1 track".to_owned(),
                n => format!("{n} tracks"),
            },
        ));
    }
    if let Some(hash) = &spec.metadata.source_hash {
        rows.push(DetailRow::muted("MD5", hash.as_str()));
    }
    DetailSection {
        id: "source",
        title: "Source".into(),
        rows,
    }
}

fn output_section(spec: &JobSpec, size: Option<u64>) -> DetailSection {
    let mut rows = path_rows(&spec.target_path());
    rows.push(DetailRow::new("Length", clock(spec.output_duration())));
    if let Some(bytes) = size {
        rows.push(DetailRow::new(
            "Size",
            humansize::format_size(bytes, humansize::DECIMAL),
        ));
    }
    if spec.target.overwrite {
        rows.push(DetailRow::muted("Existing file", "Replaced after a verified render"));
    }
    DetailSection {
        id: "output",
        title: "Output".into(),
        rows,
    }
}

fn path_rows(path: &Path) -> Vec<DetailRow> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    vec![
        DetailRow::new("File", name).tip(path.display().to_string()),
        DetailRow::muted("Folder", folder),
    ]
}

fn sections_section(spec: &JobSpec) -> DetailSection {
    let mut rows: Vec<DetailRow> = spec
        .sections
        .sections()
        .enumerate()
        .map(|(i, s)| {
            DetailRow::new(
                &(i + 1).to_string(),
                format!("{} – {}  ({})", s.start(), s.end(), s.duration()),
            )
        })
        .collect();
    if rows.len() > 1 {
        rows.push(DetailRow::muted("Total", spec.output_duration().to_string()));
    }
    DetailSection {
        id: "sections",
        title: format!("Sections ({})", spec.sections.len()),
        rows,
    }
}

fn filters_section(chain: &FilterChain) -> DetailSection {
    let mut rows: Vec<DetailRow> = chain
        .active()
        .map(|f| DetailRow::new(f.kind().label(), filter_text(f)))
        .collect();
    if rows.is_empty() {
        rows.push(DetailRow::muted("Filters", "None"));
    }
    DetailSection {
        id: "filters",
        title: "Filters".into(),
        rows,
    }
}

/// The parameters of one filter in a few words.
pub fn filter_text(filter: &Filter) -> String {
    match filter {
        Filter::Deinterlace { deinterlacer, rate } => {
            let rate = match rate {
                DeinterlaceRate::Frame => "one frame per frame",
                DeinterlaceRate::Field => "one frame per field",
            };
            format!("{}, {rate}", deinterlacer.ffmpeg_name())
        }
        Filter::Crop(c) => format!("top {}, right {}, bottom {}, left {}", c.top, c.right, c.bottom, c.left),
        Filter::Deshake(d) => format!(
            "shakiness {}, accuracy {}, smoothing {}{}",
            d.shakiness,
            d.accuracy,
            d.smoothing,
            if d.tripod { ", tripod" } else { "" }
        ),
        Filter::Rotate { rotation } => match rotation {
            rustcut_core::edit::Rotation::Clockwise => "a quarter clockwise".to_owned(),
            rustcut_core::edit::Rotation::CounterClockwise => "a quarter counter-clockwise".to_owned(),
            rustcut_core::edit::Rotation::UpsideDown => "upside down".to_owned(),
        },
        Filter::Mirror(m) => match (m.horizontal, m.vertical) {
            (true, true) => "left-right and top-bottom".to_owned(),
            (true, false) => "left-right".to_owned(),
            (false, true) => "top-bottom".to_owned(),
            (false, false) => "none".to_owned(),
        },
        Filter::Resize {
            width,
            height,
            algorithm,
        } => {
            let side = |v: &Option<u32>| v.map_or_else(|| "auto".to_owned(), |v| v.to_string());
            format!("{} x {}, {}", side(width), side(height), algorithm.ffmpeg_flag())
        }
    }
}

fn render_section(profile: &RenderProfile) -> DetailSection {
    let v = &profile.video;
    let spec = v.encoder.spec();
    let rate = match v.rate_control {
        RateControl::Quality { value } => format!("{} {value}", spec.quality.label),
        RateControl::ConstantQp { qp } => format!("QP {qp}"),
        RateControl::AverageBitrate { kbps } => format!("{kbps} kbit/s"),
        RateControl::TwoPass { kbps } => format!("{kbps} kbit/s, two passes"),
        RateControl::TargetSize { mebibytes } => format!("{mebibytes} MiB, two passes"),
    };
    let mut video = format!("{} · {rate}", spec.display_name);
    if let Some(preset) = v.preset.as_deref().or(spec.default_preset) {
        let label = spec
            .presets
            .iter()
            .find(|p| p.value == preset)
            .map_or(preset, |p| p.label);
        let _ = write!(video, " · {label}");
    }
    if v.bit_depth == rustcut_core::render::BitDepth::Ten {
        video.push_str(" · 10-bit");
    }
    let a = &profile.audio;
    let mut audio = a.codec.spec().display_name.to_owned();
    if a.codec.takes_bitrate()
        && let Some(kbps) = a.bitrate_kbps.or(a.codec.spec().default_bitrate)
    {
        let _ = write!(audio, " · {kbps} kbit/s");
    }
    let mut rows = vec![
        DetailRow::new("Profile", profile.name.as_str()),
        DetailRow::new("Video", video),
        DetailRow::new("Audio", audio),
        DetailRow::new("Container", profile.container.display_name()),
    ];
    if !v.codec_params.is_empty() {
        let params: Vec<String> = v
            .codec_params
            .iter()
            .map(|p| format!("{}={}", p.key, p.value))
            .collect();
        rows.push(DetailRow::muted("Parameters", params.join(":")));
    }
    DetailSection {
        id: "render",
        title: "Codecs".into(),
        rows,
    }
}

fn tags_section(spec: &JobSpec, tag_name: Option<&dyn Fn(u32) -> Option<String>>) -> DetailSection {
    let m = &spec.metadata;
    let names: Vec<String> = m
        .tags
        .iter()
        .map(|id| tag_name.and_then(|f| f(*id)).unwrap_or_else(|| format!("#{id}")))
        .collect();
    let tags = if names.is_empty() {
        DetailRow::muted("Tags", "None")
    } else {
        DetailRow::new("Tags", names.join(", ")).tip(names.join("\n"))
    };
    let rating = match m.rating {
        Some(r) => DetailRow::new("Rating", r.to_string()),
        None => DetailRow::muted("Rating", "None"),
    };
    DetailSection {
        id: "tags",
        title: "Tags and Rating".into(),
        rows: vec![tags, rating],
    }
}

/// Unix milliseconds as local `YYYY-MM-DD HH:MM:SS`.
fn local_time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map_or_else(String::new, |t| {
        t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string()
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustcut_core::edit::{Crop, Section};
    use rustcut_core::job::{JobId, JobMetadata, SourceSummary, TargetSpec};
    use rustcut_core::render::builtin_profiles;
    use rustcut_core::time::FrameRate;

    use super::*;

    fn summary(state: JobState) -> JobSummary {
        JobSummary {
            id: JobId(7),
            state,
            attempt: 2,
            name: "Clip - 01".into(),
            target: PathBuf::from("/out/Clip - 01.mkv"),
            source: PathBuf::from("/in/Clip.mp4"),
            profile: "AV1 balanced".into(),
            sections: 2,
            duration: Timestamp::from_secs(5),
            note: Some("Interrupted".into()),
            created_at: 0,
            started_at: Some(1_000),
            finished_at: Some(66_000),
            sync: SyncState::Failed { error: "no".into() },
        }
    }

    fn spec() -> JobSpec {
        let s = |a, b| Section::new(Timestamp::from_secs(a), Timestamp::from_secs(b)).unwrap();
        let mut filters = FilterChain::default();
        filters.set(Filter::Crop(Crop::new(2, 0, 2, 0)));
        filters.set_enabled(rustcut_core::edit::FilterKind::Crop, true);
        JobSpec {
            version: 1,
            source: PathBuf::from("/in/Clip.mp4"),
            source_info: Some(SourceSummary {
                size: rustcut_core::edit::Size::new(1920, 1080),
                frame_rate: FrameRate::new(25, 1),
                duration: Timestamp::from_secs(60),
                audio_tracks: 2,
            }),
            sections: vec![s(1, 3), s(10, 13)].into(),
            filters,
            profile: builtin_profiles().remove(0),
            target: TargetSpec {
                folder: PathBuf::from("/out"),
                stem: "Clip - 01".into(),
                overwrite: true,
                naming: None,
            },
            metadata: JobMetadata {
                tags: vec![3, 4],
                rating: Some(4),
                source_hash: None,
                without_rustview: false,
            },
        }
    }

    fn find<'a>(sections: &'a [DetailSection], id: &str) -> &'a DetailSection {
        sections.iter().find(|s| s.id == id).unwrap()
    }

    #[test]
    fn every_section_of_a_failed_job() {
        let row = summary(JobState::Failed {
            reason: "ffmpeg refused".into(),
        });
        let name = |id: u32| (id == 3).then(|| "Beach".to_owned());
        let spec = spec();
        let all = details(
            &row,
            Some(&spec),
            &Extras {
                size: Some(2_000_000),
                tag_name: Some(&name),
            },
        );
        assert_eq!(all.iter().map(|s| s.id).collect::<Vec<_>>(), SECTION_IDS);
        let job = find(&all, "job");
        assert_eq!(job.rows[1].tone, Tone::Error);
        assert!(job.rows.iter().any(|r| r.value == "#7 (attempt 2)"));
        assert!(job.rows.iter().any(|r| r.value.ends_with("(took 0:01:05)")));
        assert!(
            job.rows
                .iter()
                .any(|r| r.value == "Failed: no" && r.tone == Tone::Error)
        );
        let source = find(&all, "source");
        assert_eq!(source.rows[2].value, "1920x1080 · 25 fps · 0:01:00");
        assert_eq!(source.rows[3].value, "2 tracks");
        let output = find(&all, "output");
        assert_eq!(output.rows[0].tooltip, "/out/Clip - 01.mkv");
        assert!(output.rows.iter().any(|r| r.value == "2 MB"));
        assert!(output.rows.iter().any(|r| r.label == "Existing file"));
        let sections = find(&all, "sections");
        assert_eq!(sections.title, "Sections (2)");
        assert_eq!(sections.rows[0].value, "0:00:01.000 – 0:00:03.000  (0:00:02.000)");
        assert_eq!(sections.rows[2].value, "0:00:05.000");
        assert_eq!(find(&all, "filters").rows[0].value, "top 2, right 0, bottom 2, left 0");
        let render = find(&all, "render");
        assert_eq!(render.rows[0].value, "AV1 balanced");
        assert!(
            render.rows[1].value.starts_with("AV1 (SVT-AV1) · CRF"),
            "{}",
            render.rows[1].value
        );
        let tags = find(&all, "tags");
        assert_eq!(
            (tags.rows[0].value.as_str(), tags.rows[1].value.as_str()),
            ("Beach, #4", "4")
        );
    }

    #[test]
    fn without_a_spec_only_the_job() {
        let all = details(&summary(JobState::Waiting), None, &Extras::default());
        assert_eq!(all.len(), 1);
        let mut empty = spec();
        empty.filters = FilterChain::default();
        empty.metadata = JobMetadata::default();
        let all = details(&summary(JobState::Waiting), Some(&empty), &Extras::default());
        assert_eq!(find(&all, "filters").rows[0].value, "None");
        assert_eq!(find(&all, "tags").rows[0].tone, Tone::Muted);
    }
}
