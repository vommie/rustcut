//! Which thumbnails a video gets and in which order (pure). The spacing grows with the square root of the length, so short
//! videos are fine, long ones stay well covered and no length makes it jump (user decision Q-P12e; 1 h at factor 15: 900
//! thumbnails, one every 4 s). The order is 5 evenly spread, then halving the gaps, left to right within a level: every
//! moment covers the whole video, and each level stays kind to a drive.

use rustcut_thumbs_protocol::Job;

use super::set::ThumbSet;

/// No thumbnails nearer than this.
pub const MIN_SPACING_MS: u64 = 250;
/// A zoomed view is covered like a strip of this many pictures.
const VIEW_SLOTS: u64 = 120;

/// Milliseconds between thumbnails: `sqrt(seconds) / factor` seconds.
pub fn spacing_ms(duration_ms: u64, factor: u32) -> u64 {
    let spacing = duration_ms.saturating_mul(1000).isqrt() / u64::from(factor.max(1));
    spacing.max(MIN_SPACING_MS)
}

/// The indexes `0..count` in the coarse-to-fine order: 5, 10, 20, ... parts, each level sorted.
pub fn level_order(count: usize) -> Vec<usize> {
    let mut seen = vec![false; count];
    let mut order = Vec::with_capacity(count);
    let mut parts = 5_usize;
    while order.len() < count && parts < count.saturating_mul(8) {
        for part in 0..parts {
            let index = (part * 2 + 1).saturating_mul(count) / (parts * 2);
            if let Some(slot) = seen.get_mut(index)
                && !*slot
            {
                *slot = true;
                order.push(index);
            }
        }
        parts *= 2;
    }
    let rest = seen.iter().enumerate().filter(|(_, done)| !**done).map(|(index, _)| index);
    order.extend(rest.collect::<Vec<_>>());
    order
}

/// The grid of one video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub duration_ms: u64,
    pub spacing_ms: u64,
}

impl Grid {
    pub fn new(duration_ms: u64, factor: u32) -> Self {
        Self {
            duration_ms,
            spacing_ms: spacing_ms(duration_ms, factor),
        }
    }

    pub fn count(self) -> usize {
        usize::try_from((self.duration_ms / self.spacing_ms).max(1)).unwrap_or(usize::MAX)
    }

    fn slot(self, index: usize) -> u64 {
        let index = u64::try_from(index).unwrap_or(u64::MAX);
        index.saturating_mul(self.spacing_ms).saturating_add(self.spacing_ms / 2)
    }

    /// First stage: a keyframe for every place not covered yet, coarse to fine.
    pub fn keyframes(self, have: &ThumbSet) -> Vec<Job> {
        level_order(self.count())
            .into_iter()
            .map(|index| self.slot(index))
            .filter(|&ms| have.gap(ms).is_none_or(|gap| gap > self.spacing_ms))
            .map(|ms| Job::Key { ms })
            .collect()
    }

    /// Second stage: decode where the nearest picture is farther away than the spacing, one span per keyframe, the
    /// spans coarse to fine. Files with dense keyframes need none; a file with one keyframe gets one long span.
    pub fn refinement(self, have: &ThumbSet) -> Vec<Job> {
        let targets = (0..self.count()).map(|index| self.slot(index));
        spans(targets, self.spacing_ms, have)
    }

    /// What a zoomed view still lacks, at the view's own finer spacing.
    pub fn view(self, from_ms: u64, to_ms: u64, have: &ThumbSet) -> Vec<Job> {
        let length = to_ms.saturating_sub(from_ms);
        if length == 0 || length >= self.duration_ms {
            return Vec::new();
        }
        let spacing = (length / VIEW_SLOTS).max(MIN_SPACING_MS);
        let targets = (0..=length / spacing).map(|step| from_ms.saturating_add(step.saturating_mul(spacing)));
        spans(targets.filter(|&ms| ms < self.duration_ms), spacing, have)
    }
}

/// Groups the targets without a near picture by the picture before them.
fn spans(targets: impl Iterator<Item = u64>, spacing_ms: u64, have: &ThumbSet) -> Vec<Job> {
    let mut groups: Vec<(u64, Vec<u64>)> = Vec::new();
    for ms in targets {
        if have.gap(ms).is_some_and(|gap| gap <= spacing_ms) {
            continue;
        }
        let start = have.floor(ms).unwrap_or(0);
        match groups.last_mut() {
            Some((known, list)) if *known == start => list.push(ms),
            _ => groups.push((start, vec![ms])),
        }
    }
    let order = level_order(groups.len());
    let mut slots: Vec<Option<(u64, Vec<u64>)>> = groups.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|index| slots.get_mut(index).and_then(Option::take))
        .map(|(start_ms, targets)| Job::Span { start_ms, targets })
        .collect()
}

/// Niceness of the helper. Thumbnails stay at the setting, but a render must not push them back: the user works with them
/// and "renderings dürfen ruhig langsamer werden", so while one runs they sit just ahead of it. mpv keeps its lead.
pub fn niceness(setting: u8, render_nice: u8, rendering: bool) -> i32 {
    let nice = if rendering {
        setting.min(render_nice.saturating_sub(1))
    } else {
        setting
    };
    i32::from(nice.min(19))
}

/// The number of workers for "automatic": half the cores, at most 4.
pub fn automatic_workers(cores: usize) -> u32 {
    u32::try_from((cores / 2).clamp(1, 4)).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::set::Thumb;
    use super::*;

    fn set(times: &[u64]) -> ThumbSet {
        let mut set = ThumbSet::default();
        for &time_ms in times {
            set.insert(Thumb {
                time_ms,
                exact: false,
                width: 320,
                height: 180,
                webp: Arc::from(&b"x"[..]),
            });
        }
        set
    }

    #[test]
    fn the_spacing_follows_the_square_root() {
        assert_eq!(spacing_ms(3_600_000, 15), 4000);
        assert_eq!(spacing_ms(600_000, 15), 1632);
        assert_eq!(spacing_ms(10_800_000, 15), 6928);
        assert_eq!(spacing_ms(3_600_000, 25), 2400);
    }

    #[test]
    fn short_videos_stop_at_the_minimum() {
        assert_eq!(spacing_ms(10_000, 15), MIN_SPACING_MS);
        assert_eq!(Grid::new(10_000, 15).count(), 40);
        assert_eq!(Grid::new(100, 15).count(), 1);
    }

    #[test]
    fn the_order_starts_with_five_even_places() {
        let order = level_order(100);
        assert_eq!(order.get(..5).unwrap(), [10, 30, 50, 70, 90]);
        assert_eq!(order.get(5..10).unwrap(), [5, 15, 25, 35, 45]);
    }

    #[test]
    fn the_order_holds_every_index_once() {
        for count in [0, 1, 2, 3, 7, 64, 900, 1559] {
            let mut order = level_order(count);
            order.sort_unstable();
            assert_eq!(order, (0..count).collect::<Vec<_>>());
        }
    }

    #[test]
    fn covered_places_get_no_keyframe_job() {
        let grid = Grid::new(100_000, 15);
        let all = grid.keyframes(&ThumbSet::default()).len();
        let have = set(&[1000, 2000, 3000]);
        assert!(grid.keyframes(&have).len() < all);
    }

    #[test]
    fn dense_keyframes_need_no_refinement() {
        let grid = Grid::new(60_000, 15);
        let times: Vec<u64> = (0..120).map(|i| i * 500).collect();
        assert!(grid.refinement(&set(&times)).is_empty());
    }

    #[test]
    fn one_keyframe_makes_one_span_over_everything() {
        let grid = Grid::new(10_000, 15);
        let jobs = grid.refinement(&set(&[0]));
        let [Job::Span { start_ms, targets }] = jobs.as_slice() else {
            panic!("{jobs:?}");
        };
        assert_eq!(*start_ms, 0);
        assert!(targets.len() > 30);
        assert!(targets.is_sorted());
    }

    #[test]
    fn far_keyframes_make_one_span_each() {
        let grid = Grid::new(60_000, 15);
        let jobs = grid.refinement(&set(&[0, 20_000, 40_000]));
        assert_eq!(jobs.len(), 3);
        for job in &jobs {
            assert!(matches!(job, Job::Span { start_ms, .. } if start_ms % 20_000 == 0));
        }
    }

    #[test]
    fn a_zoomed_view_asks_finer_than_the_grid() {
        let grid = Grid::new(3_600_000, 15);
        let jobs = grid.view(600_000, 660_000, &ThumbSet::default());
        let count: usize = jobs
            .iter()
            .map(|job| match job {
                Job::Span { targets, .. } => targets.len(),
                _ => 0,
            })
            .sum();
        assert!(count > 100, "{count}");
        assert!(grid.view(0, 3_600_000, &ThumbSet::default()).is_empty());
    }

    #[test]
    fn a_render_never_gets_ahead_of_thumbnails() {
        assert_eq!(niceness(19, 10, false), 19);
        assert_eq!(niceness(19, 10, true), 9);
        assert_eq!(niceness(5, 10, true), 5);
        assert_eq!(niceness(19, 0, true), 0);
    }

    #[test]
    fn automatic_workers_stay_between_one_and_four() {
        assert_eq!(automatic_workers(1), 1);
        assert_eq!(automatic_workers(6), 3);
        assert_eq!(automatic_workers(32), 4);
    }
}
