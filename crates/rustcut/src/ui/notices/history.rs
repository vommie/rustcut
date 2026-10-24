//! The messages of this session (pure, Q-N8: kept for this session only): the bell shows the last three and counts the
//! unread ones, the Logs panel's "Messages" source lists them all.

use std::collections::VecDeque;

use rustcut_core::settings::BellBadge;

use super::kind::{Kind, Level};

/// The history keeps the newest this many.
pub const KEPT: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Counts every entry of the session (the Logs panel follows it).
    pub seq: u64,
    /// "15:42:07".
    pub time: String,
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    /// Shown nowhere ("Don't show again", or the kind's place is "nothing").
    pub silent: bool,
    pub read: bool,
}

#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    seq: u64,
}

impl History {
    pub fn push(&mut self, time: String, kind: Kind, level: Level, title: String, detail: String, silent: bool) {
        self.seq += 1;
        if self.entries.len() == KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            seq: self.seq,
            time,
            kind,
            level,
            title,
            detail,
            silent,
            read: false,
        });
    }

    /// The number of the last entry (0 = none yet).
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The newest `n`, newest first.
    pub fn recent(&self, n: usize) -> Vec<&Entry> {
        self.entries.iter().rev().take(n).collect()
    }

    /// The number on the bell and the level of its color.
    pub fn unread(&self, badge: BellBadge) -> (usize, Level) {
        let counted = |e: &&Entry| {
            !e.read
                && match badge {
                    BellBadge::ErrorsWarnings => e.level >= Level::Warning,
                    BellBadge::All => true,
                    BellBadge::None => false,
                }
        };
        let unread: Vec<&Entry> = self.entries.iter().filter(counted).collect();
        let level = unread.iter().map(|e| e.level).max().unwrap_or(Level::Info);
        (unread.len(), level)
    }

    pub fn mark_read(&mut self) {
        for entry in &mut self.entries {
            entry.read = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bell_counts_what_its_setting_says_and_keeps_the_newest() {
        let mut history = History::default();
        history.push(
            "1".into(),
            Kind::JobQueued,
            Level::Success,
            "a".into(),
            String::new(),
            false,
        );
        history.push(
            "2".into(),
            Kind::SectionRefused,
            Level::Warning,
            "b".into(),
            String::new(),
            false,
        );
        history.push(
            "3".into(),
            Kind::JobFailed,
            Level::Error,
            "c".into(),
            String::new(),
            true,
        );
        assert_eq!(history.unread(BellBadge::ErrorsWarnings), (2, Level::Error));
        assert_eq!(history.unread(BellBadge::All).0, 3);
        assert_eq!(history.unread(BellBadge::None).0, 0);
        let recent: Vec<&str> = history.recent(2).iter().map(|e| e.title.as_str()).collect();
        assert_eq!(recent, ["c", "b"]);
        history.mark_read();
        assert_eq!(history.unread(BellBadge::All), (0, Level::Info));
        for i in 0..KEPT {
            history.push(
                i.to_string(),
                Kind::Ready,
                Level::Info,
                i.to_string(),
                String::new(),
                false,
            );
        }
        assert_eq!(history.len(), KEPT);
        assert_eq!(history.seq(), 3 + u64::try_from(KEPT).unwrap());
        assert_eq!(history.entries().next().unwrap().title, "0");
    }
}
