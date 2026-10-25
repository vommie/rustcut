//! The banners (pure, Q-N8): states over the top of the video that stay while they hold. One shows at a time, the
//! most important first, with "‹ 1 / 3 ›" for the others; × hides a banner until its state changes (another title,
//! detail or level; a progress alone is no change). Event kinds routed to a banner use their kind id as the key, so
//! the next event of the kind replaces the last one.

use super::kind::{Kind, Level};
use super::notice::Label;

#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub key: String,
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub actions: Vec<Label>,
    pub clickable: bool,
    pub progress: Option<f32>,
    /// "since 15:40".
    pub since: String,
    seq: u64,
    revision: u64,
}

/// A banner to show or update.
#[derive(Debug, Clone, PartialEq)]
pub struct NewBanner {
    pub key: String,
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub actions: Vec<Label>,
    pub clickable: bool,
    pub progress: Option<f32>,
    pub since: String,
}

/// What [`BannerBoard::set`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    New,
    /// Its words or level changed (a hidden one shows again).
    Changed,
    /// Only the progress or the buttons changed.
    Same,
}

#[derive(Debug, Default)]
pub struct BannerBoard {
    banners: Vec<Banner>,
    /// Hidden with ×: key and the revision it was hidden at.
    hidden: Vec<(String, u64)>,
    /// The key of the banner the pager shows (it stays put while others come and go).
    current: Option<String>,
    next_seq: u64,
}

impl BannerBoard {
    pub fn set(&mut self, new: NewBanner) -> Set {
        self.next_seq += 1;
        let seq = self.next_seq;
        if let Some(banner) = self.banners.iter_mut().find(|b| b.key == new.key) {
            let changed = banner.title != new.title || banner.detail != new.detail || banner.level != new.level;
            banner.kind = new.kind;
            banner.actions = new.actions;
            banner.clickable = new.clickable;
            banner.progress = new.progress;
            if !changed {
                return Set::Same;
            }
            banner.title = new.title;
            banner.detail = new.detail;
            banner.level = new.level;
            banner.since = new.since;
            banner.seq = seq;
            banner.revision += 1;
            let (key, level) = (banner.key.clone(), banner.level);
            self.bring_forward(key, level);
            return Set::Changed;
        }
        self.bring_forward(new.key.clone(), new.level);
        self.banners.push(Banner {
            key: new.key,
            kind: new.kind,
            level: new.level,
            title: new.title,
            detail: new.detail,
            actions: new.actions,
            clickable: new.clickable,
            progress: new.progress,
            since: new.since,
            seq,
            revision: 0,
        });
        Set::New
    }

    /// A new or changed banner shows unless the one in front is more important (Q-N8: the most important one).
    fn bring_forward(&mut self, key: String, level: Level) {
        let front = self.current().map(|(b, _, _)| b.level);
        if front.is_none_or(|front| level >= front) {
            self.current = Some(key);
        }
    }

    /// The state is over.
    pub fn clear(&mut self, key: &str) -> bool {
        let before = self.banners.len();
        self.banners.retain(|b| b.key != key);
        self.hidden.retain(|(k, _)| k != key);
        before != self.banners.len()
    }

    /// × on the shown banner: hidden until it changes.
    pub fn hide(&mut self, key: &str) {
        if let Some(banner) = self.banners.iter().find(|b| b.key == key) {
            self.hidden.retain(|(k, _)| k != key);
            self.hidden.push((banner.key.clone(), banner.revision));
        }
    }

    /// The banners that show, most important first, then newest.
    fn shown(&self) -> Vec<&Banner> {
        let mut shown: Vec<&Banner> = self
            .banners
            .iter()
            .filter(|b| !self.hidden.iter().any(|(k, r)| *k == b.key && *r == b.revision))
            .collect();
        shown.sort_by(|a, b| b.level.cmp(&a.level).then(b.seq.cmp(&a.seq)));
        shown
    }

    /// The banner to draw and its place in the pager (index, count).
    pub fn current(&self) -> Option<(&Banner, usize, usize)> {
        let shown = self.shown();
        let index = self
            .current
            .as_ref()
            .and_then(|key| shown.iter().position(|b| &b.key == key))
            .unwrap_or(0);
        shown.get(index).map(|b| (*b, index, shown.len()))
    }

    /// ‹ or ›.
    pub fn page(&mut self, delta: i32) {
        let shown = self.shown();
        let n = shown.len();
        if n == 0 {
            return;
        }
        let index = self.current().map_or(0, |(_, i, _)| i);
        let step = usize::try_from(delta.unsigned_abs()).unwrap_or(0) % n;
        let next = if delta < 0 {
            (index + n - step) % n
        } else {
            (index + step) % n
        };
        self.current = shown.get(next).map(|b| b.key.clone());
    }

    #[cfg(test)]
    pub fn is_shown(&self, key: &str) -> bool {
        self.current().is_some_and(|(b, _, _)| b.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(key: &str, level: Level, title: &str) -> NewBanner {
        NewBanner {
            key: key.into(),
            kind: Kind::KnownSource,
            level,
            title: title.into(),
            detail: String::new(),
            actions: Vec::new(),
            clickable: false,
            progress: None,
            since: "12:00".into(),
        }
    }

    #[test]
    fn the_most_important_shows_and_the_pager_moves_through_the_rest() {
        let mut board = BannerBoard::default();
        assert_eq!(board.set(banner("probe", Level::Info, "Probe render")), Set::New);
        assert_eq!(board.set(banner("save", Level::Error, "Cannot be saved")), Set::New);
        assert_eq!(board.set(banner("known", Level::Warning, "Cut before")), Set::New);
        let (b, i, n) = board.current().unwrap();
        assert_eq!(
            (b.key.as_str(), i, n),
            ("save", 0, 3),
            "a less important one waits behind the error"
        );
        board.page(1);
        assert_eq!(board.current().unwrap().0.key, "known");
        board.page(1);
        assert_eq!(board.current().unwrap().0.key, "probe");
        board.page(-2);
        assert_eq!(board.current().unwrap().0.key, "save");
        assert!(board.clear("save"));
        assert_eq!(board.current().unwrap().0.key, "known", "the most important one left");
    }

    #[test]
    fn closing_hides_until_the_state_changes() {
        let mut board = BannerBoard::default();
        board.set(banner("probe", Level::Info, "Probe render"));
        board.hide("probe");
        assert!(board.current().is_none());
        let mut update = banner("probe", Level::Info, "Probe render");
        update.progress = Some(0.5);
        assert_eq!(board.set(update), Set::Same);
        assert!(board.current().is_none(), "a progress is no change");
        assert_eq!(
            board.set(banner("probe", Level::Info, "Probe render ready")),
            Set::Changed
        );
        assert!(board.is_shown("probe"));
        board.hide("probe");
        board.clear("probe");
        board.set(banner("probe", Level::Info, "Probe render ready"));
        assert!(board.is_shown("probe"), "a new state of the same key shows");
    }
}
