//! The status bar's activities (pure): a thing that runs shows a throbber and its text, then its result for a few
//! seconds, then goes. Nothing here knows auto-crop; the moves of target files use it the same way (P12 block 7).

use std::time::{Duration, Instant};

use crate::ui::notices::Level;

/// How long a result stays.
pub const RESULT_TIME: Duration = Duration::from_secs(5);
/// An error stays longer: there is more to read.
pub const ERROR_TIME: Duration = Duration::from_secs(10);
/// What a result keeps at least after the pointer left it.
const AFTER_HOVER: Duration = Duration::from_millis(1500);
/// The fade before a segment is removed (Slint draws it).
pub const FADE_TIME: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    /// The result shows until `Some(time)`; `None` while the pointer rests on it.
    Result {
        level: Level,
        until: Option<Instant>,
        left: Duration,
    },
    Leaving(Instant),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: u64,
    pub key: String,
    pub text: String,
    pub cancelable: bool,
    phase: Phase,
}

impl Item {
    /// The level of a result; `None` while it runs.
    pub const fn level(&self) -> Option<Level> {
        match self.phase {
            Phase::Result { level, .. } => Some(level),
            Phase::Running | Phase::Leaving(_) => None,
        }
    }

    pub const fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running)
    }

    pub const fn is_leaving(&self) -> bool {
        matches!(self.phase, Phase::Leaving(_))
    }

    fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Running => None,
            Phase::Result { until, .. } => until,
            Phase::Leaving(at) => Some(at),
        }
    }
}

/// Every segment shown, oldest first: each keeps its place while another comes and goes.
#[derive(Debug, Default)]
pub struct Board {
    items: Vec<Item>,
    next_id: u64,
}

impl Board {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Whether anything waits for a time (the timer runs only then).
    pub fn is_timed(&self) -> bool {
        self.items.iter().any(|i| i.deadline().is_some())
    }

    fn find(&mut self, key: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.key == key)
    }

    /// `key` starts; a segment it already has runs on in its place.
    pub fn begin(&mut self, key: &str, text: String, cancelable: bool) {
        if let Some(item) = self.find(key) {
            item.text = text;
            item.cancelable = cancelable;
            item.phase = Phase::Running;
            return;
        }
        self.next_id += 1;
        self.items.push(Item {
            id: self.next_id,
            key: key.to_owned(),
            text,
            cancelable,
            phase: Phase::Running,
        });
    }

    /// New words while it runs.
    pub fn update(&mut self, key: &str, text: String) -> bool {
        match self.find(key) {
            Some(item) if item.is_running() && item.text != text => {
                item.text = text;
                true
            }
            _ => false,
        }
    }

    /// `key` is over: its result shows, in the segment it has or in a new one.
    pub fn result(&mut self, key: &str, level: Level, text: String, now: Instant) {
        self.begin(key, text, false);
        let left = if level == Level::Error { ERROR_TIME } else { RESULT_TIME };
        if let Some(item) = self.find(key) {
            item.phase = Phase::Result {
                level,
                until: Some(now + left),
                left,
            };
        }
    }

    /// `key` ended without a result here (the result went elsewhere, or it was canceled); a result stays.
    pub fn finish(&mut self, key: &str, now: Instant) {
        if let Some(item) = self.find(key)
            && item.is_running()
        {
            item.phase = Phase::Leaving(now + FADE_TIME);
        }
    }

    /// The pointer rests on a result (its time stands) or left it.
    pub fn hover(&mut self, id: u64, on: bool, now: Instant) {
        let Some(item) = self.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        if let Phase::Result { until, left, .. } = &mut item.phase {
            match (on, *until) {
                (true, Some(at)) => {
                    *left = at.saturating_duration_since(now);
                    *until = None;
                }
                (false, None) => *until = Some(now + (*left).max(AFTER_HOVER)),
                _ => {}
            }
        }
    }

    /// What is due: a result starts to fade, a faded segment goes. `true` when something changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        self.items.retain_mut(|item| {
            let due = item.deadline().is_some_and(|at| at <= now);
            if due {
                changed = true;
                if item.is_leaving() {
                    return false;
                }
                item.phase = Phase::Leaving(now + FADE_TIME);
            }
            true
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gone(board: &Board) -> bool {
        board.items().is_empty()
    }

    fn pass(board: &mut Board, now: Instant) {
        board.tick(now);
        board.tick(now + FADE_TIME);
    }

    #[test]
    fn a_result_shows_for_five_seconds_then_fades_and_goes() {
        let now = Instant::now();
        let mut board = Board::default();
        board.begin("crop", "Waiting".into(), true);
        assert!(!board.is_timed());
        board.result("crop", Level::Success, "Done".into(), now);
        let item = board.items().first().unwrap();
        assert_eq!(item.level(), Some(Level::Success));
        assert!(!item.cancelable, "a result cannot be canceled");
        board.tick(now + Duration::from_millis(4999));
        assert!(board.items().first().unwrap().level().is_some());
        board.tick(now + RESULT_TIME);
        assert!(board.items().first().unwrap().is_leaving());
        board.tick(now + RESULT_TIME + FADE_TIME);
        assert!(gone(&board));
    }

    #[test]
    fn an_error_stays_longer() {
        let now = Instant::now();
        let mut board = Board::default();
        board.result("crop", Level::Error, "Failed".into(), now);
        board.tick(now + RESULT_TIME);
        assert_eq!(board.items().first().unwrap().level(), Some(Level::Error));
        pass(&mut board, now + ERROR_TIME);
        assert!(gone(&board));
    }

    #[test]
    fn the_pointer_stops_the_time_and_leaves_enough_to_finish_reading() {
        let now = Instant::now();
        let mut board = Board::default();
        board.result("crop", Level::Success, "Done".into(), now);
        let id = board.items().first().unwrap().id;
        board.hover(id, true, now + Duration::from_millis(4900));
        board.tick(now + Duration::from_secs(60));
        assert!(board.items().first().unwrap().level().is_some());
        let left = now + Duration::from_secs(61);
        board.hover(id, false, left);
        board.tick(left + Duration::from_secs(1));
        assert!(board.items().first().unwrap().level().is_some());
        pass(&mut board, left + AFTER_HOVER);
        assert!(gone(&board));
    }

    #[test]
    fn finishing_leaves_a_result_alone_and_ends_what_still_runs() {
        let now = Instant::now();
        let mut board = Board::default();
        board.result("crop", Level::Success, "Done".into(), now);
        board.finish("crop", now);
        assert!(board.items().first().unwrap().level().is_some());
        board.begin("move", "Moving".into(), false);
        board.finish("move", now);
        assert!(board.items().get(1).unwrap().is_leaving());
    }

    #[test]
    fn a_new_start_runs_on_in_its_segment_and_several_stand_side_by_side() {
        let now = Instant::now();
        let mut board = Board::default();
        board.begin("crop", "Reading".into(), true);
        let id = board.items().first().unwrap().id;
        board.begin("move", "Moving".into(), false);
        board.begin("crop", "Waiting".into(), true);
        assert_eq!(board.items().len(), 2);
        assert_eq!(board.items().first().unwrap().id, id);
        assert!(board.update("crop", "Waiting 3/8".into()));
        assert!(
            !board.update("crop", "Waiting 3/8".into()),
            "the same words change nothing"
        );
        board.result("crop", Level::Success, "Done".into(), now);
        assert!(
            !board.update("crop", "Late".into()),
            "a late update does not replace the result"
        );
    }
}
