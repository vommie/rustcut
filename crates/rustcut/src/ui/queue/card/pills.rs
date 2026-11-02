//! The job card's tag pills (Q-P12h): beside the rating, wrapped into at most two rows; what does not fit becomes a
//! "+N" pill at the end of the second row. Slint has no flow layout, so the pills are placed here with the widths Slint
//! measured (like the save toast, `notices/toast_tags.rs`).

use std::collections::HashMap;

/// The pills' room beside the rating: the card's content width (300 px less 2 x 9 px) less the rating's 18 px and gap.
pub const ROOM: f32 = 258.0;
pub const ROWS: usize = 2;
pub const GAP: f32 = 4.0;
pub const PILL_HEIGHT: f32 = 18.0;
pub const PITCH: f32 = PILL_HEIGHT + GAP;

/// A pill's width before Slint measured it: 11 px text and the padding (`job_card.slint`).
pub fn estimate(text: &str) -> f32 {
    az::cast::<usize, f32>(text.chars().count()) * 5.8 + 12.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    /// The "+N" pill.
    pub more: bool,
}

/// Places `tags` in `room` px with the `measured` widths; the height of the pills is `rows(pills) * PITCH - GAP`.
pub fn place(tags: &[String], measured: &HashMap<String, f32>, room: f32) -> Vec<Pill> {
    let width_of = |text: &str| {
        measured
            .get(text)
            .map_or_else(|| estimate(text), |w| w.ceil())
            .min(room)
    };
    // Every pill wrapped into rows of `room`.
    let mut lines: Vec<Vec<(usize, f32)>> = vec![Vec::new()];
    let mut x = 0.0;
    for (index, tag) in tags.iter().enumerate() {
        let width = width_of(tag);
        if x > 0.0 && x + width > room {
            lines.push(Vec::new());
            x = 0.0;
        }
        if let Some(line) = lines.last_mut() {
            line.push((index, width));
        }
        x += width + GAP;
    }
    if lines.len() > ROWS {
        lines.truncate(ROWS);
        // The last row gives up pills until "+N" fits behind them.
        loop {
            let kept: usize = lines.iter().map(Vec::len).sum();
            let more = width_of(&more_text(tags.len() - kept));
            let Some(last) = lines.last_mut() else { break };
            let end: f32 = last.iter().map(|(_, w)| w + GAP).sum();
            if end + more <= room || last.is_empty() {
                break;
            }
            last.pop();
        }
    }
    let mut pills = Vec::with_capacity(tags.len());
    let mut end = (0.0, 0.0);
    for (row, line) in lines.iter().enumerate() {
        let y = az::cast::<usize, f32>(row) * PITCH;
        let mut x = 0.0;
        for (index, width) in line {
            if let Some(text) = tags.get(*index) {
                pills.push(Pill {
                    text: text.clone(),
                    x,
                    y,
                    width: *width,
                    more: false,
                });
            }
            x += width + GAP;
        }
        end = (x, y);
    }
    let left = tags.len() - pills.len();
    if left > 0 {
        let text = more_text(left);
        pills.push(Pill {
            width: width_of(&text),
            text,
            x: end.0,
            y: end.1,
            more: true,
        });
    }
    pills
}

pub fn more_text(n: usize) -> String {
    format!("+{n}")
}

/// The height of the placed pills (0 without pills).
pub fn height(pills: &[Pill]) -> f32 {
    pills.iter().map(|p| p.y + PILL_HEIGHT).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("t{i}")).collect()
    }

    fn widths(tags: &[String], w: f32) -> HashMap<String, f32> {
        let mut m: HashMap<String, f32> = tags.iter().map(|t| (t.clone(), w)).collect();
        for n in 1..20 {
            m.insert(more_text(n), 24.0);
        }
        m
    }

    #[test]
    fn pills_that_fit_wrap_into_two_rows() {
        let t = tags(6);
        // 60 px pills: four per row of 256 (4 * 64 = 256, the last gap may stick out).
        let p = place(&t, &widths(&t, 60.0), ROOM);
        assert_eq!(p.len(), 6);
        assert!(p.iter().all(|p| !p.more));
        let rows: Vec<(f32, f32)> = p.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(rows[3], (192.0, 0.0));
        assert_eq!(rows[4], (0.0, PITCH));
        assert_eq!(height(&p), PITCH + PILL_HEIGHT);
    }

    #[test]
    fn the_rest_becomes_plus_n_in_the_second_row() {
        let t = tags(11);
        let p = place(&t, &widths(&t, 60.0), ROOM);
        let shown = p.iter().filter(|p| !p.more).count();
        // Row two keeps three pills (192 px) so "+N" (24 px) fits behind them.
        assert_eq!(shown, 4 + 3);
        let more = p.last().unwrap();
        assert!(more.more);
        assert_eq!((more.text.as_str(), more.x, more.y), ("+4", 192.0, PITCH));
        assert!(p.iter().all(|p| p.y <= PITCH), "never a third row");
    }

    #[test]
    fn a_wide_tag_is_cut_to_the_room_and_unmeasured_ones_are_estimated() {
        let t = vec!["a very long tag name".to_owned()];
        let wide = place(&t, &widths(&t, 900.0), ROOM);
        assert_eq!(wide[0].width, ROOM);
        let p = place(&["abcd".to_owned()], &HashMap::new(), ROOM);
        assert!((p[0].width - 35.2).abs() < 1e-3);
        assert!(place(&[], &HashMap::new(), ROOM).is_empty());
        assert_eq!(height(&[]), 0.0);
    }
}
