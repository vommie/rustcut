//! Where auto-crop looks and what counts as a border (Settings > Filters, the Crop menu; 0.0.48): how dark a border may
//! be (`cropdetect`'s `limit`, which looks at the brightness only: a colored border needs another way, P9.14), which
//! sides to crop, how many points of the video to look at, and whether only the sections count. A border color picked
//! in the video (P9.14, D-P9l) is found through `colorkey` + `alphaextract` before `cropdetect` (checked with the real
//! ffmpeg: white, gray and blue bars); when nothing is found auto-crop tries again with more tolerance.

use serde::{Deserialize, Serialize};

use crate::edit::Crop;
use crate::time::Timestamp;

/// How many points of the video auto-crop looks at by default.
pub const CROP_SAMPLES: usize = 8;

/// Which edges auto-crop may cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CropSides {
    #[default]
    All,
    /// Letterbox bars only (a dark scene's sides stay).
    TopBottom,
    /// Pillarbox bars only.
    LeftRight,
}

impl CropSides {
    /// `crop` without the edges these sides leave alone.
    pub const fn keep(self, crop: Crop) -> Crop {
        match self {
            Self::All => crop,
            Self::TopBottom => Crop::new(crop.top, 0, crop.bottom, 0),
            Self::LeftRight => Crop::new(0, crop.right, 0, crop.left),
        }
    }
}

/// How auto-crop searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct CropSearch {
    /// The brightest value (of 255) that still counts as border; higher also finds dark gray bars, but may eat into
    /// dark pictures (PyCutRv's 24; checked with the real ffmpeg: 24 finds black bars, #202020 bars were found with 51,
    /// #404040 bars with 77, and 77 already cut into the dark part of ffmpeg's test picture).
    pub limit: u8,
    pub sides: CropSides,
    /// How many points of the video are looked at, 1 to 32.
    pub samples: u8,
    /// Look only inside the sections (the whole video while there are none).
    pub sections_only: bool,
    /// The border color picked in the video (RGB; 0.0.52); `None` = dark borders (`limit`).
    pub color: Option<[u8; 3]>,
    /// How far a pixel's color may be from `color` and still count as border, in percent (`colorkey`'s similarity).
    pub similarity: u8,
    /// More tries when nothing was found, each with the tolerance (`limit` or `similarity`) raised by `step` percent
    /// up to `max_limit` / `max_similarity` (D-P9l: two more tries, by half).
    pub retries: u8,
    pub step: u8,
    pub max_limit: u8,
    pub max_similarity: u8,
}

impl Default for CropSearch {
    fn default() -> Self {
        Self {
            limit: 24,
            sides: CropSides::All,
            samples: 8,
            sections_only: true,
            color: None,
            similarity: 10,
            retries: 2,
            step: 50,
            max_limit: 64,
            max_similarity: 30,
        }
    }
}

impl CropSearch {
    pub const MAX_SAMPLES: u8 = 32;
    pub const MAX_RETRIES: u8 = 5;

    /// The values limited to what auto-crop accepts.
    pub fn sanitized(self) -> Self {
        Self {
            samples: self.samples.clamp(1, Self::MAX_SAMPLES),
            similarity: self.similarity.clamp(1, 100),
            retries: self.retries.min(Self::MAX_RETRIES),
            step: self.step.clamp(10, 200),
            max_limit: self.max_limit.max(1),
            max_similarity: self.max_similarity.clamp(1, 100),
            ..self
        }
    }

    /// The tolerance that grows from try to try: the similarity with a color, else the black limit.
    pub const fn tolerance(self) -> u8 {
        if self.color.is_some() {
            self.similarity
        } else {
            self.limit
        }
    }

    /// The searches of one auto-crop: this one, then up to `retries` more, each tolerance `step` percent above the one
    /// before (at least one more) and the last one at the maximum (checked with the real ffmpeg: #303030 bars need a
    /// black limit near 58, which 24 raised by half twice does not reach); a try that would not raise it is left out.
    pub fn tries(self) -> Vec<Self> {
        let max = if self.color.is_some() {
            self.max_similarity
        } else {
            self.max_limit
        };
        let mut tries = vec![self];
        let mut last = self;
        for retry in 1..=self.retries {
            let now = u32::from(last.tolerance());
            let raised = if retry == self.retries {
                u32::from(max)
            } else {
                (now * (100 + u32::from(self.step))).div_ceil(100).max(now + 1)
            };
            let next = u8::try_from(raised.min(u32::from(max))).unwrap_or(max);
            if u32::from(next) <= now {
                break;
            }
            last = if self.color.is_some() {
                Self {
                    similarity: next,
                    ..last
                }
            } else {
                Self { limit: next, ..last }
            };
            tries.push(last);
        }
        tries
    }

    /// The detection filters of one sample: `cropdetect`, after `colorkey` + `alphaextract` for a picked color (the
    /// border turns black, the picture white).
    pub fn detect_filters(self) -> String {
        match self.color {
            Some([r, g, b]) => format!(
                "format=rgba,colorkey=color=0x{r:02X}{g:02X}{b:02X}:similarity={:.2}:blend=0,alphaextract,\
                 cropdetect=limit=0.1:round=2:reset=0",
                f64::from(self.similarity) / 100.0
            ),
            None => format!("cropdetect=limit={}:round=2:reset=0", self.limit_fraction()),
        }
    }

    /// `cropdetect`'s `limit` as a fraction of the full range, so 10-bit sources use the same threshold.
    pub fn limit_fraction(self) -> String {
        format!("{:.3}", f64::from(self.limit) / 255.0)
    }
}

/// Where auto-crop looks: `count` points spread evenly over `ranges` (the sections; empty = the whole video without its
/// first and last 5 %, titles and fades), at most one point per two seconds, at least one. Without a known duration
/// and without ranges only the start is used.
pub fn crop_sample_times(
    duration: Option<Timestamp>,
    count: usize,
    ranges: &[(Timestamp, Timestamp)],
) -> Vec<Timestamp> {
    let ranges: Vec<(i64, i64)> = ranges
        .iter()
        .map(|(start, end)| (start.as_micros(), end.as_micros()))
        .filter(|(start, end)| end > start)
        .collect();
    // The spans to spread over, their length, and the length that sets the number of points.
    let (spans, total, length) = if ranges.is_empty() {
        let Some(micros) = duration.map(Timestamp::as_micros).filter(|d| *d > 0) else {
            return vec![Timestamp::ZERO];
        };
        let margin = micros / 20;
        (vec![(margin, micros - margin)], micros - 2 * margin, micros)
    } else {
        let total = ranges.iter().map(|(start, end)| end - start).sum();
        (ranges, total, total)
    };
    let count = usize::try_from(length / 2_000_000)
        .unwrap_or(count)
        .clamp(1, count.max(1));
    let count_i = i64::try_from(count).unwrap_or(1);
    (0..count_i)
        .map(|i| {
            let mut offset = total * (2 * i + 1) / (2 * count_i);
            for (start, end) in &spans {
                if offset < end - start {
                    return Timestamp::from_micros(start + offset);
                }
                offset -= end - start;
            }
            Timestamp::from_micros(spans.last().map_or(0, |(_, end)| *end))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_spread_over_the_middle_of_the_video() {
        let times = crop_sample_times(Some(Timestamp::from_secs(100)), CROP_SAMPLES, &[]);
        assert_eq!(times.len(), CROP_SAMPLES);
        assert_eq!(times.first(), Some(&Timestamp::from_micros(10_625_000)));
        assert_eq!(times.last(), Some(&Timestamp::from_micros(89_375_000)));
        assert_eq!(
            crop_sample_times(Some(Timestamp::from_secs(5)), CROP_SAMPLES, &[]).len(),
            2
        );
        assert_eq!(
            crop_sample_times(Some(Timestamp::from_millis(400)), CROP_SAMPLES, &[]).len(),
            1
        );
        assert_eq!(crop_sample_times(None, CROP_SAMPLES, &[]), vec![Timestamp::ZERO]);
        assert_eq!(crop_sample_times(Some(Timestamp::from_secs(100)), 3, &[]).len(), 3);
    }

    #[test]
    fn samples_stay_inside_the_sections() {
        let s = Timestamp::from_secs;
        // 10 s + 30 s of sections: four points at 5, 15, 25 and 35 s of the 40 s they hold.
        let times = crop_sample_times(Some(s(600)), 4, &[(s(100), s(110)), (s(300), s(330))]);
        assert_eq!(times, vec![s(105), s(305), s(315), s(325)]);
        // An empty or reversed range counts as none.
        assert_eq!(
            crop_sample_times(Some(s(100)), 2, &[(s(50), s(50))]),
            crop_sample_times(Some(s(100)), 2, &[])
        );
        // One second of sections: one point, in its middle.
        assert_eq!(
            crop_sample_times(None, CROP_SAMPLES, &[(s(7), s(8))]),
            vec![Timestamp::from_millis(7_500)]
        );
    }

    #[test]
    fn retries_raise_the_tolerance_by_the_step_up_to_the_maximum() {
        let black = CropSearch::default();
        assert_eq!(
            black.tries().iter().map(|s| s.limit).collect::<Vec<_>>(),
            vec![24, 36, 64]
        );
        let near_max = CropSearch { limit: 60, ..black };
        assert_eq!(
            near_max.tries().iter().map(|s| s.limit).collect::<Vec<_>>(),
            vec![60, 64]
        );
        let none = CropSearch { retries: 0, ..black };
        assert_eq!(none.tries().len(), 1);
        let white = CropSearch {
            color: Some([255, 255, 255]),
            ..black
        };
        assert_eq!(
            white.tries().iter().map(|s| s.similarity).collect::<Vec<_>>(),
            vec![10, 15, 30]
        );
        let three = CropSearch { retries: 3, ..black };
        assert_eq!(
            three.tries().iter().map(|s| s.limit).collect::<Vec<_>>(),
            vec![24, 36, 54, 64]
        );
        assert_eq!(
            white.detect_filters(),
            "format=rgba,colorkey=color=0xFFFFFF:similarity=0.10:blend=0,alphaextract,cropdetect=limit=0.1:round=2:reset=0"
        );
        assert_eq!(black.detect_filters(), "cropdetect=limit=0.094:round=2:reset=0");
    }

    #[test]
    fn the_limit_is_a_fraction_and_the_sides_keep_their_edges() {
        assert_eq!(CropSearch::default().limit_fraction(), "0.094");
        let crop = Crop::new(10, 20, 30, 40);
        assert_eq!(CropSides::TopBottom.keep(crop), Crop::new(10, 0, 30, 0));
        assert_eq!(CropSides::LeftRight.keep(crop), Crop::new(0, 20, 0, 40));
        assert_eq!(CropSides::All.keep(crop), crop);
        let wild = CropSearch {
            samples: 0,
            ..CropSearch::default()
        };
        assert_eq!(wild.sanitized().samples, 1);
    }
}
