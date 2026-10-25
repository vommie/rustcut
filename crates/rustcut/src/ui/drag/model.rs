//! Drag & drop of list rows (P9.4, the user's review note), pure so every rule is tested: a list hands in its rows'
//! heights, the space between them and which rows are dragged (the selection, or the grabbed row); while the pointer
//! moves, [`ListDrag::frame`] says where the dragged block would land, how far every other row moves aside to open
//! the gap there, and where the ghost under the pointer stands. [`auto_scroll`] is the speed at which the list scrolls
//! while the pointer is near (or past) its top or bottom edge, faster toward the edge. The queue uses it (P9.4), the
//! sections (P9.5) and the filters (P9.13) follow.

/// How close to an edge (logical pixels) the list starts to scroll, at most a quarter of its height.
pub const EDGE_ZONE: f32 = 48.0;
/// The fastest scroll step, reached at the edge and beyond it (logical pixels per tick of 16 ms).
pub const MAX_STEP: f32 = 22.0;

/// One drag of some rows of a list (content coordinates: 0 = the top of the first row).
#[derive(Debug, Clone, PartialEq)]
pub struct ListDrag {
    heights: Vec<f32>,
    spacing: f32,
    dragged: Vec<bool>,
    /// Where in the grabbed row the pointer took it.
    grab: f32,
    /// The top of the grabbed row in the original layout.
    grabbed_top: f32,
    /// The rows above the grabbed one that are dragged too (the ghost's grab lies this far into the block).
    block_before: f32,
}

/// Where everything stands for one pointer position.
#[derive(Debug, Clone, PartialEq)]
pub struct DragFrame {
    /// The index in the original order the dragged block goes before (the row count = the end).
    pub drop: usize,
    /// Whether dropping here changes the order.
    pub moves: bool,
    /// How far each row is moved from its place (dragged rows: their place inside the gap).
    pub shifts: Vec<f32>,
    /// The gap the dragged block would fill.
    pub gap_top: f32,
    pub gap_height: f32,
    /// The top of the ghost (the dragged block under the pointer), kept inside the list.
    pub ghost_top: f32,
}

impl ListDrag {
    /// A drag of the rows marked in `dragged`, grabbed at `grab_y` (content coordinates) inside row `grabbed`; `None`
    /// when nothing is dragged, the lengths differ or the grabbed row is not among the dragged ones.
    pub fn new(heights: Vec<f32>, spacing: f32, dragged: Vec<bool>, grabbed: usize, grab_y: f32) -> Option<Self> {
        if heights.len() != dragged.len() || !dragged.get(grabbed).copied().unwrap_or(false) {
            return None;
        }
        let heights: Vec<f32> = heights.into_iter().map(|h| finite(h).max(0.0)).collect();
        let spacing = finite(spacing).max(0.0);
        let tops = tops(&heights, spacing);
        let grabbed_top = tops.get(grabbed).copied().unwrap_or(0.0);
        let block_before = heights
            .iter()
            .zip(&dragged)
            .take(grabbed)
            .filter(|(_, d)| **d)
            .map(|(h, _)| h + spacing)
            .sum();
        Some(Self {
            heights,
            spacing,
            dragged,
            grab: finite(grab_y) - grabbed_top,
            grabbed_top,
            block_before,
        })
    }

    /// The number of dragged rows.
    pub fn count(&self) -> usize {
        self.dragged.iter().filter(|d| **d).count()
    }

    /// The height of the dragged block (its rows and the space after each).
    pub fn block_height(&self) -> f32 {
        self.heights
            .iter()
            .zip(&self.dragged)
            .filter(|(_, d)| **d)
            .map(|(h, _)| h + self.spacing)
            .sum()
    }

    /// Everything for the pointer at `pointer_y` (content coordinates).
    pub fn frame(&self, pointer_y: f32) -> DragFrame {
        let block = self.block_height();
        let total: f32 = self.heights.iter().map(|h| h + self.spacing).sum();
        // The block's top under the pointer, inside the list.
        let ghost_top = (finite(pointer_y) - self.grab - self.block_before).clamp(0.0, (total - block).max(0.0));
        let stays: Vec<usize> = (0..self.heights.len()).filter(|i| !self.is_dragged(*i)).collect();
        // The rows that stay, closed up: the gap goes to the slot whose top is nearest the block's top.
        let mut closed = Vec::with_capacity(stays.len() + 1);
        let mut y = 0.0;
        for i in &stays {
            closed.push(y);
            y += self.heights.get(*i).copied().unwrap_or(0.0) + self.spacing;
        }
        closed.push(y);
        let slot = closed
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (*a - ghost_top).abs().total_cmp(&(*b - ghost_top).abs()))
            .map_or(0, |(slot, _)| slot);
        let gap_top = closed.get(slot).copied().unwrap_or(0.0);
        let tops = tops(&self.heights, self.spacing);
        let mut shifts = vec![0.0; self.heights.len()];
        for (j, i) in stays.iter().enumerate() {
            let new_top = closed.get(j).copied().unwrap_or(0.0) + if j >= slot { block } else { 0.0 };
            if let (Some(shift), Some(top)) = (shifts.get_mut(*i), tops.get(*i)) {
                *shift = new_top - top;
            }
        }
        let mut inside = gap_top;
        for (i, top) in tops.iter().enumerate() {
            if self.is_dragged(i) {
                if let Some(shift) = shifts.get_mut(i) {
                    *shift = inside - top;
                }
                inside += self.heights.get(i).copied().unwrap_or(0.0) + self.spacing;
            }
        }
        let drop = stays.get(slot).copied().unwrap_or(self.heights.len());
        DragFrame {
            drop,
            moves: shifts.iter().any(|s| s.abs() > 0.5),
            shifts,
            gap_top,
            gap_height: block,
            ghost_top,
        }
    }

    fn is_dragged(&self, index: usize) -> bool {
        self.dragged.get(index).copied().unwrap_or(false)
    }

    /// The grabbed row's top before the drag (the ghost starts there).
    pub const fn grabbed_top(&self) -> f32 {
        self.grabbed_top
    }
}

/// The scroll step for the pointer at `pointer` (0 = the list's top edge) in a list `view` high: negative scrolls up,
/// positive down, 0 away from the edges; it grows toward the edge (squared) and is the fastest at and past it.
pub fn auto_scroll(pointer: f32, view: f32) -> f32 {
    let view = finite(view).max(0.0);
    let pointer = finite(pointer);
    let zone = EDGE_ZONE.min(view / 4.0);
    if zone <= 0.0 {
        return 0.0;
    }
    let speed = |depth: f32| {
        let t = (depth / zone).clamp(0.0, 1.0);
        MAX_STEP * t * t
    };
    if pointer < zone {
        -speed(zone - pointer)
    } else if pointer > view - zone {
        speed(pointer - (view - zone))
    } else {
        0.0
    }
}

fn tops(heights: &[f32], spacing: f32) -> Vec<f32> {
    let mut y = 0.0;
    heights
        .iter()
        .map(|h| {
            let top = y;
            y += h + spacing;
            top
        })
        .collect()
}

const fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Five rows 40 high with 2 between them (a pitch of 42).
    fn five(dragged: &[usize], grabbed: usize, grab_y: f32) -> ListDrag {
        let mask = (0..5).map(|i| dragged.contains(&i)).collect();
        ListDrag::new(vec![40.0; 5], 2.0, mask, grabbed, grab_y).unwrap()
    }

    #[test]
    fn nothing_moves_while_the_row_is_held_at_its_place() {
        let d = five(&[2], 2, 84.0 + 10.0);
        let f = d.frame(94.0);
        assert_eq!((f.drop, f.moves), (3, false), "{f:?}");
        assert!(f.shifts.iter().all(|s| s.abs() < 0.01));
        assert!((f.ghost_top - 84.0).abs() < 0.01);
        assert!((f.gap_top - 84.0).abs() < 0.01 && (f.gap_height - 42.0).abs() < 0.01);
    }

    #[test]
    fn dragging_down_opens_the_gap_below_and_the_rows_between_move_up() {
        // Row 1 grabbed in its middle and moved down by two pitches: it lands after row 3.
        let d = five(&[1], 1, 42.0 + 20.0);
        let f = d.frame(42.0 + 20.0 + 2.0 * 42.0);
        assert_eq!(f.drop, 4, "before row 4");
        assert!(f.moves);
        assert_eq!(f.shifts, vec![0.0, 84.0, -42.0, -42.0, 0.0]);
        assert!((f.gap_top - 126.0).abs() < 0.01);
        // Three pitches: the end.
        assert_eq!(d.frame(42.0 + 20.0 + 3.0 * 42.0).drop, 5);
    }

    #[test]
    fn dragging_up_moves_the_rows_between_down() {
        let d = five(&[3], 3, 126.0 + 5.0);
        let f = d.frame(5.0);
        assert_eq!(f.drop, 0);
        assert_eq!(f.shifts, vec![42.0, 42.0, 42.0, -126.0, 0.0]);
    }

    #[test]
    fn a_selection_moves_as_one_block_in_its_order() {
        // Rows 0 and 3 dragged by row 3: the block is 84 high, row 0 first.
        let d = five(&[0, 3], 3, 126.0 + 1.0);
        assert_eq!(d.count(), 2);
        assert!((d.block_height() - 84.0).abs() < 0.01);
        let f = d.frame(84.0 + 42.0 + 1.0);
        // The ghost's top: the pointer minus the grab and row 0 above the grabbed row.
        assert!((f.ghost_top - 84.0).abs() < 0.01, "{f:?}");
        assert_eq!(f.drop, 4, "between rows 2 and 4");
        assert_eq!(f.shifts, vec![84.0, -42.0, -42.0, 0.0, 0.0]);
    }

    #[test]
    fn the_ghost_stays_inside_the_list() {
        let d = five(&[4], 4, 168.0 + 20.0);
        assert!((d.frame(10_000.0).ghost_top - 168.0).abs() < 0.01);
        let f = d.frame(-500.0);
        assert!(f.ghost_top.abs() < 0.01);
        assert_eq!(f.drop, 0);
    }

    #[test]
    fn rows_of_different_heights_open_a_gap_as_high_as_the_block() {
        let d = ListDrag::new(vec![20.0, 60.0, 30.0], 4.0, vec![false, true, false], 1, 30.0).unwrap();
        let f = d.frame(30.0 + 200.0);
        assert_eq!(f.drop, 3);
        assert_eq!(f.shifts, vec![0.0, 34.0, -64.0]);
        assert!((f.gap_height - 64.0).abs() < 0.01);
    }

    #[test]
    fn a_drag_needs_the_grabbed_row_among_the_dragged_ones() {
        assert!(ListDrag::new(vec![40.0; 3], 2.0, vec![true, false, false], 1, 50.0).is_none());
        assert!(ListDrag::new(vec![40.0; 3], 2.0, vec![true; 2], 0, 0.0).is_none());
        assert!(ListDrag::new(Vec::new(), 2.0, Vec::new(), 0, 0.0).is_none());
    }

    #[test]
    fn the_list_scrolls_faster_toward_the_edges() {
        assert!(auto_scroll(200.0, 400.0).abs() < 0.01, "the middle stands still");
        let near = auto_scroll(40.0, 400.0);
        let nearer = auto_scroll(10.0, 400.0);
        assert!(near < 0.0 && nearer < near, "{near} {nearer}");
        assert!(
            (auto_scroll(-30.0, 400.0) + MAX_STEP).abs() < 0.01,
            "past the edge: the fastest"
        );
        assert!((auto_scroll(430.0, 400.0) - MAX_STEP).abs() < 0.01);
        assert!(auto_scroll(390.0, 400.0) > 0.0);
        assert!(auto_scroll(10.0, 0.0).abs() < 0.01, "no list, no scrolling");
        assert!(auto_scroll(f32::NAN, 400.0) < 0.0, "odd input is the top edge");
    }

    proptest! {
        #[test]
        fn the_rows_after_a_drop_never_overlap(
            heights in proptest::collection::vec(10.0f32..80.0, 1..12),
            pick in proptest::collection::vec(any::<bool>(), 12),
            grabbed in 0usize..12,
            pointer in -100.0f32..1200.0,
        ) {
            let n = heights.len();
            let grabbed = grabbed % n;
            let mut dragged: Vec<bool> = pick.into_iter().take(n).collect();
            if let Some(g) = dragged.get_mut(grabbed) {
                *g = true;
            }
            let d = ListDrag::new(heights.clone(), 2.0, dragged, grabbed, 5.0).unwrap();
            let f = d.frame(pointer);
            let old = tops(&heights, 2.0);
            let mut spans: Vec<(f32, f32)> = old
                .iter()
                .zip(&f.shifts)
                .zip(&heights)
                .map(|((top, shift), h)| (top + shift, top + shift + h))
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in spans.windows(2) {
                prop_assert!(pair[0].1 <= pair[1].0 + 0.01, "{:?}", spans);
            }
            prop_assert!(f.drop <= n);
        }
    }
}
