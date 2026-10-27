//! What the Job panel says about a job (user decision Q-Q1), without Slint: sections of label/value rows built from the
//! queue's row (state, times, RustView sync) and the job's spec (source, output, sections, filters, render settings,
//! tags and rating). Long values are elided by the panel; the full text is the row's tooltip. Since P9.11 a row can be
//! a badge (the state), carry a status symbol (RustView) and buttons on its right (play, show in the file manager, open
//! the folder, the profile), and the codecs have one row per attribute.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rustcut_core::edit::{DeinterlaceRate, Filter, FilterChain};
use rustcut_core::job::{JobSpec, JobState, JobStateKind};
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

/// A status symbol before a row's value (P9.11: the RustView state).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RowIcon {
    #[default]
    None,
    Done,
    Failed,
    Waiting,
    Off,
    Removed,
}

/// A button on a row's right (P9.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Play the file in the default player.
    Play(PathBuf),
    /// Show the file in the file manager.
    Reveal(PathBuf),
    /// Open the folder in the file manager.
    OpenFolder(PathBuf),
    /// Show the render profile this job was saved with (it exists unchanged).
    Profile(String),
}

impl RowAction {
    /// The tooltip of its button.
    pub fn tooltip(&self) -> &'static str {
        match self {
            Self::Play(_) => "Play in the default player",
            Self::Reveal(_) => "Show in the file manager",
            Self::OpenFolder(_) => "Open the folder in the file manager",
            Self::Profile(_) => "Show the profile in Settings > Render Profiles",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRow {
    pub label: String,
    pub value: String,
    pub tooltip: String,
    pub tone: Tone,
    /// A file name or folder: cut in the middle when it does not fit (P9.1).
    pub path: bool,
    /// The value as a badge in the job state's color (the state row).
    pub badge: bool,
    pub icon: RowIcon,
    /// An attribute below the row before it (the codecs' video and audio attributes).
    pub indent: bool,
    pub actions: Vec<RowAction>,
}

impl DetailRow {
    fn new(label: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: label.to_owned(),
            tooltip: value.clone(),
            value,
            tone: Tone::Normal,
            path: false,
            badge: false,
            icon: RowIcon::None,
            indent: false,
            actions: Vec::new(),
        }
    }

    fn indented(label: &str, value: impl Into<String>) -> Self {
        Self {
            indent: true,
            ..Self::new(label, value)
        }
    }

    fn act(mut self, action: RowAction) -> Self {
        self.actions.push(action);
        self
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

    const fn path(mut self) -> Self {
        self.path = true;
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
    /// Whether the profile library holds this profile unchanged (its row offers the jump to it).
    pub profile_known: Option<&'a dyn Fn(&RenderProfile) -> bool>,
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
    sections.push(output_section(
        spec,
        extras.size,
        row.state.kind() == JobStateKind::Finished,
    ));
    sections.push(sections_section(spec));
    sections.push(filters_section(&spec.filters));
    let known = extras.profile_known.is_some_and(|f| f(&spec.profile));
    sections.push(render_section(&spec.profile, known));
    sections.push(tags_section(spec, extras.tag_name));
    sections
}

fn job_section(row: &JobSummary, without_rustview: bool) -> DetailSection {
    let mut rows = vec![DetailRow {
        badge: true,
        ..DetailRow::new("State", title(row.state.kind()))
    }];
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
        rows.push(DetailRow::new("Ended", local_time(at)));
        if let Some(started) = row.started_at
            && at >= started
        {
            rows.push(DetailRow::new(
                "Render time",
                clock(Timestamp::from_millis(at - started)),
            ));
        }
    }
    let (sync, tone, icon) = match &row.sync {
        // The Tag & Rate panel's "Active" switch was off (user decision Q-P8b).
        _ if without_rustview => ("Saved without RustView".to_owned(), Tone::Muted, RowIcon::Off),
        SyncState::NotSynced => ("Not written".to_owned(), Tone::Muted, RowIcon::Off),
        SyncState::Pending => ("Waiting to be written".to_owned(), Tone::Muted, RowIcon::Waiting),
        SyncState::Synced => ("Written".to_owned(), Tone::Normal, RowIcon::Done),
        SyncState::Failed { error } => (format!("Failed: {error}"), Tone::Error, RowIcon::Failed),
        SyncState::CleanedUp => ("Removed again".to_owned(), Tone::Muted, RowIcon::Removed),
    };
    rows.push(DetailRow {
        tone,
        icon,
        ..DetailRow::new("RustView", sync)
    });
    DetailSection {
        id: "job",
        title: "Job".into(),
        rows,
    }
}

fn source_section(spec: &JobSpec) -> DetailSection {
    let mut rows = path_rows(&spec.source, true);
    if let Some(info) = &spec.source_info {
        rows.push(DetailRow::new(
            "Size",
            format!("{}x{}", info.size.width, info.size.height),
        ));
        if let Some(rate) = info.frame_rate {
            rows.push(DetailRow::new(
                "Frame rate",
                format!("{:.3} fps", rate.as_f64()).replace(".000 fps", " fps"),
            ));
        }
        rows.push(DetailRow::new("Duration", clock(info.duration)));
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

fn output_section(spec: &JobSpec, size: Option<u64>, finished: bool) -> DetailSection {
    let mut rows = path_rows(&spec.target_path(), finished);
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

/// The file and folder rows; `exists` offers playing the file and showing it (an output only once it is rendered).
fn path_rows(path: &Path, exists: bool) -> Vec<DetailRow> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut file = DetailRow::new("File", name).tip(path.display().to_string()).path();
    if exists {
        file = file
            .act(RowAction::Play(path.to_path_buf()))
            .act(RowAction::Reveal(path.to_path_buf()));
    }
    vec![
        file,
        DetailRow::muted("Folder", folder.display().to_string())
            .path()
            .act(RowAction::OpenFolder(folder)),
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
                format!(
                    "{} – {}  ({})",
                    s.start().format_compact(),
                    s.end().format_compact(),
                    s.duration().format_compact()
                ),
            )
        })
        .collect();
    if rows.len() > 1 {
        rows.push(DetailRow::muted("Total", spec.output_duration().format_compact()));
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
        Filter::Deinterlace {
            deinterlacer,
            rate,
            field_order,
            only_interlaced,
        } => {
            let rate = match rate {
                DeinterlaceRate::Frame => "one frame per frame",
                DeinterlaceRate::Field => "one frame per field",
            };
            let mut text = format!("{}, {rate}", deinterlacer.ffmpeg_name());
            if deinterlacer.has_field_options() {
                match field_order {
                    rustcut_core::edit::FieldParity::Auto => {}
                    rustcut_core::edit::FieldParity::TopFirst => text.push_str(", top field first"),
                    rustcut_core::edit::FieldParity::BottomFirst => text.push_str(", bottom field first"),
                }
                if *only_interlaced {
                    text.push_str(", only interlaced frames");
                }
            }
            text
        }
        Filter::Crop(c) => format!("top {}, right {}, bottom {}, left {}", c.top, c.right, c.bottom, c.left),
        Filter::Deshake(d) => {
            let mut text = format!(
                "shakiness {}, accuracy {}, smoothing {}{}",
                d.shakiness,
                d.accuracy,
                d.smoothing,
                if d.tripod { ", tripod" } else { "" }
            );
            if d.sharpen {
                text.push_str(", sharpened");
            }
            if let Some(shift) = d.max_shift {
                let _ = write!(text, ", at most {shift} px");
            }
            if let Some(angle) = d.max_angle {
                let _ = write!(text, ", at most {angle}°");
            }
            text
        }
        Filter::Orientation(o) => o.label().to_owned(),
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

fn render_section(profile: &RenderProfile, known: bool) -> DetailSection {
    let v = &profile.video;
    let spec = v.encoder.spec();
    let mut name = DetailRow::new("Profile", profile.name.as_str());
    if known {
        name = name.act(RowAction::Profile(profile.id.clone()));
    }
    let (rate_label, rate) = match v.rate_control {
        RateControl::Quality { value } => ("Quality", format!("{} {value}", spec.quality.label)),
        RateControl::ConstantQp { qp } => ("Quality", format!("QP {qp}")),
        RateControl::AverageBitrate { kbps } => ("Bit rate", format!("{kbps} kbit/s")),
        RateControl::TwoPass { kbps } => ("Bit rate", format!("{kbps} kbit/s, two passes")),
        RateControl::TargetSize { mebibytes } => ("Target size", format!("{mebibytes} MiB, two passes")),
    };
    let mut rows = vec![
        name,
        DetailRow::new("Video", spec.display_name),
        DetailRow::indented(rate_label, rate),
    ];
    if let Some(preset) = v.preset.as_deref().or(spec.default_preset) {
        let label = spec
            .presets
            .iter()
            .find(|p| p.value == preset)
            .map_or(preset, |p| p.label);
        rows.push(DetailRow::indented("Preset", label));
    }
    rows.push(DetailRow::indented(
        "Bit depth",
        match v.bit_depth {
            rustcut_core::render::BitDepth::Eight => "8-bit",
            rustcut_core::render::BitDepth::Ten => "10-bit",
        },
    ));
    let a = &profile.audio;
    rows.push(DetailRow::new("Audio", a.codec.spec().display_name));
    if a.codec.takes_bitrate()
        && let Some(kbps) = a.bitrate_kbps.or(a.codec.spec().default_bitrate)
    {
        rows.push(DetailRow::indented("Bit rate", format!("{kbps} kbit/s")));
    }
    if a.codec != rustcut_core::render::AudioCodec::Copy {
        rows.push(DetailRow::indented(
            "Channels",
            match a.channels {
                None => "As the source".to_owned(),
                Some(1) => "Mono".to_owned(),
                Some(2) => "Stereo".to_owned(),
                Some(6) => "5.1".to_owned(),
                Some(n) => format!("{n} channels"),
            },
        ));
        let rate = a.sample_rate.or(a.codec.spec().required_sample_rate);
        rows.push(DetailRow::indented(
            "Sample rate",
            rate.map_or_else(
                || "As the source".to_owned(),
                |hz| format!("{} kHz", f64::from(hz) / 1000.0),
            ),
        ));
    }
    rows.push(DetailRow::new("Container", profile.container.display_name()));
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
        let known = |p: &RenderProfile| p.id == "av1-balanced";
        let spec = spec();
        let all = details(
            &row,
            Some(&spec),
            &Extras {
                size: Some(2_000_000),
                tag_name: Some(&name),
                profile_known: Some(&known),
            },
        );
        assert_eq!(all.iter().map(|s| s.id).collect::<Vec<_>>(), SECTION_IDS);
        let job = find(&all, "job");
        assert!(job.rows[0].badge && job.rows[0].value == "Failed");
        assert_eq!(job.rows[1].tone, Tone::Error);
        assert!(job.rows.iter().any(|r| r.value == "#7 (attempt 2)"));
        assert!(
            job.rows
                .iter()
                .any(|r| r.label == "Render time" && r.value == "0:01:05")
        );
        assert!(
            job.rows
                .iter()
                .any(|r| r.value == "Failed: no" && r.tone == Tone::Error && r.icon == RowIcon::Failed)
        );
        let source = find(&all, "source");
        assert_eq!(
            source.rows[0].actions,
            [
                RowAction::Play(PathBuf::from("/in/Clip.mp4")),
                RowAction::Reveal(PathBuf::from("/in/Clip.mp4"))
            ]
        );
        assert_eq!(source.rows[1].actions, [RowAction::OpenFolder(PathBuf::from("/in"))]);
        let values: Vec<(&str, &str)> = source.rows[2..]
            .iter()
            .map(|r| (r.label.as_str(), r.value.as_str()))
            .collect();
        assert_eq!(
            values,
            [
                ("Size", "1920x1080"),
                ("Frame rate", "25 fps"),
                ("Duration", "0:01:00"),
                ("Audio", "2 tracks")
            ]
        );
        let output = find(&all, "output");
        assert_eq!(output.rows[0].tooltip, "/out/Clip - 01.mkv");
        assert!(
            output.rows[0].actions.is_empty(),
            "a failed job's output does not exist"
        );
        assert_eq!(output.rows[1].actions, [RowAction::OpenFolder(PathBuf::from("/out"))]);
        assert!(output.rows.iter().any(|r| r.value == "2 MB"));
        assert!(output.rows.iter().any(|r| r.label == "Existing file"));
        let sections = find(&all, "sections");
        assert_eq!(sections.title, "Sections (2)");
        assert_eq!(sections.rows[0].value, "01.000s – 03.000s  (02.000s)");
        assert_eq!(sections.rows[2].value, "05.000s");
        assert_eq!(find(&all, "filters").rows[0].value, "top 2, right 0, bottom 2, left 0");
        let render = find(&all, "render");
        assert_eq!(render.rows[0].value, "AV1 balanced");
        assert_eq!(render.rows[0].actions, [RowAction::Profile("av1-balanced".into())]);
        let rows: Vec<(&str, bool)> = render.rows.iter().map(|r| (r.label.as_str(), r.indent)).collect();
        assert_eq!(
            rows,
            [
                ("Profile", false),
                ("Video", false),
                ("Quality", true),
                ("Preset", true),
                ("Bit depth", true),
                ("Audio", false),
                ("Bit rate", true),
                ("Channels", true),
                ("Sample rate", true),
                ("Container", false),
            ],
            "one row per attribute"
        );
        assert_eq!(render.rows[1].value, "AV1 (SVT-AV1)");
        assert!(render.rows[2].value.starts_with("CRF "), "{}", render.rows[2].value);
        assert_eq!(render.rows[8].value, "48 kHz", "Opus needs 48 kHz");
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
        assert!(
            find(&all, "render").rows[0].actions.is_empty(),
            "no jump without knowing the profile library"
        );
    }

    #[test]
    fn a_finished_output_can_be_played_and_shown() {
        let mut row = summary(JobState::Finished);
        row.sync = SyncState::Synced;
        let all = details(&row, Some(&spec()), &Extras::default());
        let target = PathBuf::from("/out/Clip - 01.mkv");
        assert_eq!(
            find(&all, "output").rows[0].actions,
            [RowAction::Play(target.clone()), RowAction::Reveal(target)]
        );
        let rustview = find(&all, "job").rows.iter().find(|r| r.label == "RustView").unwrap();
        assert_eq!((rustview.value.as_str(), rustview.icon), ("Written", RowIcon::Done));
    }
}
