//! The bubbles without Slint (M6.5c, user decision Q-N10): one per key, each standing at the first of its anchors that
//! reported its place in the window's current epoch (an anchor that is gone stops reporting), what × hid until the
//! words change, and the rows the layers draw.

use std::collections::HashMap;

use crate::ui::notices::kind::{Kind, Level};

/// The scroll area that draws a bubble; its window follows from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    /// The Export panel's cards (sections, filters, the job's target).
    Export,
    /// The Codecs panel's cards.
    Codecs,
    /// The Settings window's page.
    Settings,
}

impl Area {
    /// The key the Slint layer has (the dock panels' keys).
    pub const fn id(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Codecs => "render-settings",
            Self::Settings => "settings",
        }
    }

    pub const fn window(self) -> Win {
        match self {
            Self::Export | Self::Codecs => Win::Main,
            Self::Settings => Win::Settings,
        }
    }
}

/// A window with its own `Bubbles` global.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Win {
    Main,
    Settings,
}

/// An icon button of a bubble (P8b: the export name check's ways out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BubbleButton {
    pub icon: crate::icons::Icon,
    pub tooltip: String,
}

/// What a bubble shows and where it may stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBubble {
    pub kind: Kind,
    pub level: Level,
    pub title: String,
    pub detail: String,
    /// What a click on it does ("Show the files"); empty = nothing.
    pub link: String,
    /// The link leads somewhere: it shows the jump symbol (P9.1).
    pub link_jump: bool,
    pub buttons: Vec<BubbleButton>,
    pub area: Area,
    /// The anchors in order of preference: it stands at the first one that is shown.
    pub anchors: Vec<String>,
}

impl NewBubble {
    fn words(&self) -> String {
        format!("{}\n{}", self.title, self.detail)
    }
}

/// An anchor's rectangle in window coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One row of a layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: u64,
    pub key: String,
    pub area: Area,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub link: String,
    pub link_jump: bool,
    pub buttons: Vec<BubbleButton>,
    pub place: Place,
    /// The epoch its anchor reported in (-1 = none yet: not drawn).
    pub seen: i32,
}

#[derive(Debug, Clone)]
struct Shown {
    id: u64,
    key: String,
    bubble: NewBubble,
}

#[derive(Debug, Default)]
pub struct BubbleBoard {
    /// In the order they came (ids rise).
    shown: Vec<Shown>,
    next: u64,
    /// The words × hid, by key: hidden while the words stay.
    hidden: HashMap<String, String>,
    /// The last report of every anchor that has a bubble: its place and epoch.
    places: HashMap<String, (Place, i32)>,
}

impl BubbleBoard {
    /// Shows `bubble` as the bubble of `key` (its words change in place when it shows already); `true` when the
    /// anchors that want a bubble changed.
    pub fn set(&mut self, key: &str, bubble: NewBubble) -> bool {
        let before = self.wanted_anchors();
        if self.hidden.get(key).is_some_and(|words| *words != bubble.words()) {
            self.hidden.remove(key);
        }
        if let Some(shown) = self.shown.iter_mut().find(|s| s.key == key) {
            shown.bubble = bubble;
        } else {
            self.next += 1;
            self.shown.push(Shown {
                id: self.next,
                key: key.to_owned(),
                bubble,
            });
        }
        self.settle(&before)
    }

    /// The problem of `key` is gone; `true` when the anchors that want a bubble changed.
    pub fn clear(&mut self, key: &str) -> bool {
        let before = self.wanted_anchors();
        self.shown.retain(|s| s.key != key);
        self.hidden.remove(key);
        self.settle(&before)
    }

    /// × on the bubble `id`: hidden until its words change.
    pub fn hide(&mut self, id: u64) -> bool {
        let before = self.wanted_anchors();
        if let Some(shown) = self.shown.iter().find(|s| s.id == id) {
            self.hidden.insert(shown.key.clone(), shown.bubble.words());
        }
        self.settle(&before)
    }

    /// Drops the bubbles `keep` refuses (their kind does not go to a bubble anymore).
    pub fn retain(&mut self, keep: impl Fn(&NewBubble) -> bool) -> bool {
        let before = self.wanted_anchors();
        self.shown.retain(|s| keep(&s.bubble));
        self.settle(&before)
    }

    /// An anchor reported its place; `true` when a row changed.
    pub fn moved(&mut self, anchor: &str, place: Place, epoch: i32) -> bool {
        if !self.wants(anchor) {
            return false;
        }
        self.places.insert(anchor.to_owned(), (place, epoch)) != Some((place, epoch))
    }

    /// Whether the anchor `key` has a bubble now (one that × did not hide).
    pub fn wants(&self, anchor: &str) -> bool {
        self.visible().any(|s| s.bubble.anchors.iter().any(|a| a == anchor))
    }

    /// The bubble of `id` and its key.
    pub fn get(&self, id: u64) -> Option<(&str, &NewBubble)> {
        self.shown
            .iter()
            .find(|s| s.id == id)
            .map(|s| (s.key.as_str(), &s.bubble))
    }

    /// The rows of `window`: each bubble at its first anchor that reported in `epoch` (else at its latest report,
    /// which the layer does not draw).
    pub fn rows(&self, window: Win, epoch: i32) -> Vec<Row> {
        self.visible()
            .filter(|s| s.bubble.area.window() == window)
            .map(|s| {
                let reports = s.bubble.anchors.iter().filter_map(|a| self.places.get(a));
                let (place, seen) = reports
                    .clone()
                    .find(|(_, seen)| *seen == epoch)
                    .or_else(|| reports.max_by_key(|(_, seen)| *seen))
                    .copied()
                    .unwrap_or((Place::default(), -1));
                Row {
                    id: s.id,
                    key: s.key.clone(),
                    area: s.bubble.area,
                    level: s.bubble.level,
                    title: s.bubble.title.clone(),
                    detail: s.bubble.detail.clone(),
                    link: s.bubble.link.clone(),
                    link_jump: s.bubble.link_jump,
                    buttons: s.bubble.buttons.clone(),
                    place,
                    seen,
                }
            })
            .collect()
    }

    fn visible(&self) -> impl Iterator<Item = &Shown> {
        self.shown
            .iter()
            .filter(|s| self.hidden.get(&s.key).is_none_or(|words| *words != s.bubble.words()))
    }

    fn wanted_anchors(&self) -> Vec<String> {
        let mut anchors: Vec<String> = self.visible().flat_map(|s| s.bubble.anchors.iter().cloned()).collect();
        anchors.sort();
        anchors.dedup();
        anchors
    }

    /// Forgets the places of anchors without a bubble; whether the wanted anchors changed.
    fn settle(&mut self, before: &[String]) -> bool {
        let now = self.wanted_anchors();
        self.places.retain(|anchor, _| now.binary_search(anchor).is_ok());
        now != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bubble(title: &str, anchors: &[&str]) -> NewBubble {
        NewBubble {
            kind: Kind::ValueInvalid,
            level: Level::Error,
            title: title.to_owned(),
            detail: String::new(),
            link: String::new(),
            link_jump: false,
            buttons: Vec::new(),
            area: Area::Codecs,
            anchors: anchors.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    const fn place(y: f32) -> Place {
        Place {
            x: 0.0,
            y,
            width: 10.0,
            height: 10.0,
        }
    }

    #[test]
    fn a_bubble_changes_in_place_and_goes_with_its_problem() {
        let mut board = BubbleBoard::default();
        assert!(board.set("a", bubble("Not a number", &["row"])));
        assert!(board.wants("row") && !board.wants("other"));
        assert!(board.moved("row", place(5.0), 1));
        assert!(!board.moved("row", place(5.0), 1), "the same report changes nothing");
        assert!(
            !board.moved("other", place(1.0), 1),
            "anchors without a bubble are ignored"
        );
        let id = board.rows(Win::Main, 1).first().unwrap().id;
        // New words: the same bubble (its id), no change of the anchors.
        assert!(!board.set("a", bubble("Too large", &["row"])));
        let rows = board.rows(Win::Main, 1);
        assert_eq!((rows.len(), rows[0].id, rows[0].title.as_str()), (1, id, "Too large"));
        assert!(board.rows(Win::Settings, 1).is_empty(), "another window's bubbles");
        assert!(board.clear("a"));
        assert!(board.rows(Win::Main, 1).is_empty() && !board.wants("row"));
    }

    #[test]
    fn it_stands_at_the_first_anchor_that_is_shown() {
        let mut board = BubbleBoard::default();
        board.set("issue", bubble("WebM cannot hold AAC", &["row", "profile"]));
        board.moved("profile", place(1.0), 3);
        board.moved("row", place(50.0), 3);
        let row = board.rows(Win::Main, 3).remove(0);
        assert_eq!((row.place.y, row.seen), (50.0, 3));
        // The row's card folded: only the profile choice reports in the next epoch.
        board.moved("profile", place(1.0), 4);
        let row = board.rows(Win::Main, 4).remove(0);
        assert_eq!((row.place.y, row.seen), (1.0, 4));
        // Nothing reported in epoch 5: the layer does not draw it.
        assert_ne!(board.rows(Win::Main, 5)[0].seen, 5);
    }

    #[test]
    fn the_close_button_hides_it_until_the_words_change() {
        let mut board = BubbleBoard::default();
        board.set("name", bubble("The file exists", &["name"]));
        let id = board.rows(Win::Main, 0)[0].id;
        assert!(board.hide(id));
        assert!(board.rows(Win::Main, 0).is_empty() && !board.wants("name"));
        assert!(
            !board.set("name", bubble("The file exists", &["name"])),
            "the same words stay hidden"
        );
        assert!(board.rows(Win::Main, 0).is_empty());
        assert!(board.set("name", bubble("Job 4 writes this file too", &["name"])));
        assert_eq!(board.rows(Win::Main, 0).len(), 1);
        // A problem that went and came back shows again.
        let id = board.rows(Win::Main, 0)[0].id;
        board.hide(id);
        board.clear("name");
        board.set("name", bubble("Job 4 writes this file too", &["name"]));
        assert_eq!(board.rows(Win::Main, 0).len(), 1);
    }
}
