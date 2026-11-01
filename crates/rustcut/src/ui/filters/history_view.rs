//! What the crop history's list shows (pure, P12 4.3, Q-P12c): first the crops that fit the open video, then the other
//! sizes, pinned ones first, newest first in each group. A crop scaled onto this video that equals another entry shows
//! once. Ten entries per page, and pages only while more than ten exist and the chosen length allows more than ten.

use chrono::{DateTime, Local, NaiveDate};
use rustcut_core::edit::{Crop, CropMemory, Fit, Orientation, Size, fit};

/// Entries per page.
pub const PAGE: usize = 10;
/// The height of an entry and of a group's heading.
pub const ENTRY_HEIGHT: f32 = 38.0;
pub const HEADING_HEIGHT: f32 = 20.0;
/// The box of an entry's small drawing.
const MINI: (f32, f32) = (48.0, 27.0);

/// What the list depends on.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// The frame the crop filter gets; `None` before the probe.
    pub frame: Option<Size>,
    /// The crop in use (`None`: the filter is off).
    pub current: Option<Crop>,
    /// What follows the crop in the chain (entries show as in the output).
    pub orientation: Orientation,
    pub sar: f64,
    /// Entries used since this time (Unix seconds) are this session's.
    pub session_start: i64,
    pub today: NaiveDate,
}

/// The small drawing: the frame's size in its box and the kept area in it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mini {
    pub width: f32,
    pub height: f32,
    pub kept: (f32, f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub memory: CropMemory,
    /// The crop for the open video; `None`: not usable here.
    pub crop: Option<Crop>,
    pub title: String,
    pub detail: String,
    pub tag: &'static str,
    pub tip: String,
    pub session: bool,
    pub current: bool,
    pub mini: Mini,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Heading(&'static str),
    Entry(Box<Line>),
}

/// The lines of `entries` for `context`, in the list's order.
pub fn lines(entries: &[CropMemory], context: &Context) -> Vec<Line> {
    let mut all: Vec<Line> = entries.iter().map(|m| line(m, context)).collect();
    // Group: this size, then scaled, then the rest; a stable sort keeps newest first.
    all.sort_by_key(|l| (rank(l), !l.memory.pinned));
    let mut seen: Vec<Crop> = Vec::new();
    all.retain(|l| {
        l.crop.is_none_or(|crop| {
            let fresh = !seen.contains(&crop);
            seen.push(crop);
            fresh
        })
    });
    all
}

fn rank(line: &Line) -> u8 {
    match (line.crop, line.tag.is_empty()) {
        (Some(_), true) => 0,
        (Some(_), false) => 1,
        (None, _) => 2,
    }
}

fn line(memory: &CropMemory, context: &Context) -> Line {
    let kind = context.frame.map_or(Fit::Other, |frame| fit(memory, frame));
    let crop = match kind {
        Fit::Same => Some(memory.crop),
        Fit::Scaled(crop) => Some(crop),
        Fit::Other => None,
    };
    let (shown_crop, shown_frame) = match (crop, context.frame) {
        (Some(crop), Some(frame)) => (context.orientation.map_crop(crop), context.orientation.size(frame)),
        _ => (memory.crop, memory.frame),
    };
    let kept = shown_crop.kept(shown_frame);
    let original = memory.crop.kept(memory.frame);
    let detail = match kind {
        Fit::Same => format!(
            "{} · {}",
            kept.map(|k| ratio_text(k, context.sar)).unwrap_or_default(),
            edges_text(shown_crop)
        ),
        Fit::Scaled(_) => format!(
            "from {} on {}",
            original.map(dims).unwrap_or_default(),
            dims(memory.frame)
        ),
        Fit::Other => format!("made on {}", dims(memory.frame)),
    };
    Line {
        crop,
        title: kept.map(dims).unwrap_or_default(),
        detail,
        tag: if matches!(kind, Fit::Scaled(_)) { "scaled" } else { "" },
        tip: tip(memory, kind, context.today),
        session: memory.used >= context.session_start,
        current: crop.is_some() && crop == context.current,
        mini: mini(shown_crop, shown_frame),
        memory: memory.clone(),
    }
}

/// `1920×800`
fn dims(size: Size) -> String {
    format!("{}×{}", size.width, size.height)
}

/// The edges that cut something: "140 top, 140 bottom".
pub fn edges_text(crop: Crop) -> String {
    [
        (crop.top, "top"),
        (crop.right, "right"),
        (crop.bottom, "bottom"),
        (crop.left, "left"),
    ]
    .iter()
    .filter(|(value, _)| *value > 0)
    .map(|(value, name)| format!("{value} {name}"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// A known ratio by its name ("4:3"), else "2.40:1".
fn ratio_text(size: Size, sar: f64) -> String {
    const NAMED: [(f64, &str); 7] = [
        (16.0 / 9.0, "16:9"),
        (4.0 / 3.0, "4:3"),
        (3.0 / 2.0, "3:2"),
        (1.0, "1:1"),
        (9.0 / 16.0, "9:16"),
        (3.0 / 4.0, "3:4"),
        (2.0 / 3.0, "2:3"),
    ];
    let ratio = f64::from(size.width) * sar / f64::from(size.height.max(1));
    NAMED
        .iter()
        .find(|(value, _)| (ratio - value).abs() < 0.01)
        .map_or_else(|| format!("{ratio:.2}:1"), |(_, name)| (*name).to_owned())
}

fn tip(memory: &CropMemory, kind: Fit, today: NaiveDate) -> String {
    let when = DateTime::from_timestamp(memory.used, 0)
        .filter(|_| memory.used > 0)
        .map(|t| {
            let local = t.with_timezone(&Local);
            if local.date_naive() == today {
                local.format("today %H:%M").to_string()
            } else {
                local.format("%Y-%m-%d %H:%M").to_string()
            }
        });
    let made = [Some(memory.source.clone()).filter(|s| !s.is_empty()), when]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    match kind {
        Fit::Other if made.is_empty() => "It was made for a picture of another shape.".to_owned(),
        Fit::Other => format!("{made}. It was made for a picture of another shape."),
        Fit::Same | Fit::Scaled(_) => made,
    }
}

fn mini(crop: Crop, frame: Size) -> Mini {
    let (width, height) = (precise(frame.width).max(1.0), precise(frame.height).max(1.0));
    let scale = (MINI.0 / width).min(MINI.1 / height);
    let (w, h) = (width * scale, height * scale);
    let kept_w = (width - precise(crop.left) - precise(crop.right)).max(0.0) * scale;
    let kept_h = (height - precise(crop.top) - precise(crop.bottom)).max(0.0) * scale;
    Mini {
        width: w,
        height: h,
        kept: (precise(crop.left) * scale, precise(crop.top) * scale, kept_w, kept_h),
    }
}

/// Pixels as a drawing's length (frames fit `f32` exactly; only the drawing uses it).
#[expect(clippy::cast_precision_loss, reason = "a drawing 48 px wide")]
const fn precise(value: u32) -> f32 {
    value as f32
}

/// Whether the list has pages at this `length` (the user's rule): more than ten entries, and more than ten allowed.
pub const fn is_paged(lines: usize, length: u8) -> bool {
    lines > PAGE && length as usize > PAGE
}

pub const fn page_count(lines: usize) -> usize {
    lines.div_ceil(PAGE)
}

/// The rows of `page` (all of them without pages) with their groups' headings.
pub fn rows(lines: Vec<Line>, page: Option<usize>) -> Vec<Row> {
    let shown: Vec<Line> = match page {
        Some(page) => lines.into_iter().skip(page.saturating_mul(PAGE)).take(PAGE).collect(),
        None => lines,
    };
    let mut rows = Vec::with_capacity(shown.len() + 2);
    let mut group = None;
    let many = shown.iter().any(|l| rank(l) == 0) && shown.iter().any(|l| rank(l) > 0);
    for line in shown {
        let this = u8::from(rank(&line) > 0);
        if group != Some(this) && (many || page.is_some() || this == 1) {
            rows.push(Row::Heading(if this == 0 { "This video" } else { "Other sizes" }));
        }
        group = Some(this);
        rows.push(Row::Entry(Box::new(line)));
    }
    rows
}

/// The next row with a usable entry from `from` in `step`'s direction (1 or -1), staying when there is none.
pub fn next_usable(rows: &[Row], from: Option<usize>, step: isize) -> Option<usize> {
    let usable = |at: usize| matches!(rows.get(at), Some(Row::Entry(line)) if line.crop.is_some());
    let last = rows.len().checked_sub(1)?;
    let mut at = match from {
        Some(at) => at.checked_add_signed(step).filter(|n| *n <= last),
        None if step > 0 => Some(0),
        None => Some(last),
    };
    while let Some(row) = at {
        if usable(row) {
            return Some(row);
        }
        at = row.checked_add_signed(step).filter(|n| *n <= last);
    }
    from
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: Size = Size::new(1920, 1080);

    fn memory(top: u32, frame: Size, used: i64) -> CropMemory {
        CropMemory {
            frame,
            crop: Crop::new(top, 0, top, 0),
            pinned: false,
            used,
            source: "clip.mkv".to_owned(),
        }
    }

    fn context(frame: Size) -> Context {
        Context {
            frame: Some(frame),
            current: None,
            orientation: Orientation::default(),
            sar: 1.0,
            session_start: 100,
            today: NaiveDate::default(),
        }
    }

    fn titles(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Heading(h) => format!("# {h}"),
                Row::Entry(l) => l.title.clone(),
            })
            .collect()
    }

    #[test]
    fn what_fits_comes_first_and_pinned_entries_lead_their_group() {
        let other = memory(140, Size::new(1280, 720), 300);
        let portrait = memory(100, Size::new(1080, 1920), 400);
        let old = memory(40, HD, 50);
        let pinned = CropMemory {
            pinned: true,
            ..memory(60, HD, 20)
        };
        let all = lines(&[portrait, other, old, pinned], &context(HD));
        assert_eq!(
            titles(&rows(all, None)),
            [
                "# This video",
                "1920×960",
                "1920×1000",
                "# Other sizes",
                "1920×660",
                "1080×1720"
            ]
        );
    }

    #[test]
    fn an_entry_says_what_it_is() {
        let shown = lines(&[memory(140, HD, 200)], &context(HD));
        let line = shown.first().unwrap();
        assert_eq!(line.title, "1920×800");
        assert_eq!(line.detail, "2.40:1 · 140 top, 140 bottom");
        assert_eq!(line.tag, "");
        assert!(line.session);

        let shown = lines(&[memory(140, HD, 50)], &context(Size::new(1280, 720)));
        let line = shown.first().unwrap();
        assert_eq!(line.title, "1280×532");
        assert_eq!(line.detail, "from 1920×800 on 1920×1080");
        assert_eq!(line.tag, "scaled");
        assert!(!line.session);

        let shown = lines(&[memory(140, HD, 50)], &context(Size::new(1080, 1920)));
        let line = shown.first().unwrap();
        assert_eq!(line.crop, None);
        assert!(line.tip.contains("another shape"), "{}", line.tip);
    }

    #[test]
    fn the_output_orientation_turns_what_an_entry_shows() {
        let turned = Orientation::from_degrees_clockwise(90);
        let shown = lines(
            &[memory(140, HD, 200)],
            &Context {
                orientation: turned,
                ..context(HD)
            },
        );
        let line = shown.first().unwrap();
        assert_eq!(line.title, "800×1920");
        assert_eq!(line.detail, "0.42:1 · 140 right, 140 left");
    }

    #[test]
    fn a_scaled_crop_that_equals_an_entry_shows_once_and_the_current_crop_is_marked() {
        let here = Size::new(1280, 720);
        let direct = memory(94, here, 50);
        let scaled = memory(140, HD, 60);
        let shown = lines(
            &[scaled, direct],
            &Context {
                current: Some(Crop::new(94, 0, 94, 0)),
                ..context(here)
            },
        );
        assert_eq!(shown.len(), 1);
        assert!(shown.first().unwrap().current);
        assert_eq!(shown.first().unwrap().tag, "", "the exact entry wins");
    }

    #[test]
    fn pages_need_more_than_ten_entries_and_a_length_above_ten() {
        assert!(!is_paged(10, 50));
        assert!(!is_paged(11, 10), "pinned entries can pass a length of ten");
        assert!(is_paged(11, 11));
        assert_eq!(page_count(11), 2);
        assert_eq!(page_count(20), 2);
        assert_eq!(page_count(21), 3);
        let entries: Vec<_> = (1..=12).map(|n| memory(n * 2, HD, 0)).collect();
        let all = lines(&entries, &context(HD));
        assert_eq!(rows(all.clone(), Some(0)).len(), 11);
        assert_eq!(rows(all, Some(1)).len(), 3);
    }

    #[test]
    fn keys_skip_headings_and_what_cannot_be_used() {
        let shown = lines(
            &[
                memory(40, HD, 0),
                memory(60, HD, 0),
                memory(100, Size::new(1080, 1920), 0),
            ],
            &context(HD),
        );
        let list = rows(shown, None);
        assert_eq!(next_usable(&list, None, 1), Some(1));
        assert_eq!(next_usable(&list, Some(1), 1), Some(2));
        assert_eq!(next_usable(&list, Some(2), 1), Some(2), "the last usable one stays");
        assert_eq!(next_usable(&list, None, -1), Some(2));
        assert_eq!(next_usable(&list, Some(1), -1), Some(1));
    }
}
