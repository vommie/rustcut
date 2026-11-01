//! The rating and the tags of a saved job in its toast (questionnaire `2026-11-01-rustcut-save-toast`): one line of tag
//! pills beside the rating, "+N more" for the rest, all of them while the pointer rests on the card; a quiet hint names
//! what the job does not have. Slint has no flow layout, so the pills are placed here with the widths Slint measured
//! (like the tag history, `tags/history_view.rs`).

use std::collections::HashMap;

/// What the toast says about the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLabels {
    pub rating: Option<u8>,
    /// Tag names.
    pub tags: Vec<String>,
}

impl JobLabels {
    /// The quiet hint: what the job does not have.
    pub fn hint(&self) -> &'static str {
        match (self.rating.is_some(), self.tags.is_empty()) {
            (false, true) => "No rating, no tags",
            (false, false) => "No rating",
            (true, true) => "No tags",
            (true, false) => "",
        }
    }
}

/// The width of the line under the card's title (the card's 320 px less its paddings and the symbol's column).
const LINE_WIDTH: f32 = 274.0;
/// What stands before the pills: the rating (22 px and the gap) or the hint "No rating".
const RATING_WIDTH: f32 = 26.0;
const NO_RATING_WIDTH: f32 = 58.0;
/// Room for "+12 more".
const MORE_WIDTH: f32 = 50.0;
const GAP: f32 = 4.0;
const PITCH: f32 = 26.0;
pub const PILL_HEIGHT: f32 = 22.0;

/// A pill's width before Slint measured it: 11 px text and the padding.
fn estimate(text: &str) -> f32 {
    az::cast::<usize, f32>(text.chars().count()) * 5.8 + 10.0
}

/// A pill at its places: in the one line (when it fits there) and with all pills open.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    pub text: String,
    pub width: f32,
    pub x: f32,
    pub in_line: bool,
    pub open_x: f32,
    pub open_y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub pills: Vec<Pill>,
    /// How many pills the line leaves out, and where "+N more" stands.
    pub more: usize,
    pub more_x: f32,
    /// The height of all pills open.
    pub open_height: f32,
}

/// Places the job's pills with the `measured` widths.
pub fn place(job: &JobLabels, measured: &HashMap<String, f32>) -> Placement {
    let available = LINE_WIDTH
        - if job.rating.is_some() {
            RATING_WIDTH
        } else {
            NO_RATING_WIDTH
        };
    let widths: Vec<f32> = job
        .tags
        .iter()
        .map(|t| measured.get(t).map_or_else(|| estimate(t), |w| w.ceil()).min(available))
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + GAP * az::cast::<usize, f32>(widths.len().saturating_sub(1));
    // The pills in the line: all of them, or as many as leave room for "+N more".
    let room = if total <= available {
        available
    } else {
        available - MORE_WIDTH - GAP
    };
    let mut line = 0.0;
    let mut in_line = 0;
    for width in &widths {
        if line + width > room {
            break;
        }
        line += width + GAP;
        in_line += 1;
    }
    let (mut x, mut open_x, mut open_y) = (0.0, 0.0, 0.0);
    let mut pills = Vec::with_capacity(widths.len());
    for (index, (text, width)) in job.tags.iter().zip(&widths).enumerate() {
        if open_x > 0.0 && open_x + width > available {
            open_x = 0.0;
            open_y += PITCH;
        }
        pills.push(Pill {
            text: text.clone(),
            width: *width,
            x,
            in_line: index < in_line,
            open_x,
            open_y,
        });
        x += width + GAP;
        open_x += width + GAP;
    }
    Placement {
        pills,
        more: job.tags.len() - in_line,
        more_x: line,
        open_height: open_y + PILL_HEIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(rating: Option<u8>, tags: &[&str]) -> JobLabels {
        JobLabels {
            rating,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    fn measured(tags: &[&str], width: f32) -> HashMap<String, f32> {
        tags.iter().map(|t| ((*t).to_owned(), width)).collect()
    }

    #[test]
    fn the_hint_names_what_is_missing() {
        assert_eq!(job(None, &[]).hint(), "No rating, no tags");
        assert_eq!(job(None, &["a"]).hint(), "No rating");
        assert_eq!(job(Some(0), &[]).hint(), "No tags", "0 is a rating");
        assert_eq!(job(Some(4), &["a"]).hint(), "");
    }

    #[test]
    fn tags_that_fit_stay_in_the_line() {
        let tags = ["a", "b"];
        let place = place(&job(Some(4), &tags), &measured(&tags, 40.0));
        assert_eq!(place.more, 0);
        assert!(place.pills.iter().all(|p| p.in_line));
        assert_eq!(place.pills[1].x, 44.0);
        assert_eq!((place.pills[1].open_x, place.pills[1].open_y), (44.0, 0.0));
        assert_eq!(place.open_height, PILL_HEIGHT);
    }

    #[test]
    fn the_rest_goes_to_more_and_shows_when_open() {
        let tags = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let place = place(&job(Some(4), &tags), &measured(&tags, 60.0));
        // 248 px beside the rating, 194 with "+N more": three pills of 60 and their gaps fit.
        assert_eq!(place.pills.iter().filter(|p| p.in_line).count(), 2 + 1);
        assert_eq!(place.more, 5);
        assert_eq!(place.more_x, 192.0);
        let rows: Vec<f32> = place.pills.iter().map(|p| p.open_y).collect();
        assert_eq!(
            rows,
            [0.0, 0.0, 0.0, 26.0, 26.0, 26.0, 52.0, 52.0],
            "three of them per row"
        );
        assert_eq!(place.open_height, 74.0);
    }

    #[test]
    fn a_pill_wider_than_the_line_is_cut_and_a_missing_measure_is_estimated() {
        let wide = measured(&["wide"], 900.0);
        let place_wide = place(&job(Some(1), &["wide"]), &wide);
        assert_eq!((place_wide.pills[0].width, place_wide.more), (248.0, 0));
        let unmeasured = place(&job(None, &["abcd"]), &HashMap::new());
        assert!((unmeasured.pills[0].width - 33.2).abs() < 1e-3);
    }
}
