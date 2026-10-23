//! Which of winit's file drag & drop events opens a file. winit reports one event per file: a drag over the window sends
//! `HoveredFile` for each file, a drop sends `DroppedFile` for each. Only the first file of a drop is opened (the editor
//! holds one video); the hint over the video says so while several files are dragged.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `DroppedFile` events further apart than this belong to separate drops (when no `HoveredFile` came in between).
const SAME_DROP: Duration = Duration::from_millis(500);

/// What the UI does for one event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// Show the drop hint for `files` dragged files.
    Hint { files: u32 },
    /// Hide the hint and open this file.
    Open(PathBuf),
    /// Hide the hint; a further file of a drop that already opened one.
    Ignore,
    /// Hide the hint (the drag left the window).
    Clear,
}

#[derive(Debug, Default)]
pub struct DropTracker {
    /// Files hovering in the current drag, 0 outside a drag.
    hovered: u32,
    /// When the last file of a drop was opened.
    last_drop: Option<Instant>,
}

impl DropTracker {
    pub fn hovered(&mut self) -> DropAction {
        if self.hovered == 0 {
            // A new drag: the next drop opens a file again.
            self.last_drop = None;
        }
        self.hovered = self.hovered.saturating_add(1);
        DropAction::Hint { files: self.hovered }
    }

    pub fn cancelled(&mut self) -> DropAction {
        self.hovered = 0;
        DropAction::Clear
    }

    pub fn dropped(&mut self, path: &Path, now: Instant) -> DropAction {
        self.hovered = 0;
        let same_drop = self
            .last_drop
            .is_some_and(|t| now.saturating_duration_since(t) < SAME_DROP);
        self.last_drop = Some(now);
        if same_drop {
            DropAction::Ignore
        } else {
            DropAction::Open(path.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_file_of_a_drop_opens() {
        let mut t = DropTracker::default();
        let now = Instant::now();
        assert_eq!(t.hovered(), DropAction::Hint { files: 1 });
        assert_eq!(t.hovered(), DropAction::Hint { files: 2 });
        assert_eq!(t.dropped(Path::new("/a.mkv"), now), DropAction::Open("/a.mkv".into()));
        assert_eq!(t.dropped(Path::new("/b.mkv"), now), DropAction::Ignore);
        // The next drag starts over, even right away.
        assert_eq!(t.hovered(), DropAction::Hint { files: 1 });
        assert_eq!(t.dropped(Path::new("/c.mkv"), now), DropAction::Open("/c.mkv".into()));
    }

    #[test]
    fn drops_without_hover_events_are_told_apart_by_time() {
        let mut t = DropTracker::default();
        let start = Instant::now();
        assert!(matches!(t.dropped(Path::new("/a.mkv"), start), DropAction::Open(_)));
        assert_eq!(
            t.dropped(Path::new("/b.mkv"), start + Duration::from_millis(5)),
            DropAction::Ignore
        );
        assert!(matches!(
            t.dropped(Path::new("/c.mkv"), start + Duration::from_secs(2)),
            DropAction::Open(_)
        ));
    }

    #[test]
    fn a_drag_that_leaves_clears_the_hint() {
        let mut t = DropTracker::default();
        t.hovered();
        assert_eq!(t.cancelled(), DropAction::Clear);
        assert_eq!(t.hovered(), DropAction::Hint { files: 1 });
    }
}
