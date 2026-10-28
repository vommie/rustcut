//! The Tag & Rate panel's state, without Slint: the tag tree as RustView last sent it, the tags and rating chosen for the
//! next job (the draft) or for a queued job being edited (user decision Q-G3), locked tags that stay for the next jobs
//! (Q-G2), hidden branches (chosen in the tag filter window, which edits a copy until Apply), the filter, the expanded
//! branches and the tag history. Every operation is a method; the bridge only shows what [`TagPanel::rows`] and the
//! other views say.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rustcut_core::job::{JobId, JobMetadata};
use rustcut_core::rustview::{TagItem, TagTree};
use rustcut_core::settings::TagPanelState;

use super::hidden::Marks;
use super::history::{TagHistoryItem, TagHistoryManager};
use super::tree::{TagTreeBuilder, TagTreeNode, descendants};

mod filtering;

/// Tags and rating of one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    /// In the order they were assigned.
    pub tags: Vec<u32>,
    /// `None` = no rating chosen.
    pub rating: Option<u8>,
}

impl Choice {
    fn toggle(&mut self, id: u32) -> bool {
        if let Some(at) = self.tags.iter().position(|t| *t == id) {
            self.tags.remove(at);
            false
        } else {
            self.tags.push(id);
            true
        }
    }

    fn add(&mut self, ids: &[u32]) {
        for &id in ids {
            if !self.tags.contains(&id) {
                self.tags.push(id);
            }
        }
    }
}

/// A queued job whose RustView entry is being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub id: JobId,
    pub name: String,
    /// What the job holds (its source hash stays).
    pub original: JobMetadata,
    pub choice: Choice,
}

impl Editing {
    fn original_choice(&self) -> Choice {
        Choice {
            tags: self.original.tags.clone(),
            rating: self.original.rating,
        }
    }
}

#[derive(Debug, Default)]
pub struct TagPanel {
    tree: Option<Arc<TagTree>>,
    forest: Vec<TagTreeNode>,
    by_id: HashMap<u32, TagItem>,
    pub expanded: HashSet<u32>,
    pub filter: String,
    pub regex: bool,
    pub history: Vec<u32>,
    /// Tags assigned through the history's pills that wait to move to its front (oldest click first, `settle_history`).
    pub history_pending: Vec<u32>,
    pub history_collapsed: bool,
    pub history_height: u32,
    /// The hidden branches the tree leaves out.
    pub marks: Marks,
    /// The tag filter window's copy of the marks while it is open (Apply makes it `marks`, D-P9n).
    pub filter_draft: Option<Marks>,
    pub locked: Vec<u32>,
    pub last_tags: Vec<u32>,
    pub last_rating: Option<u8>,
    /// The next job's tags and rating.
    pub draft: Choice,
    pub editing: Option<Editing>,
    /// The eye (P9.8): the tree shows the hidden tags too, dimmed.
    pub show_hidden: bool,
    /// The filter field shows; hidden, its text is kept but does not filter.
    pub search_shown: bool,
    /// The "Active" switch (user decision Q-P8b): off = the next jobs are saved without RustView.
    pub active: bool,
    /// The tag filter window: its folded branches and its size.
    pub filter_collapsed: HashSet<u32>,
    pub filter_size: (u32, u32),
    /// How many tags the history keeps (`rustview.tag_history_size`).
    history_size: usize,
}

impl TagPanel {
    pub fn new(saved: &TagPanelState) -> Self {
        Self {
            expanded: saved.expanded.iter().copied().collect(),
            filter: saved.filter.clone(),
            regex: saved.regex,
            history: saved.history.clone(),
            history_collapsed: saved.history_collapsed,
            history_height: saved.history_height,
            marks: Marks {
                hidden: saved.hidden.iter().copied().collect(),
                shown: saved.shown.iter().copied().collect(),
            },
            locked: saved.locked.clone(),
            last_tags: saved.last_tags.clone(),
            last_rating: saved.last_rating,
            draft: Choice {
                tags: saved.locked.clone(),
                rating: None,
            },
            active: saved.active,
            show_hidden: saved.show_hidden,
            search_shown: saved.search_shown,
            filter_collapsed: saved.filter_collapsed.iter().copied().collect(),
            filter_size: (saved.filter_width, saved.filter_height),
            history_size: TagPanelState::HISTORY_DEFAULT,
            ..Self::default()
        }
    }

    /// The history keeps this many tags from now on (the oldest go when it is shorter now).
    pub fn set_history_size(&mut self, size: usize) {
        self.history_size = size.clamp(1, TagPanelState::HISTORY_MAX);
        self.history.truncate(self.history_size);
    }

    /// What `ui-state.toml` keeps (sets sorted, so the file does not change on every save).
    pub fn save_into(&self, saved: &mut TagPanelState) {
        let sorted = |set: &HashSet<u32>| {
            let mut v: Vec<u32> = set.iter().copied().collect();
            v.sort_unstable();
            v
        };
        *saved = TagPanelState {
            expanded: sorted(&self.expanded),
            filter: self.filter.clone(),
            regex: self.regex,
            history: self.settled_history(),
            history_collapsed: self.history_collapsed,
            history_height: self.history_height,
            hidden: sorted(&self.marks.hidden),
            shown: sorted(&self.marks.shown),
            locked: self.locked.clone(),
            last_tags: self.last_tags.clone(),
            last_rating: self.last_rating,
            active: self.active,
            filter_collapsed: sorted(&self.filter_collapsed),
            filter_width: self.filter_size.0,
            filter_height: self.filter_size.1,
            show_hidden: self.show_hidden,
            search_shown: self.search_shown,
        };
    }

    /// RustView sent its tags: ids it does not know any more are dropped everywhere (the edited job keeps its own until
    /// it is applied, so nothing changes in it unasked).
    pub fn set_tree(&mut self, tree: Arc<TagTree>) {
        self.forest = TagTreeBuilder::build_hierarchy(&tree.tags);
        self.by_id = tree.tags.iter().map(|t| (t.id, t.clone())).collect();
        let valid: HashSet<u32> = self.by_id.keys().copied().collect();
        for set in [&mut self.expanded, &mut self.filter_collapsed] {
            set.retain(|id| valid.contains(id));
        }
        self.marks.retain(&valid);
        if let Some(draft) = &mut self.filter_draft {
            draft.retain(&valid);
        }
        TagHistoryManager::prune_invalid(&mut self.history, &valid);
        TagHistoryManager::prune_invalid(&mut self.history_pending, &valid);
        for list in [&mut self.locked, &mut self.last_tags, &mut self.draft.tags] {
            list.retain(|id| valid.contains(id));
        }
        self.tree = Some(tree);
    }

    pub fn has_tree(&self) -> bool {
        self.tree.is_some()
    }

    pub fn tag_count(&self) -> usize {
        self.by_id.len()
    }

    /// RustView's tags as it sent them.
    pub fn tags(&self) -> impl Iterator<Item = &TagItem> {
        self.tree.iter().flat_map(|t| t.tags.iter())
    }

    pub fn tag(&self, id: u32) -> Option<&TagItem> {
        self.by_id.get(&id)
    }

    /// Every tag below `id`.
    pub fn below(&self, id: u32) -> HashSet<u32> {
        self.tree
            .as_ref()
            .map(|tree| descendants(&tree.tags, id))
            .unwrap_or_default()
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(|t| t.name.trim())
    }

    /// The choice the panel edits now.
    pub fn choice(&self) -> &Choice {
        self.editing.as_ref().map_or(&self.draft, |e| &e.choice)
    }

    fn choice_mut(&mut self) -> &mut Choice {
        match &mut self.editing {
            Some(e) => &mut e.choice,
            None => &mut self.draft,
        }
    }

    /// A click on a tag in the tree or (`from_history`) a pill of the tag history: assigns or unassigns it. Unassigning
    /// a locked tag unlocks it. A pill stays where it is until the history settles; a tag from the tree moves to the
    /// front at once, together with the waiting ones.
    pub fn click_tag(&mut self, id: u32, from_history: bool) {
        if !self.by_id.contains_key(&id) {
            return;
        }
        let assigned = self.choice_mut().toggle(id);
        if assigned && from_history {
            TagHistoryManager::defer(&mut self.history_pending, id);
        } else if assigned {
            self.settle_history();
            TagHistoryManager::record_tag_used(&mut self.history, id, self.history_size);
        } else if self.editing.is_none() {
            self.locked.retain(|t| *t != id);
        }
    }

    /// A right click locks the tag (and assigns it) or unlocks it (it stays assigned for this job). Only for the draft:
    /// a lock is about the next jobs.
    pub fn toggle_lock(&mut self, id: u32) -> bool {
        if self.editing.is_some() || !self.by_id.contains_key(&id) {
            return false;
        }
        if let Some(at) = self.locked.iter().position(|t| *t == id) {
            self.locked.remove(at);
        } else {
            self.locked.push(id);
            if !self.draft.tags.contains(&id) {
                self.draft.tags.push(id);
                self.settle_history();
                TagHistoryManager::record_tag_used(&mut self.history, id, self.history_size);
            }
        }
        true
    }

    /// Ctrl+0..5: chooses `rating` (a key never clears it, unlike a second click); `false` when nothing can be tagged now
    /// (no video and no job edited, or "Active" off for the draft).
    pub fn rate_key(&mut self, rating: u8, has_video: bool) -> bool {
        let taggable = self.editing.is_some() || (has_video && self.active);
        if taggable && rating <= 5 {
            self.choice_mut().rating = Some(rating);
        }
        taggable && rating <= 5
    }

    /// A click on a rating button: chooses it, or clears it when it was the chosen one.
    pub fn click_rating(&mut self, rating: i32) {
        let rating = u8::try_from(rating).ok().filter(|r| *r <= 5);
        let choice = self.choice_mut();
        choice.rating = if choice.rating == rating { None } else { rating };
    }

    /// Clears tags and rating; the draft keeps its locked tags.
    pub fn clear(&mut self) {
        let keep = if self.editing.is_some() {
            Vec::new()
        } else {
            self.locked.clone()
        };
        *self.choice_mut() = Choice {
            tags: keep,
            rating: None,
        };
    }

    pub fn can_clear(&self) -> bool {
        let choice = self.choice();
        let locked = if self.editing.is_some() { 0 } else { self.locked.len() };
        choice.rating.is_some() || choice.tags.len() > locked
    }

    pub fn apply_last_tags(&mut self) {
        let last = self.last_tags.clone();
        self.choice_mut().add(&last);
    }

    pub fn apply_last_rating(&mut self) {
        if let Some(rating) = self.last_rating {
            self.choice_mut().rating = Some(rating);
        }
    }

    pub fn toggle_expanded(&mut self, id: u32) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// The pointer left the history 2 s ago (or its panel was made again): the waiting tags move to the front;
    /// `true` when the order changed.
    pub fn settle_history(&mut self) -> bool {
        TagHistoryManager::settle(&mut self.history, &mut self.history_pending, self.history_size)
    }

    /// The history's order with the waiting tags in front (what a restart shows).
    fn settled_history(&self) -> Vec<u32> {
        let mut history = self.history.clone();
        TagHistoryManager::settle(&mut history, &mut self.history_pending.clone(), self.history_size);
        history
    }

    pub fn history_items(&self) -> Vec<TagHistoryItem> {
        let assigned: HashSet<u32> = self.choice().tags.iter().copied().collect();
        TagHistoryManager::items(&self.history, &self.by_id, &assigned)
    }

    /// Tags of the choice that a hidden branch keeps out of the tree (they are still assigned).
    pub fn assigned_in_hidden(&self) -> usize {
        if self.marks.hidden.is_empty() {
            return 0;
        }
        self.choice().tags.iter().filter(|t| !self.visible(**t)).count()
    }

    /// The draft as the next job's metadata (tags sorted, as RustView stores them).
    pub fn draft_metadata(&self) -> (Vec<u32>, Option<u8>) {
        let mut tags = self.draft.tags.clone();
        tags.sort_unstable();
        (tags, self.draft.rating)
    }

    /// A job was saved with `tags` and `rating`: they become "last", the draft starts again with the locked tags.
    pub fn saved(&mut self, tags: &[u32], rating: Option<u8>) {
        if !tags.is_empty() {
            self.last_tags = tags.to_vec();
        }
        if rating.is_some() {
            self.last_rating = rating;
        }
        self.draft = Choice {
            tags: self.locked.clone(),
            rating: None,
        };
    }

    /// A job loaded into the editor (or a draft put back): its tags and rating become the draft. Locked tags stay
    /// locked; an entry being edited is left.
    pub fn load_draft(&mut self, choice: Choice) {
        self.editing = None;
        self.draft = choice;
    }

    /// Starts editing a queued job's entry; the draft waits meanwhile.
    pub fn begin_edit(&mut self, id: JobId, name: String, metadata: JobMetadata) {
        let choice = Choice {
            tags: metadata.tags.clone(),
            rating: metadata.rating,
        };
        self.editing = Some(Editing {
            id,
            name,
            original: metadata,
            choice,
        });
    }

    /// The edited job's new metadata, when it differs from what the job holds.
    pub fn edited(&self) -> Option<(JobId, JobMetadata)> {
        let e = self.editing.as_ref()?;
        if e.choice == e.original_choice() {
            return None;
        }
        let mut tags = e.choice.tags.clone();
        tags.sort_unstable();
        Some((
            e.id,
            JobMetadata {
                tags,
                rating: e.choice.rating,
                source_hash: e.original.source_hash.clone(),
                without_rustview: e.original.without_rustview,
            },
        ))
    }

    /// Back to the draft (after Apply or Cancel).
    pub fn end_edit(&mut self) -> Option<Editing> {
        self.editing.take()
    }
}

#[cfg(test)]
mod tests;
